//! A deterministic in-process terminal for tests and loopback development.
//! It runs nothing: it echoes input and answers a few fixed commands.

use super::{
    CampusTerminalAttacher, CampusTerminalError, CampusTerminalInput, CampusTerminalSize,
    CampusTerminalStream, CampusTerminalTarget, STREAM_CAPACITY, TERMINAL_OUTPUT_CHUNK_BYTES,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

const MAX_FLOOD_CHUNKS: usize = 4096;
const MAX_LINE_BYTES: usize = 4096;
pub const FAKE_TERMINAL_PROMPT: &str = "student@campus-lab:~$ ";
/// What the fake answers to `codex --version`, standing in for the agent CLI
/// the runner images ship.
pub const FAKE_AGENT_CLI_VERSION: &str = "codex-cli 0.0.0-campus-fake";

#[derive(Default)]
struct FakeTerminalState {
    attaches: Vec<CampusTerminalTarget>,
    refuse: Option<CampusTerminalError>,
}

/// Records every attach and how many terminals are live.
#[derive(Default, Clone)]
pub struct FakeCampusTerminal {
    state: Arc<Mutex<FakeTerminalState>>,
    live: Arc<AtomicUsize>,
}

impl FakeCampusTerminal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every target attached so far, in order.
    pub fn attaches(&self) -> Vec<CampusTerminalTarget> {
        self.state
            .lock()
            .expect("fake terminal lock")
            .attaches
            .clone()
    }

    /// Terminals whose process is still running.
    pub fn live(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }

    /// Make the next attaches fail with `error`, or succeed again with `None`.
    pub fn refuse(&self, error: Option<CampusTerminalError>) {
        self.state.lock().expect("fake terminal lock").refuse = error;
    }
}

struct LiveGuard(Arc<AtomicUsize>);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl CampusTerminalAttacher for FakeCampusTerminal {
    async fn attach(
        &self,
        target: &CampusTerminalTarget,
    ) -> Result<CampusTerminalStream, CampusTerminalError> {
        {
            let mut state = self.state.lock().expect("fake terminal lock");
            if let Some(error) = state.refuse.clone() {
                return Err(error);
            }
            state.attaches.push(target.clone());
        }
        let (input_tx, input_rx) = mpsc::channel(STREAM_CAPACITY);
        let (output_tx, output_rx) = mpsc::channel(STREAM_CAPACITY);
        let (stop_tx, stop_rx) = oneshot::channel();
        self.live.fetch_add(1, Ordering::SeqCst);
        let guard = LiveGuard(self.live.clone());
        let target = target.clone();
        tokio::spawn(async move {
            let _guard = guard;
            tokio::select! {
                _ = run(target, input_rx, output_tx) => {}
                _ = stop_rx => {}
            }
        });
        Ok(CampusTerminalStream::new(input_tx, output_rx, stop_tx))
    }
}

async fn run(
    target: CampusTerminalTarget,
    mut input: mpsc::Receiver<CampusTerminalInput>,
    output: mpsc::Sender<Vec<u8>>,
) {
    let mut size = target.size;
    let banner =
        format!("PHarness Campus fake terminal (no workload is running)\r\n{FAKE_TERMINAL_PROMPT}");
    if output.send(banner.into_bytes()).await.is_err() {
        return;
    }
    let mut line = Vec::new();
    while let Some(message) = input.recv().await {
        let bytes = match message {
            CampusTerminalInput::Resize(next) => {
                size = next;
                continue;
            }
            CampusTerminalInput::Data(bytes) => bytes,
        };
        for byte in bytes {
            let reply = match byte {
                b'\r' | b'\n' => {
                    let command = String::from_utf8_lossy(&line).trim().to_string();
                    line.clear();
                    match answer(&command, &target, size) {
                        Answer::Exit => {
                            let _ = output.send(b"\r\nlogout\r\n".to_vec()).await;
                            return;
                        }
                        Answer::Flood(chunks) => {
                            if output.send(b"\r\n".to_vec()).await.is_err() {
                                return;
                            }
                            for _ in 0..chunks {
                                let chunk = vec![b'x'; TERMINAL_OUTPUT_CHUNK_BYTES];
                                if output.send(chunk).await.is_err() {
                                    return;
                                }
                            }
                            format!("\r\n{FAKE_TERMINAL_PROMPT}").into_bytes()
                        }
                        Answer::Text(text) => {
                            format!("\r\n{text}{FAKE_TERMINAL_PROMPT}").into_bytes()
                        }
                    }
                }
                0x03 => {
                    line.clear();
                    format!("^C\r\n{FAKE_TERMINAL_PROMPT}").into_bytes()
                }
                0x7f | 0x08 => {
                    if line.pop().is_some() {
                        b"\x08 \x08".to_vec()
                    } else {
                        Vec::new()
                    }
                }
                byte if (0x20..0x7f).contains(&byte) && line.len() < MAX_LINE_BYTES => {
                    line.push(byte);
                    vec![byte]
                }
                _ => Vec::new(),
            };
            if !reply.is_empty() && output.send(reply).await.is_err() {
                return;
            }
        }
    }
}

enum Answer {
    Exit,
    Flood(usize),
    Text(String),
}

fn answer(command: &str, target: &CampusTerminalTarget, size: CampusTerminalSize) -> Answer {
    let text = match command {
        "" => String::new(),
        "exit" | "logout" => return Answer::Exit,
        "codex --version" => format!("{FAKE_AGENT_CLI_VERSION}\r\n"),
        "ulimit -u" => format!("{}\r\n", target.pids),
        "stty size" => format!("{} {}\r\n", size.rows, size.cols),
        "whoami" => "pharness\r\n".to_string(),
        "pwd" => "/workspace\r\n".to_string(),
        other => match other.strip_prefix("flood ") {
            Some(count) => {
                return Answer::Flood(
                    count
                        .trim()
                        .parse::<usize>()
                        .unwrap_or(0)
                        .min(MAX_FLOOD_CHUNKS),
                )
            }
            None => format!("fake terminal: {other}: command not found\r\n"),
        },
    };
    Answer::Text(text)
}
