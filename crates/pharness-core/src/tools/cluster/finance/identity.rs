use super::{kubernetes::Resources, FinanceDeploymentExpectation};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

type Check<T> = Result<T, &'static str>;

fn text<'a>(v: &'a Value, path: &str) -> Check<&'a str> {
    v.pointer(path)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or("missing_or_malformed_identity")
}

fn items(v: &Value) -> Check<&Vec<Value>> {
    if v.pointer("/metadata/continue")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
    {
        return Err("incomplete_kubernetes_list");
    }
    v["items"]
        .as_array()
        .filter(|a| a.len() <= 32)
        .ok_or("missing_or_oversized_kubernetes_list")
}

fn resource(v: &Value, kind: &str, namespace: &str, name: Option<&str>) -> Check<()> {
    if v["kind"] != kind
        || v["metadata"]["namespace"] != namespace
        || name.is_some_and(|name| v["metadata"]["name"] != name)
        || !v["metadata"]["deletionTimestamp"].is_null()
    {
        return Err("wrong_or_terminating_resource");
    }
    text(v, "/metadata/name")?;
    text(v, "/metadata/uid")?;
    text(v, "/metadata/resourceVersion")?;
    Ok(())
}

fn owner(v: &Value, kind: &str, uid: &str, name: &str) -> bool {
    v["metadata"]["ownerReferences"]
        .as_array()
        .is_some_and(|owners| {
            owners.iter().filter(|o| o["controller"] == true).count() == 1
                && owners.iter().any(|o| {
                    o["kind"] == kind
                        && o["uid"] == uid
                        && o["name"] == name
                        && o["controller"] == true
                })
        })
}

fn template(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(labels) = v["metadata"]["labels"].as_object_mut() {
        labels.remove("pod-template-hash");
    }
    v
}

fn references(v: &Value) -> Value {
    json!({"name":v["metadata"]["name"],"uid":v["metadata"]["uid"],"resource_version":v["metadata"]["resourceVersion"]})
}

