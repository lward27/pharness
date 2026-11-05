use super::{
    kubectl_exec_args, select_session_pod, terminal_command, CampusTerminalAttacher,
    CampusTerminalError, CampusTerminalInput, CampusTerminalSize, CampusTerminalTarget,
    FakeCampusTerminal, KubectlCampusTerminal, KubectlCampusTerminalConfig, FAKE_AGENT_CLI_VERSION,
};
use crate::campus_lab_session::kubernetes::{CampusKubeApi, CampusKubeError, CampusKubeKind};
use crate::CampusWorkloadRef;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

const CONTROLLER: &str = "pharness-campus-controller";
const NAMESPACE: &str = "campus-lab-slot-1";

fn workload() -> CampusWorkloadRef {
    CampusWorkloadRef {
        session_id: "cls_0001".into(),
        slot_id: "campus-lab-slot-1".into(),
        slot_generation: 3,
    }
}

fn pod(name: &str, generation: i64, ready: bool) -> Value {
    json!({
        "metadata": {
            "name": name,
            "namespace": NAMESPACE,
            "labels": {
                "pharness.dev/campus-controller": CONTROLLER,
                "pharness.dev/campus-session": "cls_0001",
                "pharness.dev/campus-slot": "campus-lab-slot-1",
                "pharness.dev/campus-generation": generation.to_string(),
                "pharness.dev/campus-record": "workload",
            },
        },
        "spec": { "containers": [{ "name": "workspace" }] },
        "status": {
            "phase": "Running",
            "conditions": [{ "type": "Ready", "status": if ready { "True" } else { "False" } }],
        },
    })
}

#[test]
fn the_terminal_command_sets_the_session_process_limit_before_the_shell() {
    let argv = terminal_command(128, &["/bin/bash".to_string()]);
    assert_eq!(argv[0], "/bin/sh");
    assert_eq!(argv[1], "-c");
    assert_eq!(
        argv[2],
        "{ ulimit -u 128 || ulimit -p 128; } 2>/dev/null && ulimit -c 0 && \
         export TERM=xterm-256color && exec \"$@\""
    );
    assert_eq!(argv[3], "campus-terminal");
    assert_eq!(argv[4..], ["/bin/bash".to_string()]);
}

/// RLIMIT_NPROC counts every process of the UID on this host (inside a lab
/// it counts the sandbox only), so tests pick a limit above the host's
/// current use and no higher than the current hard limit.
fn testable_pids() -> u32 {
    let limits = std::fs::read_to_string("/proc/self/limits").unwrap();
    let hard = limits
        .lines()
        .find(|line| line.starts_with("Max processes"))
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|hard| hard.parse::<u32>().ok())
        .unwrap_or(u32::MAX);
    hard.saturating_sub(1).min(40_000)
}

fn run_sh(argv: &[String]) -> std::process::Output {
    std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .unwrap()
}

/// The wrapper really applies the limit on this host's `/bin/sh` (dash on
/// Debian and Ubuntu, which spells it `-p`).
#[test]
fn the_wrapper_applies_rlimit_nproc_and_disables_core_dumps() {
    let pids = testable_pids();
    let argv = terminal_command(
        pids,
        &[
            "/bin/sh".into(),
            "-c".into(),
            "grep -E '^Max (processes|core file size)' /proc/self/limits; echo TERM=$TERM".into(),
        ],
    );
    let output = run_sh(&argv);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let processes = stdout
        .lines()
        .find(|line| line.starts_with("Max processes"))
        .unwrap();
    let fields = processes.split_whitespace().collect::<Vec<_>>();
    let expected = pids.to_string();
    assert_eq!(
        fields[2..4],
        [expected.as_str(), expected.as_str()],
        "soft and hard: {processes}"
    );
    let core = stdout
        .lines()
        .find(|line| line.starts_with("Max core file size"))
        .unwrap();
    assert_eq!(core.split_whitespace().nth(4), Some("0"), "{core}");
    assert!(stdout.contains("TERM=xterm-256color"));
}

fn running_as_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|uids| uids.split_whitespace().nth(1))
        == Some("0")
}

/// When the limit cannot be set (here: a lower hard limit that an
/// unprivileged process may not raise), the shell never starts.
#[test]
fn no_shell_starts_when_the_limit_cannot_be_set() {
    if running_as_root() {
        return;
    }
    let mut argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "{ ulimit -u 40 || ulimit -p 40; } 2>/dev/null && exec \"$@\"".to_string(),
        "outer".to_string(),
    ];
    argv.extend(terminal_command(
        4000,
        &["/bin/echo".into(), "shell-started".into()],
    ));
    let output = run_sh(&argv);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("shell-started"));
}

