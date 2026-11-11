//! An in-memory API server for the provider tests: label selectors, uids,
//! owner-reference garbage collection, a Job controller stand-in and faults.

use super::kube_api::{CampusKubeApi, CampusKubeError, CampusKubeKind};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;

type Key = (CampusKubeKind, String, String);
type CreateHook = Box<dyn FnOnce(&mut FakeKubeState) + Send>;

#[derive(Default)]
pub struct FakeKubeState {
    pub objects: BTreeMap<Key, Value>,
    next_uid: u64,
    /// Every create attempt, in order, as `Kind/namespace/name`.
    pub creates: Vec<String>,
    pub replaces: Vec<String>,
    pub unreachable: bool,
    /// Refuse creates of this kind as admission would.
    pub reject_kind: Option<(CampusKubeKind, String)>,
    /// Deleted Pods stay terminating instead of disappearing.
    pub pods_hang_on_delete: bool,
    /// Deleted Jobs keep a finalizer instead of disappearing.
    pub jobs_hang_on_delete: bool,
    /// The next create of this kind fails as if the API server went away.
    pub fail_next_create: Option<CampusKubeKind>,
    /// Runs right after the next Job is stored.
    after_job_create: Option<CreateHook>,
}

impl FakeKubeState {
    fn uid(&mut self) -> String {
        self.next_uid += 1;
        format!("uid-{}", self.next_uid)
    }

    pub fn count(&self, kind: CampusKubeKind) -> usize {
        self.objects.keys().filter(|key| key.0 == kind).count()
    }

    pub fn of_kind(&self, kind: CampusKubeKind) -> Vec<Value> {
        self.objects
            .iter()
            .filter(|(key, _)| key.0 == kind)
            .map(|(_, object)| object.clone())
            .collect()
    }

    pub fn put(&mut self, mut object: Value) -> Value {
        let kind = CampusKubeKind::from_kind(object["kind"].as_str().unwrap_or_default())
            .expect("known kind");
        let namespace = object
            .pointer("/metadata/namespace")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let name = object["metadata"]["name"]
            .as_str()
            .expect("name")
            .to_string();
        if object.pointer("/metadata/uid").is_none() {
            object["metadata"]["uid"] = self.uid().into();
        }
        self.next_uid += 1;
        object["metadata"]["resourceVersion"] = self.next_uid.to_string().into();
        self.objects.insert((kind, namespace, name), object.clone());
        object
    }

    pub fn put_slot_network_policies(&mut self, namespace: &str) {
        self.put(json!({
            "apiVersion": "networking.k8s.io/v1",
            "kind": "NetworkPolicy",
            "metadata": { "name": "campus-lab-default-deny", "namespace": namespace },
            "spec": { "podSelector": {}, "policyTypes": ["Ingress", "Egress"] },
        }));
        for name in ["campus-lab-session-broker", "campus-lab-session-egress"] {
            self.put(json!({
                "apiVersion": "networking.k8s.io/v1",
                "kind": "NetworkPolicy",
                "metadata": { "name": name, "namespace": namespace },
                "spec": { "podSelector": { "matchLabels": { "a": "b" } }, "policyTypes": ["Egress"] },
            }));
        }
    }

    pub fn tombstone_now(&mut self, namespace: &str, name: &str) {
        let key = (
            CampusKubeKind::ConfigMap,
            namespace.to_string(),
            name.to_string(),
        );
        if let Some(mut anchor) = self.objects.get(&key).cloned() {
            self.remove_with_dependents(&key);
            anchor["data"]["state"] = "tombstone".into();
            anchor["metadata"]["labels"]["pharness.dev/campus-record"] = "tombstone".into();
            anchor["metadata"]
                .as_object_mut()
                .expect("metadata")
                .remove("uid");
            self.put(anchor);
        }
    }

    /// The Job controller: one Pod per Job that has none, carrying the Pod
    /// template's labels and annotations.
    pub fn start_pods(&mut self, ready: bool) {
        for job in self.of_kind(CampusKubeKind::Job) {
            if job.pointer("/metadata/deletionTimestamp").is_some() {
                continue;
            }
            let name = job["metadata"]["name"]
                .as_str()
                .expect("job name")
                .to_string();
            let namespace = job["metadata"]["namespace"]
                .as_str()
                .expect("ns")
                .to_string();
            let pod_name = format!("{name}-pod");
            let key = (CampusKubeKind::Pod, namespace.clone(), pod_name.clone());
            if self.objects.contains_key(&key) {
                continue;
            }
            let mut pod = json!({
                "apiVersion": "v1",
                "kind": "Pod",
                "metadata": {
                    "name": pod_name,
                    "namespace": namespace,
                    "labels": job["spec"]["template"]["metadata"]["labels"].clone(),
                    "annotations": job["spec"]["template"]["metadata"]["annotations"].clone(),
                    "ownerReferences": [{ "kind": "Job", "name": name, "uid": job["metadata"]["uid"].clone() }],
                },
                "spec": job["spec"]["template"]["spec"].clone(),
                "status": { "phase": "Pending" },
            });
            pod["spec"]["nodeName"] = "campus-node".into();
            if ready {
                pod["status"] = json!({
                    "phase": "Running",
                    "conditions": [{ "type": "Ready", "status": "True" }],
                });
            }
            self.put(pod);
        }
    }

