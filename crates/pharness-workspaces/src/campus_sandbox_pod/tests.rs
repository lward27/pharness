use super::*;
use crate::campus_broker_token::CampusBrokerSecret;

const BROKER: &str = "http://pharness-campus-broker.pharness.svc:4780/";

fn workload() -> CampusWorkloadRef {
    CampusWorkloadRef {
        session_id: "cls_alice".into(),
        slot_id: "tenant-a-lab-slot-01".into(),
        slot_generation: 3,
    }
}

fn binding() -> CampusSandboxBrokerBinding {
    CampusSandboxBrokerBinding::new(&Url::parse(BROKER).unwrap(), &workload()).unwrap()
}

/// A spec built only from the binding's helpers: what a compliant provider
/// submits.
fn compliant() -> Value {
    let binding = binding();
    let mut env = binding.env();
    env.push(json!({ "name": "HOME", "value": "/workspace" }));
    json!({
        "automountServiceAccountToken": false,
        "enableServiceLinks": false,
        "restartPolicy": "Never",
        "securityContext": { "runAsNonRoot": true, "runAsUser": 1000 },
        "initContainers": [{
            "name": "seed-workspace",
            "image": "registry.example/campus/seed@sha256:abc",
            "env": [{ "name": "SEED_SOURCE", "value": "/seed" }],
            "securityContext": { "allowPrivilegeEscalation": false },
            "volumeMounts": [{ "name": "workspace", "mountPath": "/workspace" }],
        }],
        "containers": [{
            "name": "student",
            "image": "registry.example/campus/coding@sha256:def",
            "env": env,
            "securityContext": {
                "allowPrivilegeEscalation": false,
                "capabilities": { "drop": ["ALL"] },
            },
            "volumeMounts": [
                { "name": "workspace", "mountPath": "/workspace" },
                { "name": "lab-materials", "mountPath": "/lab", "readOnly": true },
                binding.volume_mount(),
            ],
        }],
        "volumes": [
            { "name": "workspace", "persistentVolumeClaim": { "claimName": "ws" } },
            { "name": "lab-materials", "configMap": { "name": "lab-1" } },
            { "name": "scratch", "emptyDir": {} },
            binding.volume(),
        ],
    })
}

fn rule_of(spec: &Value) -> CampusPodSpecRule {
    check_campus_sandbox_pod_spec(spec, &binding())
        .expect_err("the spec must be refused")
        .rule
}

fn container(spec: &mut Value) -> &mut Value {
    &mut spec["containers"][0]
}

#[test]
fn a_spec_built_from_the_binding_is_accepted() {
    check_campus_sandbox_pod_spec(&compliant(), &binding()).unwrap();
}

#[test]
fn credential_scraping_of_a_compliant_spec_finds_only_the_scoped_token() {
    let spec = compliant();
    let text = spec.to_string();
    for forbidden in [
        "PHARNESS_",
        "WORKER",
        "worker-token",
        "OPERATOR",
        "FIREWORKS",
        "API_KEY",
        "KUBECONFIG",
        "AWS_",
        "serviceaccount",
        "/var/run/secrets",
        "secretKeyRef",
        "configMapKeyRef",
        "envFrom",
        "projected",
        "hostPath",
        "serviceAccountToken",
    ] {
        assert!(!text.contains(forbidden), "{forbidden} in {text}");
    }
    // The only secret reachable from the Pod is the session's own token, and
    // the only variable the platform adds is the broker location.
    let secrets = spec["volumes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|volume| volume.pointer("/secret/secretName"))
        .collect::<Vec<_>>();
    assert_eq!(secrets, [&json!(binding().secret_name())]);
    let names = spec["containers"][0]["env"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, [CAMPUS_BROKER_URL_ENV, "HOME"]);
    assert_eq!(
        spec["containers"][0]["volumeMounts"][2],
        json!({ "name": CAMPUS_BROKER_TOKEN_VOLUME, "mountPath": CAMPUS_BROKER_TOKEN_DIR, "readOnly": true })
    );
}

#[test]
fn service_account_tokens_and_host_access_are_refused() {
    use CampusPodSpecRule as R;
    let mut missing = compliant();
    missing
        .as_object_mut()
        .unwrap()
        .remove("automountServiceAccountToken");
    assert_eq!(rule_of(&missing), R::ServiceAccountToken);
    let mut mounted = compliant();
    mounted["automountServiceAccountToken"] = json!(true);
    assert_eq!(rule_of(&mounted), R::ServiceAccountToken);

    for field in ["hostNetwork", "hostPID", "hostIPC"] {
        let mut host = compliant();
        host[field] = json!(true);
        assert_eq!(rule_of(&host), R::HostNamespace, "{field}");
    }
    let mut shared = compliant();
    shared["shareProcessNamespace"] = json!(true);
    assert_eq!(rule_of(&shared), R::ProcessNamespaceSharing);

    let mut projected = compliant();
    projected["volumes"].as_array_mut().unwrap().push(json!({
        "name": "kube-api-access",
        "projected": { "sources": [{ "serviceAccountToken": { "path": "token" } }] },
    }));
    assert_eq!(rule_of(&projected), R::VolumeSource);
    let mut host_path = compliant();
    host_path["volumes"].as_array_mut().unwrap().push(json!({
        "name": "host", "hostPath": { "path": "/var/lib/kubelet" },
    }));
    assert_eq!(rule_of(&host_path), R::VolumeSource);

    let mut reserved = compliant();
    container(&mut reserved)["volumeMounts"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "name": "scratch",
            "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount",
        }));
    assert_eq!(rule_of(&reserved), R::MountPath);
}