#[test]
fn kubectl_exec_args_are_fixed_except_namespace_and_pod() {
    let args = kubectl_exec_args(
        &["--kubeconfig=/run/terminal/kubeconfig".to_string()],
        NAMESPACE,
        "campus-abc-x7k2p",
        &["/bin/sh".to_string(), "-c".to_string(), "true".to_string()],
    );
    assert_eq!(
        args,
        [
            "--kubeconfig=/run/terminal/kubeconfig",
            "exec",
            "--stdin",
            "--tty",
            "--namespace=campus-lab-slot-1",
            "--container=workspace",
            "campus-abc-x7k2p",
            "--",
            "/bin/sh",
            "-c",
            "true",
        ]
    );
}

#[test]
fn only_the_single_ready_pod_of_the_exact_occupancy_is_selected() {
    let workload = workload();
    let select = |pods: &[Value]| select_session_pod(CONTROLLER, NAMESPACE, &workload, pods);
    assert_eq!(select(&[pod("campus-a-1", 3, true)]).unwrap(), "campus-a-1");

    assert!(matches!(
        select(&[]),
        Err(CampusTerminalError::NotRunning(_))
    ));
    assert!(matches!(
        select(&[pod("campus-a-1", 3, true), pod("campus-a-2", 3, true)]),
        Err(CampusTerminalError::NotRunning(_))
    ));
    // Another occupancy of the slot is never a candidate.
    assert!(matches!(
        select(&[pod("campus-a-old", 2, true)]),
        Err(CampusTerminalError::NotRunning(_))
    ));
    assert!(matches!(
        select(&[pod("campus-a-1", 3, false)]),
        Err(CampusTerminalError::NotRunning(_))
    ));
    let mut terminating = pod("campus-a-1", 3, true);
    terminating["metadata"]["deletionTimestamp"] = json!("2030-01-01T00:00:00Z");
    assert!(matches!(
        select(&[terminating]),
        Err(CampusTerminalError::NotRunning(_))
    ));
    let mut elsewhere = pod("campus-a-1", 3, true);
    elsewhere["metadata"]["namespace"] = json!("kube-system");
    assert!(matches!(
        select(&[elsewhere]),
        Err(CampusTerminalError::NotRunning(_))
    ));
    let mut other_controller = pod("campus-a-1", 3, true);
    other_controller["metadata"]["labels"]["pharness.dev/campus-controller"] = json!("someone");
    assert!(matches!(
        select(&[other_controller]),
        Err(CampusTerminalError::NotRunning(_))
    ));
    let mut no_container = pod("campus-a-1", 3, true);
    no_container["spec"]["containers"] = json!([{ "name": "sidecar" }]);
    assert!(matches!(
        select(&[no_container]),
        Err(CampusTerminalError::Refused(_))
    ));
    let mut bad_name = pod("campus-a-1", 3, true);
    bad_name["metadata"]["name"] = json!("--as=system:admin");
    assert!(matches!(
        select(&[bad_name]),
        Err(CampusTerminalError::Refused(_))
    ));
}

fn config(kubectl: &str) -> KubectlCampusTerminalConfig {
    KubectlCampusTerminalConfig {
        kubectl: kubectl.into(),
        global_args: vec!["--context=campus-terminal".into()],
        controller: CONTROLLER.into(),
        slots: BTreeMap::from([("campus-lab-slot-1".into(), NAMESPACE.into())]),
        shell: vec!["/bin/sh".into()],
    }
}

#[test]
fn configuration_is_validated() {
    assert!(config("kubectl").validate().is_ok());
    let cases: [fn(&mut KubectlCampusTerminalConfig); 7] = [
        |c| c.kubectl = " ".into(),
        |c| c.controller = "not a label".into(),
        |c| c.slots.clear(),
        |c| {
            c.slots.insert("slot-2".into(), "Kube_System".into());
        },
        |c| c.shell = vec!["bash".into()],
        |c| c.global_args = vec!["exec".into()],
        |c| c.global_args = vec!["--".into()],
    ];
    for (index, mutate) in cases.into_iter().enumerate() {
        let mut candidate = config("kubectl");
        mutate(&mut candidate);
        assert!(candidate.validate().is_err(), "case {index}");
    }
}

