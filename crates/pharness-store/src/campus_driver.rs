//! One-step reconciler for Campus lab sessions.
//!
//! [`advance_campus_lab_session`] moves one session forward by at most one
//! provider side effect, entirely from journal state:
//!
//! 1. load the session;
//! 2. either begin an operation in the same transaction as the transition
//!    and its audit event (the operation is leased to the caller), or lease
//!    the session's existing pending operation;
//! 3. call the provider with the operation's idempotency key;
//! 4. record the result under the lease fence, or release the lease when the
//!    result is inconclusive.
//!
//! A crash at any point leaves either no intent (nothing happened) or a
//! pending operation whose lease expires, after which any worker repeats step
//! 3 with the same key. A worker whose lease expired, or whose session or
//! slot moved on, has its write rejected with [`CampusJournalError::Fenced`].
//!
//! Nothing here spawns background work; callers decide when to step.

use crate::{
    CampusJournalError, CampusLabTransition, SqliteStore, StoredCampusLabOperation,
    StoredCampusLabSession,
};
use pharness_core::{
    CampusDeleteOutcome, CampusExecutionProvider, CampusLabDesiredState, CampusLabObservedState,
    CampusLabOperationKind, CampusLabSessionState, CampusObservation, CampusProviderError,
    CampusProvisionOutcome, CampusProvisionSpec, CampusTeardownHook, CampusWorkloadRef,
};
use serde_json::json;
use std::fmt;
use std::sync::Arc;

/// Failure reason recorded when a launch is denied without central authority.
pub const CENTRAL_AUTHORITY_UNAVAILABLE: &str =
    "central authority is unavailable; new launches are denied";

#[derive(Clone, Copy)]
pub struct CampusDriverContext<'a> {
    /// Unique per worker process; a restarted process must use a new id.
    pub worker_id: &'a str,
    pub now: i64,
    pub lease_seconds: i64,
    /// Whether the central admission authority is reachable. Without it no
    /// new launch is admitted, but sessions that already hold a slot keep
    /// running until they expire.
    pub central_authority: bool,
    /// Run, in order, once the provider confirmed a workload is gone.
    pub teardown_hooks: &'a [Arc<dyn CampusTeardownHook>],
}

