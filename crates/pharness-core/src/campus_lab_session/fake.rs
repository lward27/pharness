//! A deterministic in-memory [`CampusExecutionProvider`] with fault
//! injection at every crash window of the journal driver.

use super::{
    CampusDeleteOutcome, CampusExecutionProvider, CampusObservation, CampusProviderError,
    CampusProvisionOutcome, CampusProvisionSpec, CampusWorkloadRef, OwnedCampusResource,
};
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use tokio::sync::{Notify, Semaphore};

/// Where a fault fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeFaultPoint {
    /// Before `provision` creates anything.
    BeforeProvision,
    /// After `provision` created the workspace, before it returns. A pause
    /// here that is never released is a crash between the side effect and the
    /// journal update.
    AfterProvision,
    /// Before `delete` removes anything.
    BeforeDelete,
    /// After `delete` removed the first of several resources.
    MidDelete,
    Observe,
    ListOwned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeFault {
    /// Wait at the fault point until [`FakeCampusProvider::release_pause`].
    /// Dropping the caller's future while it waits simulates a process crash
    /// at that point; releasing it simulates a slow, now-stale worker.
    Pause,
    /// `delete` reports `Unreachable`; other calls fail with
    /// [`CampusProviderError::Unreachable`].
    Unreachable,
    /// `delete` reports `Blocked`; other calls fail with an internal error.
    Blocked(String),
    /// `provision` rejects the spec (`BeforeProvision` only).
    Reject(String),
    /// `provision` reports `InProgress` (`AfterProvision`) or `delete`
    /// reports `InProgress` (`BeforeDelete`).
    InProgress,
}

#[derive(Default)]
struct FakeState {
    resources: BTreeMap<String, OwnedCampusResource>,
    provisions: BTreeMap<String, (CampusProvisionSpec, String)>,
    deleted: BTreeSet<CampusWorkloadRef>,
    faults: Vec<(FakeFaultPoint, FakeFault)>,
    next_id: u64,
    workspaces_created: u64,
    calls: Vec<String>,
}

/// Every provisioned workload gets two resources, a workspace and its network
/// policy, so that a delete can be interrupted half way.
pub struct FakeCampusProvider {
    state: Mutex<FakeState>,
    paused: Notify,
    release: Semaphore,
}

impl Default for FakeCampusProvider {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            paused: Notify::new(),
            release: Semaphore::new(0),
        }
    }
}

impl FakeCampusProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a one-shot fault. Faults for the same point fire in order.
    pub fn inject(&self, point: FakeFaultPoint, fault: FakeFault) {
        self.lock().faults.push((point, fault));
    }

    /// Resolve once a call is waiting at a [`FakeFault::Pause`].
    pub async fn wait_for_pause(&self) {
        self.paused.notified().await;
    }

    /// Let one paused call continue.
    pub fn release_pause(&self) {
        self.release.add_permits(1);
    }

    /// How many workspaces were ever created. Idempotent replays do not count.
    pub fn workspaces_created(&self) -> u64 {
        self.lock().workspaces_created
    }

    pub fn resources(&self) -> Vec<OwnedCampusResource> {
        self.lock().resources.values().cloned().collect()
    }

    pub fn resources_for(&self, workload: &CampusWorkloadRef) -> Vec<OwnedCampusResource> {
        self.lock()
            .resources
            .values()
            .filter(|resource| &resource.workload == workload)
            .cloned()
            .collect()
    }

    /// Workspace IDs currently present for a session, across generations.
    pub fn workspaces_for_session(&self, session_id: &str) -> Vec<String> {
        self.lock()
            .resources
            .values()
            .filter(|resource| resource.kind == "workspace")
            .filter(|resource| resource.workload.session_id == session_id)
            .map(|resource| resource.resource_id.clone())
            .collect()
    }

    /// Remove a resource out of band, e.g. a node that lost the workspace.
    pub fn lose_resource(&self, resource_id: &str) -> bool {
        self.lock().resources.remove(resource_id).is_some()
    }

    /// Every call in order, as `method:session_id:slot_generation`.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().expect("fake campus provider state")
    }

    fn take_fault(&self, point: FakeFaultPoint) -> Option<FakeFault> {
        let mut state = self.lock();
        let index = state
            .faults
            .iter()
            .position(|(fault_point, _)| *fault_point == point)?;
        Some(state.faults.remove(index).1)
    }

    async fn pause(&self) {
        self.paused.notify_one();
        self.release
            .acquire()
            .await
            .expect("fake provider semaphore is never closed")
            .forget();
    }

    fn record_call(&self, method: &str, workload: &CampusWorkloadRef) {
        self.lock().calls.push(format!(
            "{method}:{}:{}",
            workload.session_id, workload.slot_generation
        ));
    }
}

fn unexpected(fault: &FakeFault, point: FakeFaultPoint) -> CampusProviderError {
    CampusProviderError::Internal(format!("injected {fault:?} at {point:?}"))
}