#[test]
fn worker_provider_and_cloud_credentials_cannot_reach_the_environment() {
    use CampusPodSpecRule as R;
    let worker_secret_ref = json!({
        "name": "PHARNESS_WORKER_TOKEN",
        "valueFrom": { "secretKeyRef": { "name": "pharness-worker-token", "key": "token" } },
    });
    let renamed_secret_ref = json!({
        "name": "HELPER",
        "valueFrom": { "secretKeyRef": { "name": "pharness-fireworks", "key": "api-key" } },
    });
    let field_ref = json!({
        "name": "POD_NAME",
        "valueFrom": { "fieldRef": { "fieldPath": "metadata.name" } },
    });
    for (entry, expected) in [
        (worker_secret_ref, R::EnvValueFrom),
        (renamed_secret_ref, R::EnvValueFrom),
        (field_ref, R::EnvValueFrom),
        (
            json!({ "name": "PHARNESS_WORKER_TOKEN", "value": "worker-secret" }),
            R::Environment,
        ),
        (
            json!({ "name": "PHARNESS_OPERATOR_TOKEN", "value": "x" }),
            R::Environment,
        ),
        (
            json!({ "name": "FIREWORKS_API_KEY", "value": "x" }),
            R::Environment,
        ),
        (
            json!({ "name": "AWS_ACCESS_KEY_ID", "value": "x" }),
            R::Environment,
        ),
        (
            json!({ "name": "KUBECONFIG", "value": "/root/.kube/config" }),
            R::Environment,
        ),
        (
            json!({ "name": "CAMPUS_BROKER_TOKEN", "value": "pcbt1_x" }),
            R::Environment,
        ),
    ] {
        let mut spec = compliant();
        container(&mut spec)["env"]
            .as_array_mut()
            .unwrap()
            .push(entry.clone());
        assert_eq!(rule_of(&spec), expected, "{entry}");
    }
    let mut env_from = compliant();
    container(&mut env_from)["envFrom"] =
        json!([{ "secretRef": { "name": "pharness-worker-token" } }]);
    assert_eq!(rule_of(&env_from), R::EnvFromSource);
    let mut config_from = compliant();
    container(&mut config_from)["envFrom"] = json!([{ "configMapRef": { "name": "pharness" } }]);
    assert_eq!(rule_of(&config_from), R::EnvFromSource);

    let mut init_secret = compliant();
    init_secret["initContainers"][0]["env"] = json!([{
        "name": "GIT_TOKEN",
        "valueFrom": { "secretKeyRef": { "name": "github", "key": "token" } },
    }]);
    assert_eq!(rule_of(&init_secret), R::EnvValueFrom);
}

