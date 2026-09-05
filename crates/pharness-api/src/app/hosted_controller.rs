mod approval;
mod control;
mod operations;
mod progression;
mod scheduler;
mod state;

#[cfg(test)]
pub(in crate::app) use scheduler::reconcile_once;
pub(in crate::app) use scheduler::spawn;

pub(in crate::app) use control::{control_actions, execute_control, public_state};

// The deployment has one SQLite writer. Serialize operator controls with the
// scheduler's short dispatch boundaries; persisted fences arbitrate restarts.
static DISPATCH_BOUNDARY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