impl fmt::Debug for CampusDriverContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CampusDriverContext")
            .field("worker_id", &self.worker_id)
            .field("now", &self.now)
            .field("lease_seconds", &self.lease_seconds)
            .field("central_authority", &self.central_authority)
            .field(
                "teardown_hooks",
                &self
                    .teardown_hooks
                    .iter()
                    .map(|hook| hook.name())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampusStepOutcome {
    /// The journal moved to `state`.
    Advanced {
        state: CampusLabSessionState,
        generation: i64,
    },
    /// Nothing changed in the lifecycle; step again later.
    Waiting {
        state: CampusLabSessionState,
        reason: String,
    },
    /// Another worker holds the live lease on the pending operation.
    LeaseHeld {
        owner: String,
        expires_at: i64,
    },
    /// A ready session was observed; no transition was needed.
    Observed {
        observed: CampusLabObservedState,
    },
    /// Cleanup is blocked. Only a retry ([`retry_campus_lab_cleanup`])
    /// moves the session on; the reconciler schedules one with backoff.
    Quarantined {
        reason: String,
    },
    Terminal {
        state: CampusLabSessionState,
    },
}

/// Advance one session by one step. See the module documentation.
pub async fn advance_campus_lab_session(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    context: &CampusDriverContext<'_>,
    session_id: &str,
) -> Result<CampusStepOutcome, CampusJournalError> {
    use CampusLabSessionState as State;

    let mut session = store
        .get_campus_lab_session(session_id)
        .await?
        .ok_or_else(|| CampusJournalError::NotFound(session_id.into()))?;
    if session.state.is_terminal() {
        return Ok(CampusStepOutcome::Terminal {
            state: session.state,
        });
    }
    if context.now >= session.expires_at && session.desired_state == CampusLabDesiredState::Ready {
        session = store
            .request_campus_lab_session_stop(
                session_id,
                context.worker_id,
                "absolute expiry reached",
                context.now,
            )
            .await?;
    }
    let stopping = session.desired_state == CampusLabDesiredState::Terminated;
    match session.state {
        State::Requested => {
            let to = if stopping {
                State::Terminated
            } else {
                State::Authorizing
            };
            transition(store, transition_for(&session, to, context)).await
        }
        State::Authorizing => {
            if stopping {
                return transition(store, transition_for(&session, State::Terminated, context))
                    .await;
            }
            if !context.central_authority {
                let mut denied = transition_for(&session, State::Failed, context);
                denied.failure = Some(CENTRAL_AUTHORITY_UNAVAILABLE.into());
                return transition(store, denied).await;
            }
            let mut admit = transition_for(&session, State::Provisioning, context);
            admit.lease_seconds = context.lease_seconds;
            match store.transition_campus_lab_session(admit).await {
                Ok(outcome) => {
                    let operation = outcome.begun.ok_or_else(|| {
                        CampusJournalError::Conflict("provisioning began no operation".into())
                    })?;
                    provision(store, provider, context, &outcome.session, &operation).await
                }
                Err(CampusJournalError::NoFreeSlot(tenant)) => Ok(CampusStepOutcome::Waiting {
                    state: session.state,
                    reason: format!("no free lab slot for tenant {tenant}"),
                }),
                Err(error) => Err(error),
            }
        }
        State::Provisioning => {
            if stopping {
                let mut stop = transition_for(&session, State::Stopping, context);
                stop.reason = Some("stop requested during provisioning".into());
                return match store.transition_campus_lab_session(stop).await {
                    Err(CampusJournalError::LeaseHeld {
                        owner, expires_at, ..
                    }) => Ok(CampusStepOutcome::LeaseHeld { owner, expires_at }),
                    other => advanced(other),
                };
            }
            let operation = pending(store, &session, CampusLabOperationKind::Provision).await?;
            match lease(store, context, &operation).await? {
                Ok(leased) => provision(store, provider, context, &session, &leased).await,
                Err(held) => Ok(held),
            }
        }
        State::Ready => {
            if stopping {
                let mut stop = transition_for(&session, State::Stopping, context);
                stop.reason = Some("stop requested".into());
                return transition(store, stop).await;
            }
            observe(store, provider, context, &session).await
        }
        State::Stopping => {
            let mut clean = transition_for(&session, State::Cleaning, context);
            clean.lease_seconds = context.lease_seconds;
            let outcome = store.transition_campus_lab_session(clean).await?;
            let operation = outcome.begun.ok_or_else(|| {
                CampusJournalError::Conflict("cleaning began no operation".into())
            })?;
            delete(store, provider, context, &outcome.session, &operation).await
        }
        State::Cleaning => {
            let operation = pending(store, &session, CampusLabOperationKind::Delete).await?;
            match lease(store, context, &operation).await? {
                Ok(leased) => delete(store, provider, context, &session, &leased).await,
                Err(held) => Ok(held),
            }
        }
        State::CleanupBlocked => Ok(CampusStepOutcome::Quarantined {
            reason: session.blocked_reason.unwrap_or_default(),
        }),
        State::Terminated | State::Failed => Ok(CampusStepOutcome::Terminal {
            state: session.state,
        }),
    }
}

/// Retry a quarantined cleanup: `CleanupBlocked -> Cleaning` with a new
/// delete operation, then one delete attempt.
pub async fn retry_campus_lab_cleanup(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    context: &CampusDriverContext<'_>,
    session_id: &str,
) -> Result<CampusStepOutcome, CampusJournalError> {
    let session = store
        .get_campus_lab_session(session_id)
        .await?
        .ok_or_else(|| CampusJournalError::NotFound(session_id.into()))?;
    let mut retry = transition_for(&session, CampusLabSessionState::Cleaning, context);
    retry.lease_seconds = context.lease_seconds;
    retry.reason = Some("quarantined cleanup retried".into());
    let outcome = store.transition_campus_lab_session(retry).await?;
    let operation = outcome
        .begun
        .ok_or_else(|| CampusJournalError::Conflict("cleaning began no operation".into()))?;
    delete(store, provider, context, &outcome.session, &operation).await
}

fn transition_for(
    session: &StoredCampusLabSession,
    to: CampusLabSessionState,
    context: &CampusDriverContext<'_>,
) -> CampusLabTransition {
    CampusLabTransition::new(session, to, context.worker_id, context.now)
}

async fn transition(
    store: &SqliteStore,
    transition: CampusLabTransition,
) -> Result<CampusStepOutcome, CampusJournalError> {
    advanced(store.transition_campus_lab_session(transition).await)
}

fn advanced(
    outcome: Result<crate::CampusLabTransitionOutcome, CampusJournalError>,
) -> Result<CampusStepOutcome, CampusJournalError> {
    let outcome = outcome?;
    Ok(CampusStepOutcome::Advanced {
        state: outcome.session.state,
        generation: outcome.session.generation,
    })
}

async fn pending(
    store: &SqliteStore,
    session: &StoredCampusLabSession,
    kind: CampusLabOperationKind,
) -> Result<StoredCampusLabOperation, CampusJournalError> {
    store
        .pending_campus_lab_operation(&session.session_id)
        .await?
        .filter(|operation| operation.kind == kind)
        .ok_or_else(|| {
            CampusJournalError::Conflict(format!(
                "{} session {} has no pending {} operation",
                session.state,
                session.session_id,
                kind.as_str()
            ))
        })
}

async fn lease(
    store: &SqliteStore,
    context: &CampusDriverContext<'_>,
    operation: &StoredCampusLabOperation,
) -> Result<Result<StoredCampusLabOperation, CampusStepOutcome>, CampusJournalError> {
    match store
        .lease_campus_lab_operation(
            &operation.op_key,
            context.worker_id,
            context.now,
            context.lease_seconds,
        )
        .await
    {
        Ok(leased) => Ok(Ok(leased)),
        Err(CampusJournalError::LeaseHeld {
            owner, expires_at, ..
        }) => Ok(Err(CampusStepOutcome::LeaseHeld { owner, expires_at })),
        Err(error) => Err(error),
    }
}

async fn release(
    store: &SqliteStore,
    context: &CampusDriverContext<'_>,
    session: &StoredCampusLabSession,
    operation: &StoredCampusLabOperation,
    reason: String,
) -> Result<CampusStepOutcome, CampusJournalError> {
    if let Some(lease) = operation.lease() {
        store
            .release_campus_lab_operation_lease(&lease, Some(&reason), context.now)
            .await?;
    }
    Ok(CampusStepOutcome::Waiting {
        state: session.state,
        reason,
    })
}

async fn provision(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    context: &CampusDriverContext<'_>,
    session: &StoredCampusLabSession,
    operation: &StoredCampusLabOperation,
) -> Result<CampusStepOutcome, CampusJournalError> {
    let spec = CampusProvisionSpec {
        workload: operation.workload(),
        tenant_id: session.tenant_id.clone(),
        course_id: session.course_id.clone(),
        profile: session.profile(),
        expires_at: session.expires_at,
    };
    let outcome = provider.provision(&operation.op_key, &spec).await;
    let mut next = transition_for(session, CampusLabSessionState::Ready, context);
    next.lease = operation.lease();
    match outcome {
        Ok(CampusProvisionOutcome::Ready { workspace_id }) => {
            next.result = json!({ "workspace_id": workspace_id });
            next.workspace_id = Some(workspace_id);
            transition(store, next).await
        }
        Ok(CampusProvisionOutcome::Rejected { reason }) => {
            next.to = CampusLabSessionState::Stopping;
            next.result = json!({ "rejected": reason });
            next.failure = Some(format!("provision rejected: {reason}"));
            next.observed = Some(CampusLabObservedState::Failed);
            transition(store, next).await
        }
        Ok(CampusProvisionOutcome::InProgress { workspace_id }) => {
            release(
                store,
                context,
                session,
                operation,
                format!("workspace {workspace_id} is still provisioning"),
            )
            .await
        }
        Err(error) => release(store, context, session, operation, error.to_string()).await,
    }
}

async fn observe(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    context: &CampusDriverContext<'_>,
    session: &StoredCampusLabSession,
) -> Result<CampusStepOutcome, CampusJournalError> {
    let workload = session
        .workload()
        .ok_or_else(|| CampusJournalError::Conflict("a ready session holds no slot".into()))?;
    let observed = match provider.observe(&workload).await {
        Ok(CampusObservation::Ready { workspace_id })
            if Some(&workspace_id) == session.workspace_id.as_ref() =>
        {
            CampusLabObservedState::Ready
        }
        Ok(CampusObservation::Provisioning) => CampusLabObservedState::Provisioning,
        Ok(lost) => {
            let mut stop = transition_for(session, CampusLabSessionState::Stopping, context);
            stop.failure = Some(format!("workspace lost: {lost:?}"));
            stop.observed = Some(match lost {
                CampusObservation::Absent => CampusLabObservedState::Absent,
                _ => CampusLabObservedState::Failed,
            });
            return transition(store, stop).await;
        }
        // Already-admitted work continues until expiry while the provider
        // cannot be reached; only the observation is recorded.
        Err(CampusProviderError::Unreachable(_)) => CampusLabObservedState::Unreachable,
        Err(error) => {
            return Ok(CampusStepOutcome::Waiting {
                state: session.state,
                reason: error.to_string(),
            })
        }
    };
    store
        .record_campus_lab_observation(
            &session.session_id,
            session.generation,
            observed,
            context.now,
        )
        .await?;
    Ok(CampusStepOutcome::Observed { observed })
}

async fn delete(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    context: &CampusDriverContext<'_>,
    session: &StoredCampusLabSession,
    operation: &StoredCampusLabOperation,
) -> Result<CampusStepOutcome, CampusJournalError> {
    let workload = operation.workload();
    let blocked = match provider.delete(&operation.op_key, &workload).await {
        Ok(CampusDeleteOutcome::Deleted) => {
            match confirm_teardown(store, provider, context, session, operation, &workload).await? {
                Teardown::Done => {
                    let to = if session.failure_reason.is_some() {
                        CampusLabSessionState::Failed
                    } else {
                        CampusLabSessionState::Terminated
                    };
                    let mut done = transition_for(session, to, context);
                    done.lease = operation.lease();
                    done.result = json!({ "deleted": true, "remaining": 0 });
                    return transition(store, done).await;
                }
                Teardown::Retry(reason) => {
                    return release(store, context, session, operation, reason).await
                }
                Teardown::Blocked(reason) => reason,
            }
        }
        Ok(CampusDeleteOutcome::InProgress) => {
            return release(
                store,
                context,
                session,
                operation,
                "deletion in progress".into(),
            )
            .await
        }
        Ok(CampusDeleteOutcome::Blocked { reason }) => format!("cleanup blocked: {reason}"),
        Ok(CampusDeleteOutcome::Unreachable { reason }) => format!("unreachable: {reason}"),
        Err(CampusProviderError::Unreachable(reason)) => format!("unreachable: {reason}"),
        Err(error) => return release(store, context, session, operation, error.to_string()).await,
    };
    let mut quarantine = transition_for(session, CampusLabSessionState::CleanupBlocked, context);
    quarantine.lease = operation.lease();
    quarantine.result = json!({ "blocked": blocked });
    quarantine.reason = Some(blocked);
    transition(store, quarantine).await
}

enum Teardown {
    Done,
    Retry(String),
    Blocked(String),
}

/// After the provider reported the workload deleted: confirm it from the
/// owned-resource inventory, then run every teardown hook that has not yet
/// succeeded for the session.
async fn confirm_teardown(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    context: &CampusDriverContext<'_>,
    session: &StoredCampusLabSession,
    operation: &StoredCampusLabOperation,
    workload: &CampusWorkloadRef,
) -> Result<Teardown, CampusJournalError> {
    let owned = match provider.list_owned().await {
        Ok(owned) => owned,
        Err(error) => {
            return Ok(Teardown::Blocked(format!(
                "deletion could not be confirmed: {error}"
            )))
        }
    };
    let remaining = owned
        .iter()
        .filter(|resource| &resource.workload == workload)
        .count();
    if remaining > 0 {
        return Ok(Teardown::Retry(format!(
            "{remaining} owned resources remain after delete"
        )));
    }
    if context.teardown_hooks.is_empty() {
        return Ok(Teardown::Done);
    }
    let done = store
        .list_campus_teardown_hook_runs(&session.session_id)
        .await?;
    for hook in context.teardown_hooks {
        let name = hook.name();
        if done.iter().any(|run| run == name) {
            continue;
        }
        match hook.release(&operation.op_key, workload).await {
            Ok(()) => {
                store
                    .record_campus_teardown_hook_run(
                        &session.session_id,
                        name,
                        &operation.op_key,
                        workload,
                        context.now,
                    )
                    .await?;
            }
            Err(CampusProviderError::Unreachable(reason)) => {
                return Ok(Teardown::Blocked(format!(
                    "teardown hook {name} unreachable: {reason}"
                )))
            }
            Err(error) => {
                return Ok(Teardown::Retry(format!(
                    "teardown hook {name} failed: {error}"
                )))
            }
        }
    }
    Ok(Teardown::Done)
}

#[cfg(test)]
mod tests;