#[test]
fn only_the_sessions_own_token_secret_may_be_mounted_and_only_read_only() {
    use CampusPodSpecRule as R;
    let mut worker_secret = compliant();
    worker_secret["volumes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "name": "worker", "secret": { "secretName": "pharness-worker-token" },
        }));
    assert_eq!(rule_of(&worker_secret), R::SecretVolume);

    let mut other_session = compliant();
    other_session["volumes"][3]["secret"]["secretName"] =
        json!(campus_broker_token_secret_name(&CampusWorkloadRef {
            slot_generation: 4,
            ..workload()
        }));
    assert_eq!(rule_of(&other_session), R::SecretVolume);

    let mut extra_key = compliant();
    extra_key["volumes"][3]["secret"]["items"] =
        json!([{ "key": "token", "path": "token" }, { "key": "ca", "path": "ca" }]);
    assert_eq!(rule_of(&extra_key), R::SecretVolume);

    let mut missing = compliant();
    missing["volumes"].as_array_mut().unwrap().pop();
    assert_eq!(rule_of(&missing), R::SecretVolume);

    let mut writable = compliant();
    container(&mut writable)["volumeMounts"][2]["readOnly"] = json!(false);
    assert_eq!(rule_of(&writable), R::TokenMount);
    let mut sub_path = compliant();
    container(&mut sub_path)["volumeMounts"][2]["subPath"] = json!("token");
    assert_eq!(rule_of(&sub_path), R::TokenMount);
    let mut elsewhere = compliant();
    container(&mut elsewhere)["volumeMounts"][2]["mountPath"] = json!("/workspace/.token");
    assert_eq!(rule_of(&elsewhere), R::TokenMount);
    let mut unmounted = compliant();
    container(&mut unmounted)["volumeMounts"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_eq!(rule_of(&unmounted), R::TokenMount);
    let mut init_mount = compliant();
    init_mount["initContainers"][0]["volumeMounts"]
        .as_array_mut()
        .unwrap()
        .push(binding().volume_mount());
    assert_eq!(rule_of(&init_mount), R::TokenMount);
    let mut undeclared = compliant();
    container(&mut undeclared)["volumeMounts"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "ghost", "mountPath": "/ghost" }));
    assert_eq!(rule_of(&undeclared), R::UnknownVolume);
}