pub(super) fn verify(e: &FinanceDeploymentExpectation, r: &Resources) -> Check<Value> {
    e.validate().map_err(|_| "invalid_expected_identity")?;
    let ns = e.namespace();
    let name = e.workload();
    let image = e.image_ref();
    let a = &r.argo;
    resource(a, "Application", "argocd", Some(e.argo_application()))?;
    let source = &a["spec"]["source"];
    let destination = &a["spec"]["destination"];
    if source["repoURL"] != crate::hosted_sdlc::staging::GITOPS_REPOSITORY
        || source["path"] != e.gitops_path()
        || !matches!(source["targetRevision"].as_str(), Some("HEAD" | "main"))
        || !a["spec"]["sources"].is_null()
        || destination["namespace"] != ns
        || destination["server"] != "https://kubernetes.default.svc"
        || a["status"]["sync"]["comparedTo"]["source"] != *source
        || a["status"]["sync"]["comparedTo"]["destination"] != *destination
    {
        return Err("argo_target_or_comparison_changed");
    }
    if a["status"]["sync"]["revision"] != e.gitops_commit_sha {
        return Err("argo_revision_not_expected");
    }
    if a["status"]["sync"]["status"] != "Synced"
        || a["status"]["health"]["status"] != "Healthy"
        || a["status"]["operationState"]["phase"] == "Running"
        || !a["status"]["resources"]
            .as_array()
            .is_some_and(|resources| {
                resources.iter().any(|v| {
                    v["kind"] == "Deployment"
                        && v["namespace"] == ns
                        && v["name"] == name
                        && v["status"] == "Synced"
                })
            })
    {
        return Err("argo_reconciliation_incomplete");
    }
    let d = &r.deployment;
    resource(d, "Deployment", ns, Some(name))?;
    let generation = d["metadata"]["generation"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or("deployment_generation_missing")?;
    let replicas = d["spec"]["replicas"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 32)
        .ok_or("deployment_replicas_missing_or_unsupported")?;
    if d["status"]["observedGeneration"].as_u64() != Some(generation)
        || [
            "replicas",
            "readyReplicas",
            "availableReplicas",
            "updatedReplicas",
        ]
        .iter()
        .any(|k| d["status"][k].as_u64() != Some(replicas))
        || d["status"]["unavailableReplicas"].as_u64().unwrap_or(0) != 0
        || d["spec"]["paused"] == true
    {
        return Err("deployment_rollout_incomplete");
    }
    let containers = d
        .pointer("/spec/template/spec/containers")
        .and_then(Value::as_array)
        .ok_or("deployment_containers_missing")?;
    if containers.len() != 1
        || containers[0]["name"] != name
        || containers[0]["image"] != image
        || d["spec"]["selector"]["matchLabels"] != json!({"app":name})
    {
        return Err("deployment_image_or_selector_changed");
    }
    let deployment_uid = text(d, "/metadata/uid")?;
    let revision = text(
        d,
        "/metadata/annotations/deployment.kubernetes.io~1revision",
    )?;
    let mut current_replicasets = BTreeMap::new();
    for rs in items(&r.replicasets)? {
        if !owner(rs, "Deployment", deployment_uid, name) {
            continue;
        }
        if rs["metadata"]["annotations"]["deployment.kubernetes.io/revision"] != revision {
            continue;
        }
        resource(rs, "ReplicaSet", ns, None)?;
        if template(&rs["spec"]["template"]) != template(&d["spec"]["template"]) {
            return Err("replicaset_template_changed");
        }
        current_replicasets.insert(text(rs, "/metadata/uid")?, text(rs, "/metadata/name")?);
    }
    if current_replicasets.len() != 1 {
        return Err("current_replicaset_unproven");
    }
    let mut pods = BTreeMap::new();
    for pod in items(&r.pods)? {
        if !current_replicasets
            .iter()
            .any(|(uid, rs)| owner(pod, "ReplicaSet", uid, rs))
        {
            // Retained Jobs can share an application label. They prove nothing
            // about the rollout; a ready service endpoint to one is rejected below.
            continue;
        }
        resource(pod, "Pod", ns, None)?;
        let statuses = pod["status"]["containerStatuses"]
            .as_array()
            .ok_or("pod_container_status_missing")?;
        let specs = pod["spec"]["containers"]
            .as_array()
            .ok_or("pod_container_spec_missing")?;
        if statuses.len() != 1
            || specs.len() != 1
            || specs[0]["name"] != name
            || specs[0]["image"] != image
            || statuses[0]["name"] != name
            || statuses[0]["imageID"]
                .as_str()
                .map(|s| s.strip_prefix("docker-pullable://").unwrap_or(s))
                != Some(image.as_str())
            || statuses[0]["ready"] != true
            || !statuses[0]["state"]["running"].is_object()
            || pod["status"]["phase"] != "Running"
            || !pod["status"]["conditions"].as_array().is_some_and(|c| {
                c.iter()
                    .any(|c| c["type"] == "Ready" && c["status"] == "True")
            })
        {
            return Err("pod_image_or_readiness_unproven");
        }
        let uid = text(pod, "/metadata/uid")?;
        let ip = text(pod, "/status/podIP")?;
        if ip.parse::<std::net::IpAddr>().is_err() {
            return Err("pod_ip_invalid");
        }
        let restarts = statuses[0]["restartCount"]
            .as_u64()
            .ok_or("pod_restart_count_missing")?;
        let mut summary = references(pod);
        summary["ip"] = json!(ip);
        summary["image_id"] = statuses[0]["imageID"].clone();
        summary["restart_count"] = json!(restarts);
        summary["started_at"] = json!(text(&statuses[0], "/state/running/startedAt")?);
        if pods.insert(uid, (ip, summary)).is_some() {
            return Err("duplicate_pod_identity");
        }
    }
    if pods.len() != replicas as usize {
        return Err("pod_count_does_not_match_deployment");
    }
    let service = &r.service;
    resource(service, "Service", ns, Some(name))?;
    if service["spec"]["selector"] != json!({"app":name}) {
        return Err("service_selector_changed");
    }
    if service["spec"]["type"] != "ClusterIP"
        || !service["spec"]["ports"].as_array().is_some_and(|ports| {
            ports.len() == 1
                && ports[0]["port"] == e.service_port()
                && ports[0]["targetPort"] == e.container_port()
                && ports[0]["protocol"] == "TCP"
        })
    {
        return Err("service_port_or_type_changed");
    }
    let service_uid = text(service, "/metadata/uid")?;
    let mut endpoint_pods = BTreeSet::new();
    let mut slices = Vec::new();
    let mut not_ready_endpoints = 0;
    for slice in items(&r.endpoints)? {
        resource(slice, "EndpointSlice", ns, None)?;
        if !owner(slice, "Service", service_uid, name)
            || slice["metadata"]["labels"]["kubernetes.io/service-name"] != name
        {
            return Err("endpoint_service_owner_changed");
        }
        if !slice["ports"].as_array().is_some_and(|ports| {
            ports.len() == 1
                && ports[0]["port"] == e.container_port()
                && ports[0]["protocol"] == "TCP"
        }) {
            return Err("endpoint_port_changed");
        }
        let endpoints = slice["endpoints"]
            .as_array()
            .filter(|a| a.len() <= 32)
            .ok_or("endpoints_missing_or_oversized")?;
        for endpoint in endpoints {
            match endpoint["conditions"]["ready"].as_bool() {
                Some(false) => {
                    not_ready_endpoints += 1;
                    continue;
                }
                Some(true) => {}
                None => return Err("endpoint_readiness_unknown"),
            }
            let target = &endpoint["targetRef"];
            let uid = target["uid"]
                .as_str()
                .ok_or("endpoint_pod_identity_missing")?;
            let (ip, pod) = pods.get(uid).ok_or("endpoint_targets_other_pod")?;
            if target["kind"] != "Pod"
                || target["namespace"] != ns
                || target["name"] != pod["name"]
                || endpoint["conditions"]["ready"] != true
                || endpoint["conditions"]["terminating"] == true
                || endpoint["addresses"] != json!([ip])
            {
                return Err("endpoint_identity_or_readiness_unproven");
            }
            if !endpoint_pods.insert(uid) {
                return Err("duplicate_endpoint_pod");
            }
        }
        slices.push(references(slice));
    }
    if endpoint_pods.len() != pods.len() {
        return Err("ready_endpoints_incomplete");
    }
    Ok(
        json!({"argo":references(a),"gitops_revision":e.gitops_commit_sha,"deployment":references(d),"generation":generation,"deployment_revision":revision,"template_hash":crate::canonical_json_sha256(&d["spec"]["template"]).map_err(|_|"template_hash_failed")?,"replicas":replicas,"image_ref":image,"pods":pods.values().map(|(_,v)|v).collect::<Vec<_>>(),"service":references(service),"endpoint_slices":slices,"not_ready_endpoints_excluded":not_ready_endpoints,"observation_is_atomic":false,"runtime_verification":"not_evaluated"}),
    )
}
