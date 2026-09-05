mod control;

pub(in crate::app) use control::{control_actions, execute_control, public_state};

// The deployment has one SQLite writer. Serialize operator controls with the
// scheduler's short dispatch boundaries; persisted fences arbitrate restarts.
static DISPATCH_BOUNDARY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