#[test]
fn no_trusted_process_may_share_the_pod() {
    use CampusPodSpecRule as R;
    // A broker sidecar would share the network namespace and volumes with
    // the student container, so any second container is refused.
    let mut sidecar = compliant();
    sidecar["containers"].as_array_mut().unwrap().push(json!({
        "name": "inference-broker",
        "image": "registry.example/pharness/broker",
        "securityContext": { "allowPrivilegeEscalation": false },
    }));
    assert_eq!(rule_of(&sidecar), R::SingleContainer);
    let mut none = compliant();
    none["containers"] = json!([]);
    assert_eq!(rule_of(&none), R::SingleContainer);
    let mut debug = compliant();
    debug["ephemeralContainers"] = json!([{ "name": "debugger", "image": "busybox" }]);
    assert_eq!(rule_of(&debug), R::EphemeralContainers);

    for url in [
        "http://127.0.0.1:4780/",
        "http://localhost:4780/",
        "http://broker.localhost/",
        "http://[::1]:4780/",
        "http://0.0.0.0:4780/",
        "http://169.254.169.254/",
        "http://metadata.google.internal/",
        "http://[fe80::1]/",
        "http://[::ffff:127.0.0.1]/",
        "unix:/run/broker.sock",
        "http://user:pass@broker.pharness.svc/",
        "http://broker.pharness.svc/?token=x",
    ] {
        let error =
            CampusSandboxBrokerBinding::new(&Url::parse(url).unwrap(), &workload()).expect_err(url);
        assert_eq!(error.rule, R::BrokerUrl, "{url}");
    }

    let mut moved = compliant();
    container(&mut moved)["env"][0]["value"] = json!("http://127.0.0.1:4780/");
    assert_eq!(rule_of(&moved), R::BrokerUrl);
    let mut duplicated = compliant();
    container(&mut duplicated)["env"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": CAMPUS_BROKER_URL_ENV, "value": BROKER }));
    assert_eq!(rule_of(&duplicated), R::BrokerUrl);
    let mut absent = compliant();
    container(&mut absent)["env"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    assert_eq!(rule_of(&absent), R::BrokerUrl);
}

#[test]
fn privileged_execution_is_refused() {
    use CampusPodSpecRule as R;
    let mut privileged = compliant();
    container(&mut privileged)["securityContext"]["privileged"] = json!(true);
    assert_eq!(rule_of(&privileged), R::Privileged);
    let mut escalation = compliant();
    container(&mut escalation)["securityContext"]
        .as_object_mut()
        .unwrap()
        .remove("allowPrivilegeEscalation");
    assert_eq!(rule_of(&escalation), R::PrivilegeEscalation);
    let mut init_escalation = compliant();
    init_escalation["initContainers"][0]["securityContext"]["allowPrivilegeEscalation"] =
        json!(true);
    assert_eq!(rule_of(&init_escalation), R::PrivilegeEscalation);
    let mut capabilities = compliant();
    container(&mut capabilities)["securityContext"]["capabilities"]["add"] = json!(["SYS_PTRACE"]);
    assert_eq!(rule_of(&capabilities), R::AddedCapabilities);
    assert_eq!(rule_of(&json!([])), R::Shape);
}

#[test]
fn secret_names_are_dns_safe_and_fenced_by_occupancy() {
    let name = campus_broker_token_secret_name(&workload());
    assert_eq!(name, campus_broker_token_secret_name(&workload()));
    assert!(name.len() <= 63);
    assert!(name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'));
    let next = CampusWorkloadRef {
        slot_generation: 4,
        ..workload()
    };
    assert_ne!(name, campus_broker_token_secret_name(&next));
    let weird = CampusWorkloadRef {
        session_id: "Session/With Spaces".into(),
        slot_id: "UPPER_slot".into(),
        slot_generation: 1,
    };
    assert!(campus_broker_token_secret_name(&weird).starts_with("campus-broker-"));
}

#[test]
fn the_secret_manifest_carries_only_an_inference_token_for_its_workload() {
    let binding = binding();
    let token = |audience, workload| CampusIssuedBrokerToken {
        token_id: "cbt_1".into(),
        audience,
        workload,
        lab_generation: 1,
        issued_at: 100,
        expires_at: 1_000,
        secret: CampusBrokerSecret::from_random_bytes(&[3; 32]),
    };
    let labels = BTreeMap::from([(
        "pharness.dev/campus-session".to_string(),
        "cls_alice".to_string(),
    )]);
    let issued = token(CampusBrokerAudience::Inference, workload());
    let manifest = binding
        .secret_manifest("campus-slot-01", &labels, &issued)
        .unwrap();
    assert_eq!(manifest["metadata"]["name"], binding.secret_name());
    assert_eq!(manifest["stringData"]["token"], issued.secret.expose());
    assert_eq!(
        manifest["metadata"]["annotations"][CAMPUS_BROKER_TOKEN_ROTATE_AT_ANNOTATION],
        "550"
    );
    assert_eq!(manifest["stringData"].as_object().unwrap().len(), 1);

    let tools = token(CampusBrokerAudience::Tools, workload());
    assert_eq!(
        binding
            .secret_manifest("campus-slot-01", &labels, &tools)
            .unwrap_err()
            .rule,
        CampusPodSpecRule::SecretVolume
    );
    let stale = token(
        CampusBrokerAudience::Inference,
        CampusWorkloadRef {
            slot_generation: 2,
            ..workload()
        },
    );
    assert!(binding
        .secret_manifest("campus-slot-01", &labels, &stale)
        .is_err());
}

const CA: &str =
    "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUY2FtcHVz\n-----END CERTIFICATE-----";

fn egress_binding() -> CampusSandboxBrokerBinding {
    let egress =
        CampusSandboxEgress::new(&Url::parse("http://10.43.130.30:3128").unwrap(), CA).unwrap();
    binding().with_egress(egress)
}

fn with_binding(binding: &CampusSandboxBrokerBinding) -> Value {
    let mut spec = compliant();
    let mut env = binding.env();
    env.push(json!({ "name": "HOME", "value": "/workspace" }));
    spec["containers"][0]["env"] = Value::Array(env);
    let volumes = spec["volumes"].as_array_mut().unwrap();
    *volumes.last_mut().unwrap() = binding.volume();
    spec
}

#[test]
fn an_egress_session_gets_exactly_the_proxy_and_ca_variables() {
    let binding = egress_binding();
    let spec = with_binding(&binding);
    check_campus_sandbox_pod_spec(&spec, &binding).unwrap();
    let environment = binding.platform_environment();
    assert!(environment.contains(&(CAMPUS_EGRESS_PROXY_ENV, "http://10.43.130.30:3128".into())));
    assert!(environment.contains(&(
        CAMPUS_NO_PROXY_ENV,
        "pharness-campus-broker.pharness.svc".into()
    )));
    assert_eq!(
        binding.volume()["secret"]["items"],
        json!([
            { "key": "token", "path": "token" },
            { "key": "egress-ca.pem", "path": "egress-ca.pem" },
        ])
    );
    // The egress spec does not fit a session without egress, and the
    // other way round: the proxy variables and the CA key belong to the
    // binding, not to the spec.
    assert!(check_campus_sandbox_pod_spec(&spec, &super::tests::binding()).is_err());
    assert!(check_campus_sandbox_pod_spec(&compliant(), &binding).is_err());

    for (name, value) in [
        (CAMPUS_EGRESS_PROXY_ENV, "http://attacker.example:3128"),
        ("SSL_CERT_FILE", "/workspace/my-ca.pem"),
        (CAMPUS_NO_PROXY_ENV, "*"),
    ] {
        let mut spec = with_binding(&binding);
        for entry in spec["containers"][0]["env"].as_array_mut().unwrap() {
            if entry["name"] == name {
                entry["value"] = value.into();
            }
        }
        let error = check_campus_sandbox_pod_spec(&spec, &binding).unwrap_err();
        assert_eq!(error.rule, CampusPodSpecRule::EgressProxy, "{name}");
    }
    // Neither may an init container see the proxy.
    let mut spec = with_binding(&binding);
    spec["initContainers"][0]["env"] =
        json!([{ "name": "HTTPS_PROXY", "value": "http://10.43.130.30:3128" }]);
    assert_eq!(
        check_campus_sandbox_pod_spec(&spec, &binding)
            .unwrap_err()
            .rule,
        CampusPodSpecRule::EgressProxy
    );
}

#[test]
fn the_egress_ca_is_public_certificates_and_the_proxy_a_network_endpoint() {
    let proxy = Url::parse("http://10.43.130.30:3128").unwrap();
    assert!(CampusSandboxEgress::new(&proxy, CA).is_ok());
    for bad in [
        "",
        "-----BEGIN PRIVATE KEY-----\nMIIB\n-----END PRIVATE KEY-----",
        "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n-----BEGIN PRIVATE KEY-----\nMIIB\n-----END PRIVATE KEY-----",
        "-----BEGIN CERTIFICATE-----\nMI IB\n-----END CERTIFICATE-----",
    ] {
        assert!(CampusSandboxEgress::new(&proxy, bad).is_err(), "{bad:?}");
    }
    for url in [
        "http://127.0.0.1:3128",
        "http://[::1]:3128",
        "http://169.254.169.254:80",
        "http://10.43.130.30",
        "https://10.43.130.30:3128",
        "http://10.43.130.30:3128/path",
        "http://u:p@10.43.130.30:3128",
    ] {
        assert!(
            CampusSandboxEgress::new(&Url::parse(url).unwrap(), CA).is_err(),
            "{url}"
        );
    }
}

#[test]
fn the_egress_secret_carries_the_ca_next_to_the_token() {
    let binding = egress_binding();
    let token = CampusIssuedBrokerToken {
        token_id: "cbt-1".into(),
        audience: CampusBrokerAudience::Inference,
        workload: workload(),
        lab_generation: 1,
        issued_at: 100,
        expires_at: 1_000,
        secret: CampusBrokerSecret::from_random_bytes(&[7; 32]),
    };
    let manifest = binding
        .secret_manifest("slot", &BTreeMap::new(), &token)
        .unwrap();
    assert_eq!(manifest["stringData"]["egress-ca.pem"], format!("{CA}\n"));
    assert_eq!(
        manifest["stringData"].as_object().unwrap().len(),
        2,
        "token and CA only"
    );
}
