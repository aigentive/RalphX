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
}

/// Monotonic identity for one concrete registry entry.
///
/// A stream-exit cleanup may outlive a persona-driven replacement under the same key;
/// the token makes that cleanup remove only the process it originally registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractiveProcessToken(u64);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InteractiveProcessMetadata {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_has_process() {
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("ideation", "session-123");
        assert!(!registry.has_process(&key).await);
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_remove_nonexistent_returns_none() {
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("ideation", "session-123");
        assert!(registry.remove(&key).await.is_none());
    }

    #[tokio::test]
    async fn stale_stream_exit_does_not_remove_fresh_ipr_entry() {
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("project", "replacement-race");
        let (stdin_a, _child_a) = create_test_stdin().await;
        let token_a = registry
            .register_with_metadata(key.clone(), stdin_a, InteractiveProcessMetadata::default())
            .await;
        let (stdin_b, _child_b) = create_test_stdin().await;
        let token_b = registry
            .register_with_metadata(key.clone(), stdin_b, InteractiveProcessMetadata::default())
            .await;

        assert!(
            registry.remove_if_token(&key, token_a).await.is_none(),
            "old stream cleanup must not remove the replacement entry"
        );
        assert!(registry.has_process(&key).await);
        assert!(
            registry.remove_if_token(&key, token_b).await.is_some(),
            "the current stream cleanup must still remove its own entry"
        );
    }

    #[tokio::test]
    async fn test_write_message_no_process_returns_error() {
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("ideation", "session-123");
        let result = registry.write_message(&key, "hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No interactive process"));
    }

    #[tokio::test]
    async fn test_register_returns_completion_signal() {
        let (stdin, _child) = create_test_stdin().await;
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("task", "task-789");

        let signal = registry.register(key.clone(), stdin).await;
        // Signal is live and shared with the entry
        let fetched = registry.get_completion_signal(&key).await.unwrap();
        assert!(Arc::ptr_eq(&signal, &fetched));
    }

    #[tokio::test]
    async fn test_register_with_metadata_persists_harness_metadata() {
        let (stdin, _child) = create_test_stdin().await;
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("ideation", "session-xyz");

        registry
            .register_with_metadata(
                key.clone(),
                stdin,
                InteractiveProcessMetadata {
                    harness: Some(AgentHarnessKind::Codex),
                    provider_session_id: Some("thread-123".to_string()),
                    persona_id: None,
                    persona_content_hash: None,
                },
            )
            .await;

        let metadata = registry.get_metadata(&key).await.unwrap();
        assert_eq!(metadata.harness, Some(AgentHarnessKind::Codex));
        assert_eq!(metadata.provider_session_id.as_deref(), Some("thread-123"));
    }

    #[tokio::test]
    async fn test_get_completion_signal_none_if_not_registered() {
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("ideation", "session-999");
        assert!(registry.get_completion_signal(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_completion_signal_survives_remove() {
        // The Arc<Notify> should remain usable after the process is removed,
        // so any awaiter that cloned it before removal can still be notified.
        let (stdin, _child) = create_test_stdin().await;
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("merge", "merge-1");

        let signal = registry.register(key.clone(), stdin).await;
        let _removed = registry.remove(&key).await;

        // Notifying after removal should not panic
        signal.notify_waiters();
        // Signal for key is gone from registry
        assert!(registry.get_completion_signal(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_register_and_write_message() {
        // Create a real pipe to test write
        let (stdin, _child) = create_test_stdin().await;
        let registry = InteractiveProcessRegistry::new();
        let key = InteractiveProcessKey::new("task", "task-456");

        registry.register(key.clone(), stdin).await;
        assert!(registry.has_process(&key).await);
        assert_eq!(registry.count().await, 1);

        // Write should succeed
        let result = registry.write_message(&key, "test message").await;
        assert!(result.is_ok());

        // Remove
        let removed = registry.remove(&key).await;
        assert!(removed.is_some());
        assert!(!registry.has_process(&key).await);
    }

    #[tokio::test]
    async fn test_dump_state_empty() {
        let registry = InteractiveProcessRegistry::new();
        let keys = registry.dump_state().await;
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn test_dump_state_returns_all_keys() {
        let (stdin1, _child1) = create_test_stdin().await;
        let (stdin2, _child2) = create_test_stdin().await;
        let registry = InteractiveProcessRegistry::new();

        let key1 = InteractiveProcessKey::new("ideation", "session-1");
        let key2 = InteractiveProcessKey::new("task_execution", "task-2");
        registry.register(key1.clone(), stdin1).await;
        registry.register(key2.clone(), stdin2).await;

        let keys = registry.dump_state().await;
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&key1));
        assert!(keys.contains(&key2));
    }

    #[tokio::test]
    async fn test_clear_removes_all() {
        let (stdin1, _child1) = create_test_stdin().await;
        let (stdin2, _child2) = create_test_stdin().await;
        let registry = InteractiveProcessRegistry::new();

        registry
            .register(InteractiveProcessKey::new("a", "1"), stdin1)
            .await;
        registry
            .register(InteractiveProcessKey::new("b", "2"), stdin2)
            .await;
        assert_eq!(registry.count().await, 2);

        registry.clear().await;
        assert_eq!(registry.count().await, 0);
    }

    /// Helper: create a real stdin pipe via `cat` subprocess for testing writes.
    async fn create_test_stdin() -> (ChildStdin, tokio::process::Child) {
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to spawn cat");
        let stdin = child.stdin.take().expect("no stdin");
        (stdin, child)
    }
}