#[derive(Default)]
struct FakePods {
    pods: Vec<Value>,
    selectors: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl CampusKubeApi for FakePods {
    async fn get(
        &self,
        _kind: CampusKubeKind,
        _namespace: &str,
        _name: &str,
    ) -> Result<Option<Value>, CampusKubeError> {
        Err(CampusKubeError::Rejected("get is not used".into()))
    }

    async fn list(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        selector: &str,
    ) -> Result<Vec<Value>, CampusKubeError> {
        assert_eq!(kind, CampusKubeKind::Pod);
        self.selectors
            .lock()
            .unwrap()
            .push((namespace.into(), selector.into()));
        Ok(self.pods.clone())
    }

    async fn create(&self, _manifest: &Value) -> Result<Value, CampusKubeError> {
        Err(CampusKubeError::Rejected("create is not used".into()))
    }

    async fn replace(&self, _manifest: &Value) -> Result<Value, CampusKubeError> {
        Err(CampusKubeError::Rejected("replace is not used".into()))
    }

    async fn delete(
        &self,
        _kind: CampusKubeKind,
        _namespace: &str,
        _name: &str,
    ) -> Result<(), CampusKubeError> {
        Err(CampusKubeError::Rejected("delete is not used".into()))
    }
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A stand-in kubectl that records its argv and pid, then runs the command
/// after `--` locally, so the wrapper and the pty run for real.
fn fake_kubectl() -> (TempDir, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "pharness-campus-terminal-{}-{}",
        std::process::id(),
        uuid_like()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let argv = dir.join("argv");
    let script = dir.join("kubectl");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{argv}'\necho $$ > '{argv}.pid'\n\
             while [ \"$#\" -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
            argv = argv.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (TempDir(dir), script, argv)
}

fn uuid_like() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

async fn read_until(output: &mut mpsc::Receiver<Vec<u8>>, needle: &str) -> String {
    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !seen.contains(needle) {
        match tokio::time::timeout_at(deadline, output.recv()).await {
            Ok(Some(chunk)) => seen.push_str(&String::from_utf8_lossy(&chunk)),
            Ok(None) => panic!("terminal closed before {needle:?}; saw {seen:?}"),
            Err(_) => panic!("timed out waiting for {needle:?}; saw {seen:?}"),
        }
    }
    seen
}

#[tokio::test]
async fn the_kubectl_terminal_runs_the_wrapper_under_a_tty_and_forwards_resizes() {
    let (_dir, script, argv_file) = fake_kubectl();
    let api = Arc::new(FakePods {
        pods: vec![pod("campus-a-1", 3, true)],
        ..FakePods::default()
    });
    let terminal =
        KubectlCampusTerminal::new(config(script.to_str().unwrap()), api.clone()).unwrap();
    let target = CampusTerminalTarget {
        workload: workload(),
        pids: testable_pids(),
        size: CampusTerminalSize::new(90, 30).unwrap(),
    };
    let mut stream = terminal.attach(&target).await.unwrap();
    stream
        .input
        .send(CampusTerminalInput::Data(
            b"[ -t 0 ] && echo IS-A-TTY; stty size; grep '^Max processes' /proc/self/limits; \
              echo DONE-$((40 + 2))\n"
                .to_vec(),
        ))
        .await
        .unwrap();
    let seen = read_until(&mut stream.output, "DONE-42").await;
    assert!(seen.contains("IS-A-TTY"), "{seen}");
    assert!(seen.contains("30 90"), "{seen}");
    let limits = seen
        .lines()
        .find(|line| line.starts_with("Max processes"))
        .unwrap();
    assert_eq!(
        limits.split_whitespace().nth(2),
        Some(target.pids.to_string().as_str()),
        "{limits}"
    );

    stream
        .input
        .send(CampusTerminalInput::Resize(
            CampusTerminalSize::new(132, 43).unwrap(),
        ))
        .await
        .unwrap();
    stream
        .input
        .send(CampusTerminalInput::Data(b"stty size\n".to_vec()))
        .await
        .unwrap();
    read_until(&mut stream.output, "43 132").await;

    let recorded = std::fs::read_to_string(&argv_file).unwrap();
    let recorded = recorded.lines().collect::<Vec<_>>();
    assert_eq!(
        recorded[..8],
        [
            "--context=campus-terminal",
            "exec",
            "--stdin",
            "--tty",
            "--namespace=campus-lab-slot-1",
            "--container=workspace",
            "campus-a-1",
            "--",
        ]
    );
    assert_eq!(recorded[8], "/bin/sh");
    assert_eq!(recorded[11], "campus-terminal");
    assert_eq!(recorded[12], "/bin/sh");
    let selectors = api.selectors.lock().unwrap().clone();
    assert_eq!(
        selectors,
        [(
            NAMESPACE.to_string(),
            "pharness.dev/campus-controller=pharness-campus-controller,\
             pharness.dev/campus-session=cls_0001,pharness.dev/campus-slot=campus-lab-slot-1,\
             pharness.dev/campus-generation=3,pharness.dev/campus-record=workload"
                .to_string()
        )]
    );

    // Dropping the stream ends the process.
    let pid = std::fs::read_to_string(format!("{}.pid", argv_file.display()))
        .unwrap()
        .trim()
        .to_string();
    drop(stream);
    let proc_path = format!("/proc/{pid}");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::path::Path::new(&proc_path).exists()
        && std::fs::read_to_string(format!("{proc_path}/stat"))
            .map(|stat| !stat.contains(") Z "))
            .unwrap_or(false)
    {
        assert!(
            std::time::Instant::now() < deadline,
            "terminal {pid} kept running"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn the_shell_exiting_closes_the_output() {
    let (_dir, script, _argv) = fake_kubectl();
    let api = Arc::new(FakePods {
        pods: vec![pod("campus-a-1", 3, true)],
        ..FakePods::default()
    });
    let terminal = KubectlCampusTerminal::new(config(script.to_str().unwrap()), api).unwrap();
    let target = CampusTerminalTarget {
        workload: workload(),
        pids: 64,
        size: CampusTerminalSize::DEFAULT,
    };
    let mut stream = terminal.attach(&target).await.unwrap();
    stream
        .input
        .send(CampusTerminalInput::Data(b"exit\n".to_vec()))
        .await
        .unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(10), async {
        while stream.output.recv().await.is_some() {}
    })
    .await;
    assert!(closed.is_ok(), "output closed after exit");
}

#[tokio::test]
async fn the_kubectl_terminal_refuses_unknown_slots_and_missing_pods() {
    let api = Arc::new(FakePods::default());
    let terminal = KubectlCampusTerminal::new(config("kubectl"), api.clone()).unwrap();
    let mut target = CampusTerminalTarget {
        workload: workload(),
        pids: 64,
        size: CampusTerminalSize::DEFAULT,
    };
    assert!(matches!(
        terminal.attach(&target).await,
        Err(CampusTerminalError::NotRunning(_))
    ));
    target.workload.slot_id = "kube-system".into();
    assert!(matches!(
        terminal.attach(&target).await,
        Err(CampusTerminalError::Refused(_))
    ));
    target.workload = workload();
    target.pids = 0;
    assert!(matches!(
        terminal.attach(&target).await,
        Err(CampusTerminalError::Refused(_))
    ));
    assert_eq!(api.selectors.lock().unwrap().len(), 1);
}

#[test]
fn terminal_sizes_are_bounded() {
    assert!(CampusTerminalSize::new(80, 24).is_some());
    assert!(CampusTerminalSize::new(500, 200).is_some());
    assert!(CampusTerminalSize::new(0, 24).is_none());
    assert!(CampusTerminalSize::new(80, 0).is_none());
    assert!(CampusTerminalSize::new(501, 24).is_none());
    assert!(CampusTerminalSize::new(80, 201).is_none());
}

#[tokio::test]
async fn the_fake_terminal_echoes_answers_and_counts_live_terminals() {
    let fake = FakeCampusTerminal::new();
    let target = CampusTerminalTarget {
        workload: workload(),
        pids: 128,
        size: CampusTerminalSize::DEFAULT,
    };
    let mut stream = fake.attach(&target).await.unwrap();
    assert_eq!(fake.live(), 1);
    read_until(&mut stream.output, "$ ").await;
    stream
        .input
        .send(CampusTerminalInput::Data(b"codex --version\r".to_vec()))
        .await
        .unwrap();
    read_until(&mut stream.output, FAKE_AGENT_CLI_VERSION).await;
    stream
        .input
        .send(CampusTerminalInput::Data(b"ulimit -u\r".to_vec()))
        .await
        .unwrap();
    read_until(&mut stream.output, "128").await;
    drop(stream);
    tokio::time::timeout(Duration::from_secs(2), async {
        while fake.live() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fake.attaches(), [target]);

    fake.refuse(Some(CampusTerminalError::Unavailable("down".into())));
    assert!(fake.attach(&workload_target()).await.is_err());
}

fn workload_target() -> CampusTerminalTarget {
    CampusTerminalTarget {
        workload: workload(),
        pids: 1,
        size: CampusTerminalSize::DEFAULT,
    }
}
