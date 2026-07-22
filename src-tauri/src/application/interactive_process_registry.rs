// Interactive Process Registry
//
// Maps running interactive Claude CLI processes by (context_type, context_id) to their
// stdin handle. When a message arrives for a context with a running interactive process,
// the message is written directly to stdin instead of spawning a new process.
//
// The Claude CLI handles internal queuing: messages sent to stdin while the agent is
// mid-turn are queued and processed after the current turn completes.

use crate::domain::agents::AgentHarnessKind;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{Mutex, Notify};

/// Key for identifying an interactive process by context.
/// Reuses the same (context_type, context_id) pattern as RunningAgentKey.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct InteractiveProcessKey {
    pub context_type: String,
    pub context_id: String,
}

impl InteractiveProcessKey {
    pub fn new(context_type: impl Into<String>, context_id: impl Into<String>) -> Self {
        Self {
            context_type: context_type.into(),
            context_id: context_id.into(),
        }
    }
}

/// Wrapper around an interactive CLI process's stdin handle and its completion signal.
///
/// The `completion_signal` notifier allows waiters to be unblocked when the process
/// has finished (i.e., after `run_completed` should fire).
#[derive(Debug)]
pub struct InteractiveProcess {
    pub stdin: ChildStdin,
    pub completion_signal: Arc<Notify>,
    pub metadata: InteractiveProcessMetadata,
    token: InteractiveProcessToken,
    state: InteractiveProcessState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveProcessState {
    Active,
    Idle,
}

/// Monotonic identity for one concrete registry entry.
///
/// A stream-exit cleanup may outlive a persona-driven replacement under the same key;
/// the token makes that cleanup remove only the process it originally registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractiveProcessToken(u64);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InteractiveProcessMetadata {
    pub agent_run_id: Option<String>,
    pub harness: Option<AgentHarnessKind>,
    pub provider_session_id: Option<String>,
    pub persona_id: Option<String>,
    pub persona_content_hash: Option<String>,
}

/// Registry for interactive CLI processes with open stdin handles.
///
/// Thread-safe: uses tokio::sync::Mutex for async-compatible locking.
/// ChildStdin is not Clone, so the registry owns it exclusively.
#[derive(Debug)]
pub struct InteractiveProcessRegistry {
    processes: Mutex<HashMap<InteractiveProcessKey, InteractiveProcess>>,
    next_token: AtomicU64,
}

