pub(in crate::app) mod approval;
pub(in crate::app) mod build;
mod control;
mod operations;
mod preparation;
mod progression;
mod scheduler;
mod source;
pub(in crate::app) mod source_merge;
mod state;

#[cfg(test)]
pub(in crate::app) use scheduler::reconcile_once;
pub(in crate::app) use scheduler::spawn;

pub(in crate::app) use control::{control_actions, execute_control, public_state};

// The deployment has one SQLite writer. Serialize operator controls with the
// scheduler's short dispatch boundaries; persisted fences arbitrate restarts.
static DISPATCH_BOUNDARY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