#[async_trait]
impl CampusExecutionProvider for FakeCampusProvider {
    async fn provision(
        &self,
        op_key: &str,
        spec: &CampusProvisionSpec,
    ) -> Result<CampusProvisionOutcome, CampusProviderError> {
        self.record_call("provision", &spec.workload);
        match self.take_fault(FakeFaultPoint::BeforeProvision) {
            None => {}
            Some(FakeFault::Pause) => self.pause().await,
            Some(FakeFault::Unreachable) => {
                return Err(CampusProviderError::Unreachable(
                    "injected before provision".into(),
                ))
            }
            Some(FakeFault::Reject(reason)) => {
                return Ok(CampusProvisionOutcome::Rejected { reason })
            }
            Some(fault) => return Err(unexpected(&fault, FakeFaultPoint::BeforeProvision)),
        }
        let workspace_id = {
            let mut state = self.lock();
            if state.deleted.contains(&spec.workload) {
                return Ok(CampusProvisionOutcome::Rejected {
                    reason: "the workload was already deleted".into(),
                });
            }
            if let Some((existing, workspace_id)) = state.provisions.get(op_key) {
                if existing != spec {
                    return Ok(CampusProvisionOutcome::Rejected {
                        reason: "operation key was reused with a different spec".into(),
                    });
                }
                workspace_id.clone()
            } else {
                state.next_id += 1;
                let workspace_id = format!("fake-ws-{}", state.next_id);
                for (kind, resource_id) in [
                    ("workspace", workspace_id.clone()),
                    ("network_policy", format!("{workspace_id}-netpol")),
                ] {
                    state.resources.insert(
                        resource_id.clone(),
                        OwnedCampusResource {
                            resource_id,
                            kind: kind.into(),
                            workload: spec.workload.clone(),
                            op_key: op_key.into(),
                        },
                    );
                }
                state.workspaces_created += 1;
                state
                    .provisions
                    .insert(op_key.into(), (spec.clone(), workspace_id.clone()));
                workspace_id
            }
        };
        match self.take_fault(FakeFaultPoint::AfterProvision) {
            None => Ok(CampusProvisionOutcome::Ready { workspace_id }),
            Some(FakeFault::Pause) => {
                self.pause().await;
                Ok(CampusProvisionOutcome::Ready { workspace_id })
            }
            Some(FakeFault::Unreachable) => Err(CampusProviderError::Unreachable(
                "injected after provision".into(),
            )),
            Some(FakeFault::InProgress) => Ok(CampusProvisionOutcome::InProgress { workspace_id }),
            Some(fault) => Err(unexpected(&fault, FakeFaultPoint::AfterProvision)),
        }
    }

    async fn observe(
        &self,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusObservation, CampusProviderError> {
        self.record_call("observe", workload);
        match self.take_fault(FakeFaultPoint::Observe) {
            None => {}
            Some(FakeFault::Pause) => self.pause().await,
            Some(FakeFault::Unreachable) => {
                return Err(CampusProviderError::Unreachable("injected observe".into()))
            }
            Some(fault) => return Err(unexpected(&fault, FakeFaultPoint::Observe)),
        }
        let state = self.lock();
        Ok(state
            .resources
            .values()
            .find(|resource| resource.kind == "workspace" && &resource.workload == workload)
            .map_or(CampusObservation::Absent, |resource| {
                CampusObservation::Ready {
                    workspace_id: resource.resource_id.clone(),
                }
            }))
    }

    async fn delete(
        &self,
        _op_key: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusDeleteOutcome, CampusProviderError> {
        self.record_call("delete", workload);
        match self.take_fault(FakeFaultPoint::BeforeDelete) {
            None => {}
            Some(FakeFault::Pause) => self.pause().await,
            Some(FakeFault::Unreachable) => {
                return Ok(CampusDeleteOutcome::Unreachable {
                    reason: "injected before delete".into(),
                })
            }
            Some(FakeFault::Blocked(reason)) => return Ok(CampusDeleteOutcome::Blocked { reason }),
            Some(FakeFault::InProgress) => return Ok(CampusDeleteOutcome::InProgress),
            Some(fault) => return Err(unexpected(&fault, FakeFaultPoint::BeforeDelete)),
        }
        let owned: Vec<String> = {
            let mut state = self.lock();
            state.deleted.insert(workload.clone());
            state
                .resources
                .values()
                .filter(|resource| &resource.workload == workload)
                .map(|resource| resource.resource_id.clone())
                .collect()
        };
        for (index, resource_id) in owned.iter().enumerate() {
            self.lock().resources.remove(resource_id);
            if index == 0 && owned.len() > 1 {
                match self.take_fault(FakeFaultPoint::MidDelete) {
                    None => {}
                    Some(FakeFault::Pause) => self.pause().await,
                    Some(FakeFault::Unreachable) => {
                        return Ok(CampusDeleteOutcome::Unreachable {
                            reason: "injected mid delete".into(),
                        })
                    }
                    Some(FakeFault::Blocked(reason)) => {
                        return Ok(CampusDeleteOutcome::Blocked { reason })
                    }
                    Some(fault) => return Err(unexpected(&fault, FakeFaultPoint::MidDelete)),
                }
            }
        }
        Ok(CampusDeleteOutcome::Deleted)
    }

    async fn list_owned(&self) -> Result<Vec<OwnedCampusResource>, CampusProviderError> {
        match self.take_fault(FakeFaultPoint::ListOwned) {
            None => {}
            Some(FakeFault::Pause) => self.pause().await,
            Some(FakeFault::Unreachable) => {
                return Err(CampusProviderError::Unreachable(
                    "injected list owned".into(),
                ))
            }
            Some(fault) => return Err(unexpected(&fault, FakeFaultPoint::ListOwned)),
        }
        Ok(self.resources())
    }
}