impl Default for InteractiveProcessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveProcessRegistry {
    pub fn new() -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
        }
    }

    /// Register a stdin handle for an interactive process.
    ///
    /// Wraps the stdin in an `InteractiveProcess` with a fresh `Arc<Notify>` completion signal.
    /// Returns the completion signal so callers can await it without holding the registry lock.
    /// If a process already exists for this key, the old one is dropped (closes the pipe).
    pub async fn register(&self, key: InteractiveProcessKey, stdin: ChildStdin) -> Arc<Notify> {
        self.register_entry(key, stdin, InteractiveProcessMetadata::default())
            .await
            .0
    }

    /// Register a stdin handle plus optional provider metadata for an interactive process.
    pub async fn register_with_metadata(
        &self,
        key: InteractiveProcessKey,
        stdin: ChildStdin,
        metadata: InteractiveProcessMetadata,
    ) -> InteractiveProcessToken {
        self.register_entry(key, stdin, metadata).await.1
    }

    async fn register_entry(
        &self,
        key: InteractiveProcessKey,
        stdin: ChildStdin,
        metadata: InteractiveProcessMetadata,
    ) -> (Arc<Notify>, InteractiveProcessToken) {
        let mut processes = self.processes.lock().await;
        if processes.contains_key(&key) {
            tracing::warn!(
                context_type = %key.context_type,
                context_id = %key.context_id,
                "InteractiveProcessRegistry: replacing existing stdin for context"
            );
        }
        let completion_signal = Arc::new(Notify::new());
        let token = InteractiveProcessToken(self.next_token.fetch_add(1, Ordering::Relaxed));
        let entry = InteractiveProcess {
            stdin,
            completion_signal: Arc::clone(&completion_signal),
            metadata,
            token,
            state: InteractiveProcessState::Active,
        };
        processes.insert(key, entry);
        (completion_signal, token)
    }

    /// Check if an interactive process exists for this context.
    pub async fn has_process(&self, key: &InteractiveProcessKey) -> bool {
        let processes = self.processes.lock().await;
        processes.contains_key(key)
    }

    /// Write a message to the stdin of a running interactive process.
    ///
    /// Returns Ok(()) if the write succeeded, Err if no process found or write failed.
    /// The Claude CLI reads stdin line-by-line in interactive mode, so messages
    /// should end with a newline (this method appends one if missing).
    pub async fn write_message(
        &self,
        key: &InteractiveProcessKey,
        message: &str,
    ) -> Result<(), String> {
        let mut processes = self.processes.lock().await;
        let entry = processes.get_mut(key).ok_or_else(|| {
            format!(
                "No interactive process for {}/{}",
                key.context_type, key.context_id
            )
        })?;
        // The registry lock serializes this ownership transition against idle retirement.
        entry.state = InteractiveProcessState::Active;

        // Ensure message ends with newline for CLI's line-based stdin reader
        let msg = if message.ends_with('\n') {
            message.to_string()
        } else {
            format!("{}\n", message)
        };

        entry.stdin.write_all(msg.as_bytes()).await.map_err(|e| {
            format!(
                "Failed to write to interactive process stdin for {}/{}: {}",
                key.context_type, key.context_id, e
            )
        })?;

        entry.stdin.flush().await.map_err(|e| {
            format!(
                "Failed to flush interactive process stdin for {}/{}: {}",
                key.context_type, key.context_id, e
            )
        })
    }

    /// Remove and return the InteractiveProcess for a context (e.g., on process exit).
    ///
    /// Dropping the returned InteractiveProcess (and its ChildStdin) closes the pipe,
    /// signaling EOF to the process.
    pub async fn remove(&self, key: &InteractiveProcessKey) -> Option<InteractiveProcess> {
        let mut processes = self.processes.lock().await;
        processes.remove(key)
    }

    /// Remove an entry only when it is still the same registration.
    ///
    /// Stream-exit cleanup uses this instead of keyed removal so an old process cannot
    /// erase a newer replacement that reused the same context key.
    pub async fn remove_if_token(
        &self,
        key: &InteractiveProcessKey,
        token: InteractiveProcessToken,
    ) -> Option<InteractiveProcess> {
        let mut processes = self.processes.lock().await;
        if processes.get(key).is_some_and(|entry| entry.token == token) {
            processes.remove(key)
        } else {
            None
        }
    }

    /// Mark only the concrete stream registration that emitted TurnComplete as idle.
    pub async fn mark_idle_if_token(
        &self,
        key: &InteractiveProcessKey,
        token: InteractiveProcessToken,
    ) -> bool {
        let mut processes = self.processes.lock().await;
        let Some(entry) = processes.get_mut(key) else {
            return false;
        };
        if entry.token != token {
            return false;
        }
        entry.state = InteractiveProcessState::Idle;
        true
    }

    /// Atomically retire only an idle registration so a concurrent stdin write wins.
    pub async fn retire_if_idle(&self, key: &InteractiveProcessKey) -> Option<InteractiveProcess> {
        let mut processes = self.processes.lock().await;
        if processes
            .get(key)
            .is_some_and(|entry| entry.state == InteractiveProcessState::Idle)
        {
            processes.remove(key)
        } else {
            None
        }
    }

    /// Return the completion signal for a running process, or None if not registered.
    ///
    /// Callers can clone and `.await` the returned notifier to be woken when the process
    /// signals completion. The Arc keeps the Notify alive even after the process is removed.
    pub async fn get_completion_signal(&self, key: &InteractiveProcessKey) -> Option<Arc<Notify>> {
        let processes = self.processes.lock().await;
        processes
            .get(key)
            .map(|entry| Arc::clone(&entry.completion_signal))
    }

    /// Return cloned provider metadata for a running process, if present.
    pub async fn get_metadata(
        &self,
        key: &InteractiveProcessKey,
    ) -> Option<InteractiveProcessMetadata> {
        let processes = self.processes.lock().await;
        processes.get(key).map(|entry| entry.metadata.clone())
    }

    /// Remove all registered processes.
    pub async fn clear(&self) {
        let mut processes = self.processes.lock().await;
        processes.clear();
    }

    /// Get the count of registered interactive processes.
    #[cfg(test)]
    pub async fn count(&self) -> usize {
        let processes = self.processes.lock().await;
        processes.len()
    }

    #[cfg(test)]
    pub async fn state_for_test(
        &self,
        key: &InteractiveProcessKey,
    ) -> Option<InteractiveProcessState> {
        let processes = self.processes.lock().await;
        processes.get(key).map(|entry| entry.state)
    }

    /// Return all registered process keys for shutdown diagnostics.
    pub async fn dump_state(&self) -> Vec<InteractiveProcessKey> {
        let processes = self.processes.lock().await;
        processes.keys().cloned().collect()
    }

    /// Log all registered process keys at info level for diagnostics.
    pub async fn log_registered_keys(&self, label: &str) {
        let processes = self.processes.lock().await;
        let keys: Vec<String> = processes
            .keys()
            .map(|k| format!("{}/{}", k.context_type, k.context_id))
            .collect();
        tracing::info!(
            label = %label,
            count = processes.len(),
            keys = ?keys,
            "[IPR_DIAG] Registered interactive processes"
        );
    }
}