    pub fn set_pod_status(&mut self, status: Value) {
        for object in self
            .objects
            .iter_mut()
            .filter(|(key, _)| key.0 == CampusKubeKind::Pod)
        {
            object.1["status"] = status.clone();
        }
    }

    pub fn set_job_status(&mut self, status: Value) {
        for object in self
            .objects
            .iter_mut()
            .filter(|(key, _)| key.0 == CampusKubeKind::Job)
        {
            object.1["status"] = status.clone();
        }
    }

    fn remove_with_dependents(&mut self, key: &Key) {
        let Some(object) = self.objects.remove(key) else {
            return;
        };
        let uid = object.pointer("/metadata/uid").cloned();
        let dependents = self
            .objects
            .iter()
            .filter(|(_, other)| {
                other
                    .pointer("/metadata/ownerReferences")
                    .and_then(Value::as_array)
                    .is_some_and(|owners| {
                        owners.iter().any(|owner| owner.get("uid") == uid.as_ref())
                    })
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for dependent in dependents {
            self.delete_key(&dependent);
        }
    }

    fn delete_key(&mut self, key: &Key) {
        let hang = match key.0 {
            CampusKubeKind::Pod => self.pods_hang_on_delete,
            CampusKubeKind::Job => self.jobs_hang_on_delete,
            _ => false,
        };
        if hang {
            if let Some(object) = self.objects.get_mut(key) {
                if object.pointer("/metadata/deletionTimestamp").is_none() {
                    object["metadata"]["deletionTimestamp"] = "2030-03-17T17:46:40Z".into();
                    if key.0 == CampusKubeKind::Job {
                        object["metadata"]["finalizers"] = json!(["example.com/hold"]);
                    }
                }
            }
            // Pods of a hanging Job are still deleted.
            if key.0 == CampusKubeKind::Job {
                let uid = self
                    .objects
                    .get(key)
                    .and_then(|job| job.pointer("/metadata/uid").cloned());
                let pods = self
                    .objects
                    .iter()
                    .filter(|(other, object)| {
                        other.0 == CampusKubeKind::Pod
                            && object.pointer("/metadata/ownerReferences/0/uid") == uid.as_ref()
                    })
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                for pod in pods {
                    self.delete_key(&pod);
                }
            }
            return;
        }
        self.remove_with_dependents(key);
    }
}

fn split_selector(selector: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for character in selector.chars() {
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

pub fn matches_selector(object: &Value, selector: &str) -> bool {
    let labels = object.pointer("/metadata/labels");
    let value_of = |key: &str| {
        labels
            .and_then(|labels| labels.get(key))
            .and_then(Value::as_str)
    };
    split_selector(selector).iter().all(|requirement| {
        if let Some((key, values)) = requirement.split_once(" in ") {
            let values = values.trim().trim_start_matches('(').trim_end_matches(')');
            let found = value_of(key.trim());
            values.split(',').any(|value| Some(value.trim()) == found)
        } else if let Some((key, value)) = requirement.split_once('=') {
            value_of(key.trim()) == Some(value.trim())
        } else {
            panic!("unsupported selector {requirement:?}")
        }
    })
}

#[derive(Default)]
pub struct FakeKube {
    pub state: Mutex<FakeKubeState>,
}

impl FakeKube {
    /// A cluster with the qualified RuntimeClass and every slot's admin-owned
    /// NetworkPolicies.
    pub fn with_runtime_class(handler: &str, pool: (&str, &str)) -> Self {
        let fake = Self::without_network_policies(handler, pool);
        for slot in 1..=5 {
            fake.state()
                .put_slot_network_policies(&format!("campus-lab-slot-{slot}"));
        }
        fake
    }

    pub fn without_network_policies(handler: &str, pool: (&str, &str)) -> Self {
        let fake = Self::default();
        fake.state().put(json!({
            "apiVersion": "node.k8s.io/v1",
            "kind": "RuntimeClass",
            "metadata": { "name": "gvisor" },
            "handler": handler,
            "overhead": { "podFixed": { "cpu": "50m", "memory": "128Mi" } },
            "scheduling": { "nodeSelector": { pool.0: pool.1 } },
        }));
        fake
    }

    pub fn state(&self) -> std::sync::MutexGuard<'_, FakeKubeState> {
        self.state.lock().expect("fake kube lock")
    }

    pub fn after_next_job_create(&self, hook: impl FnOnce(&mut FakeKubeState) + Send + 'static) {
        self.state().after_job_create = Some(Box::new(hook));
    }

    fn check(&self) -> Result<std::sync::MutexGuard<'_, FakeKubeState>, CampusKubeError> {
        let state = self.state();
        if state.unreachable {
            return Err(CampusKubeError::Unreachable("connection refused".into()));
        }
        Ok(state)
    }
}

#[async_trait]
impl CampusKubeApi for FakeKube {
    async fn get(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Value>, CampusKubeError> {
        let state = self.check()?;
        let namespace = if kind == CampusKubeKind::RuntimeClass {
            ""
        } else {
            namespace
        };
        Ok(state
            .objects
            .get(&(kind, namespace.to_string(), name.to_string()))
            .cloned())
    }

    async fn list(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        selector: &str,
    ) -> Result<Vec<Value>, CampusKubeError> {
        let state = self.check()?;
        Ok(state
            .objects
            .iter()
            .filter(|(key, object)| {
                key.0 == kind && key.1 == namespace && matches_selector(object, selector)
            })
            .map(|(_, object)| object.clone())
            .collect())
    }

    async fn create(&self, manifest: &Value) -> Result<Value, CampusKubeError> {
        let mut state = self.check()?;
        let kind = CampusKubeKind::from_kind(manifest["kind"].as_str().unwrap_or_default())
            .ok_or_else(|| CampusKubeError::Rejected("unknown kind".into()))?;
        let namespace = manifest["metadata"]["namespace"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let name = manifest["metadata"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        state.creates.push(format!("{kind:?}/{namespace}/{name}"));
        if state.fail_next_create == Some(kind) {
            state.fail_next_create = None;
            return Err(CampusKubeError::Unreachable("connection reset".into()));
        }
        if let Some((rejected, message)) = &state.reject_kind {
            if *rejected == kind {
                return Err(CampusKubeError::Rejected(message.clone()));
            }
        }
        // Like the slot ResourceQuota, which admission checks before the
        // name is known to be taken.
        let jobs_in_namespace = state
            .objects
            .keys()
            .filter(|key| key.0 == CampusKubeKind::Job && key.1 == namespace)
            .count();
        if kind == CampusKubeKind::Job && jobs_in_namespace >= 1 {
            return Err(CampusKubeError::Rejected(format!(
                "jobs.batch \"{name}\" is forbidden: exceeded quota: campus-lab-slot"
            )));
        }
        if state
            .objects
            .contains_key(&(kind, namespace.clone(), name.clone()))
        {
            return Err(CampusKubeError::AlreadyExists(name));
        }
        if let Some(owners) = manifest
            .pointer("/metadata/ownerReferences")
            .and_then(Value::as_array)
        {
            let owner_alive = owners.iter().all(|owner| {
                state
                    .objects
                    .values()
                    .any(|object| object.pointer("/metadata/uid") == owner.get("uid"))
            });
            if !owner_alive {
                // The garbage collector removes it right away; model that as
                // never having been stored.
                return Ok(manifest.clone());
            }
        }
        let stored = state.put(manifest.clone());
        if kind == CampusKubeKind::Job {
            if let Some(hook) = state.after_job_create.take() {
                hook(&mut state);
            }
        }
        Ok(stored)
    }

    async fn replace(&self, manifest: &Value) -> Result<Value, CampusKubeError> {
        let mut state = self.check()?;
        let kind = CampusKubeKind::from_kind(manifest["kind"].as_str().unwrap_or_default())
            .ok_or_else(|| CampusKubeError::Rejected("unknown kind".into()))?;
        let namespace = manifest["metadata"]["namespace"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let name = manifest["metadata"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        state.replaces.push(format!("{kind:?}/{namespace}/{name}"));
        let key = (kind, namespace, name.clone());
        let Some(stored) = state.objects.get(&key) else {
            return Err(CampusKubeError::Failed(format!("{name} not found")));
        };
        if stored.pointer("/metadata/resourceVersion")
            != manifest.pointer("/metadata/resourceVersion")
        {
            return Err(CampusKubeError::Failed(format!(
                "Operation cannot be fulfilled on {name}: the object has been modified (Conflict)"
            )));
        }
        let mut replacement = manifest.clone();
        replacement["metadata"]["uid"] = stored["metadata"]["uid"].clone();
        Ok(state.put(replacement))
    }

    async fn delete(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        name: &str,
    ) -> Result<(), CampusKubeError> {
        let mut state = self.check()?;
        state.delete_key(&(kind, namespace.to_string(), name.to_string()));
        Ok(())
    }
}
