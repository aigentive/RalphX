// Verification Child Process Registry
//
// Tracks PIDs of active verification child processes so they can be explicitly
// killed after terminal reconciliation (Fix A). This prevents idle verification
// child processes from lingering until the 600s ideation no-output timeout fires,
// which would cause false `agent:error` events.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::domain::services::kill_process;

/// Registry that stores the OS PID of each active verification child process.
///
/// Registration: at spawn time, if `session_purpose == SessionPurpose::Verification`.
/// Deregistration: after `reconcile_verification_on_child_complete` returns `Some(_)`
/// in `handle_stream_success`, via `remove_and_kill`.
pub(crate) struct VerificationChildProcessRegistry {
    pids: Mutex<HashMap<String, u32>>,
}

impl VerificationChildProcessRegistry {
    pub fn new() -> Self {
        Self {
            pids: Mutex::new(HashMap::new()),
        }
    }

    /// Register a verification child's PID keyed by its context/session ID.
    pub fn register(&self, context_id: &str, pid: u32) {
        let mut pids = self.pids.lock().unwrap();
        tracing::debug!(
            context_id,
            pid,
            "VerificationChildProcessRegistry: registered"
        );
        pids.insert(context_id.to_string(), pid);
    }

    /// Remove an entry only if it still points at the process being cleaned up.
    pub fn remove_if_pid(&self, context_id: &str, expected_pid: u32) -> bool {
        let mut pids = self.pids.lock().unwrap();
        if pids.get(context_id) == Some(&expected_pid) {
            pids.remove(context_id);
            true
        } else {
            false
        }
    }

    /// Remove the registry entry for `context_id` and send SIGTERM to the stored PID.
    ///
    /// If no entry is found (already removed, or was never registered), logs a debug
    /// message and returns without error — this is a safe no-op.
    pub fn remove_and_kill(&self, context_id: &str) {
        let pid = {
            let mut pids = self.pids.lock().unwrap();
            pids.remove(context_id)
        };
        match pid {
            Some(pid) => {
                tracing::info!(
                    context_id,
                    pid,
                    "VerificationChildProcessRegistry: sending SIGTERM to verification child"
                );
                kill_process(pid);
            }
            None => {
                tracing::debug!(
                    context_id,
                    "VerificationChildProcessRegistry: no PID entry found (already removed or never registered)"
                );
            }
        }
    }
}
