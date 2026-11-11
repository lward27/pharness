//! Browser terminal access to a Campus lab session's workload.
//!
//! The Campus API authorizes every terminal connection against the lab
//! journal and hands an attacher a [`CampusTerminalTarget`] built only from
//! trusted state: the fenced workload the journal records for the session
//! and the process limit of the session's effective policy. Nothing a
//! browser sends selects a Pod, namespace, container or command. An attacher
//! starts the one fixed terminal command inside that workload and returns a
//! byte stream; dropping the stream ends the terminal.
//!
//! The session's PID 1 sets RLIMIT_NPROC for itself and everything it
//! starts ([`crate::campus_lab_session::kubernetes::session_command`]). A
//! terminal started through `pods/exec` is not a descendant of PID 1, so
//! [`terminal_command`] sets the same limit again before the shell starts,
//! and does not start the shell when the limit cannot be set.

mod fake;
mod kubectl;

#[cfg(test)]
mod tests;

pub use fake::{FakeCampusTerminal, FAKE_AGENT_CLI_VERSION, FAKE_TERMINAL_PROMPT};
pub use kubectl::{
    kubectl_exec_args, select_session_pod, CampusTerminalConfigError, KubectlCampusTerminal,
    KubectlCampusTerminalConfig,
};

use crate::CampusWorkloadRef;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

pub const MAX_TERMINAL_COLS: u16 = 500;
pub const MAX_TERMINAL_ROWS: u16 = 200;
/// The largest chunk of output an attacher hands over at once.
pub const TERMINAL_OUTPUT_CHUNK_BYTES: usize = 16 * 1024;
/// `argv[0]` of the terminal wrapper, so the shell's parent is recognisable
/// in `ps` inside the sandbox.
pub const TERMINAL_ARGV0: &str = "campus-terminal";
/// Bounded so a slow browser pushes back on the workload instead of
/// buffering its output in the control plane.
pub(crate) const STREAM_CAPACITY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusTerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl CampusTerminalSize {
    pub const DEFAULT: Self = Self { cols: 80, rows: 24 };

    /// `None` outside 1..=500 columns and 1..=200 rows.
    pub fn new(cols: u16, rows: u16) -> Option<Self> {
        ((1..=MAX_TERMINAL_COLS).contains(&cols) && (1..=MAX_TERMINAL_ROWS).contains(&rows))
            .then_some(Self { cols, rows })
    }
}

/// What to attach to. Built by the Campus API from the journal and the
/// session's effective policy, never from a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusTerminalTarget {
    pub workload: CampusWorkloadRef,
    /// RLIMIT_NPROC of the session; the terminal runs under the same limit.
    pub pids: u32,
    pub size: CampusTerminalSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampusTerminalInput {
    Data(Vec<u8>),
    Resize(CampusTerminalSize),
}

/// A live terminal. `output` closes when the terminal process exits.
/// Dropping the stream stops the process.
pub struct CampusTerminalStream {
    pub input: mpsc::Sender<CampusTerminalInput>,
    pub output: mpsc::Receiver<Vec<u8>>,
    _stop: oneshot::Sender<()>,
}

impl CampusTerminalStream {
    /// `stop` resolves (with an error) when the stream is dropped.
    pub fn new(
        input: mpsc::Sender<CampusTerminalInput>,
        output: mpsc::Receiver<Vec<u8>>,
        stop: oneshot::Sender<()>,
    ) -> Self {
        Self {
            input,
            output,
            _stop: stop,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusTerminalError {
    /// The workload has no single running, ready session Pod right now.
    #[error("the session workload is not running: {0}")]
    NotRunning(String),
    /// The configuration does not cover the workload, or the cluster refused
    /// the attach. Retrying will not help.
    #[error("the terminal was refused: {0}")]
    Refused(String),
    /// The backend could not be reached; a later attempt may work.
    #[error("the terminal backend is unavailable: {0}")]
    Unavailable(String),
}

/// Starts the fixed terminal command inside one session workload.
#[async_trait]
pub trait CampusTerminalAttacher: Send + Sync {
    async fn attach(
        &self,
        target: &CampusTerminalTarget,
    ) -> Result<CampusTerminalStream, CampusTerminalError>;
}

/// The argv that starts `shell` under the session's process limit, with
/// core dumps off and `TERM` set for xterm.js. It mirrors
/// [`crate::campus_lab_session::kubernetes::session_command`] except that
/// the terminal keeps its standard streams. If neither `ulimit` spelling
/// works the chain stops before the `exec`, so no shell ever runs without
/// the limit.
pub fn terminal_command(pids: u32, shell: &[String]) -> Vec<String> {
    let mut argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        format!(
            "{{ ulimit -u {pids} || ulimit -p {pids}; }} 2>/dev/null && ulimit -c 0 && \
             export TERM=xterm-256color && exec \"$@\""
        ),
        TERMINAL_ARGV0.to_string(),
    ];
    argv.extend(shell.iter().cloned());
    argv
}
