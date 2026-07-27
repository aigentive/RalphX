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
    retire_after_turn: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveProcessState {
    Active,
    Idle,
}

/// Result of completing a concrete interactive-process turn.
///
/// Only the entry's original token and launch run id may settle a turn. This keeps
/// delayed stream events from changing the lifecycle of a replacement registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveProcessTurnCompleteDisposition {
    Stale,
    KeepAlive,
    RetireAfterTurn,
}

/// Result of staging retirement for one concrete interactive-process registration.
///
/// The caller must commit an `IdleReady` retirement separately after its reversible
/// staging work succeeds. Keeping the idle entry registered until that point lets a
/// failed stage disarm it and resume normal writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveProcessRetireArmDisposition {
    Stale,
    AwaitingTurn,
    IdleReady,
}

/// Read-only retirement status for one exact interactive-process owner.
///
/// `Stale` covers a missing entry, a missing/blank run id, or a token/run-id
/// mismatch. The other variants expose the current state without arming or
/// otherwise changing the registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveProcessRetireAfterTurnDisposition {
    Stale,
    Active { is_armed: bool },
    Idle { is_armed: bool },
}

/// Monotonic identity for one concrete registry entry.
///
/// A stream-exit cleanup may outlive a persona-driven replacement under the same key;
/// the token makes that cleanup remove only the process it originally registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractiveProcessToken(u64);

/// Why a write through an interactive-process registration did not complete.
///
/// `StdinIo` carries the exact registration token that owned the handle when the
/// I/O operation failed. Callers may remove only that token, never whichever
/// process later happens to use the same context key.
#[derive(Debug, thiserror::Error)]
pub enum InteractiveProcessWriteError {
    #[error("no interactive process for {context_type}/{context_id}")]
    Missing {
        context_type: String,
        context_id: String,
    },
    #[error(
        "interactive process for {context_type}/{context_id} is retiring after the current turn"
    )]
    Retiring {
        context_type: String,
        context_id: String,
    },
    #[error(
        "failed to {operation} interactive process stdin for {context_type}/{context_id}: {source}"
    )]
    StdinIo {
        context_type: String,
        context_id: String,
        token: InteractiveProcessToken,
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InteractiveProcessMetadata {
    pub agent_run_id: Option<String>,
    pub harness: Option<AgentHarnessKind>,
    pub provider_session_id: Option<String>,
    pub persona_id: Option<String>,
    pub persona_content_hash: Option<String>,
}

/// Immutable identity and metadata captured from the current registry entry.
///
/// A snapshot is available only when the entry has a non-blank launch run id.
/// Callers must still pass both the token and run id to exact-owner operations,
/// because a later registration under the same key can replace this owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveProcessOwnerSnapshot {
    pub token: InteractiveProcessToken,
    pub agent_run_id: String,
    pub metadata: InteractiveProcessMetadata,
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
            retire_after_turn: false,
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
    /// Returns Ok(()) if the write succeeded, otherwise a typed outcome for the
    /// missing, retiring, or exact-token stdin I/O failure case.
    /// The Claude CLI reads stdin line-by-line in interactive mode, so messages
    /// should end with a newline (this method appends one if missing).
    pub async fn write_message(
        &self,
        key: &InteractiveProcessKey,
        message: &str,
    ) -> Result<(), InteractiveProcessWriteError> {
        let mut processes = self.processes.lock().await;
        let entry =
            processes
                .get_mut(key)
                .ok_or_else(|| InteractiveProcessWriteError::Missing {
                    context_type: key.context_type.clone(),
                    context_id: key.context_id.clone(),
                })?;
        if entry.retire_after_turn {
            return Err(InteractiveProcessWriteError::Retiring {
                context_type: key.context_type.clone(),
                context_id: key.context_id.clone(),
            });
        }
        // The registry lock serializes this ownership transition against idle retirement.
        entry.state = InteractiveProcessState::Active;

        // Ensure message ends with newline for CLI's line-based stdin reader
        let msg = if message.ends_with('\n') {
            message.to_string()
        } else {
            format!("{}\n", message)
        };

        let token = entry.token;
        entry
            .stdin
            .write_all(msg.as_bytes())
            .await
            .map_err(|source| InteractiveProcessWriteError::StdinIo {
                context_type: key.context_type.clone(),
                context_id: key.context_id.clone(),
                token,
                operation: "write to",
                source,
            })?;

        entry
            .stdin
            .flush()
            .await
            .map_err(|source| InteractiveProcessWriteError::StdinIo {
                context_type: key.context_type.clone(),
                context_id: key.context_id.clone(),
                token,
                operation: "flush",
                source,
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

    /// Stage retirement after the current turn for exactly one launch registration.
    ///
    /// An already idle entry remains registered but armed until the caller commits
    /// its retirement with `retire_armed_idle_if_owner`. A stale token or run id
    /// cannot arm a replacement.
    pub async fn arm_retire_after_turn_if_owner(
        &self,
        key: &InteractiveProcessKey,
        token: InteractiveProcessToken,
        agent_run_id: &str,
    ) -> InteractiveProcessRetireArmDisposition {
        let mut processes = self.processes.lock().await;
        let Some(entry) = processes.get_mut(key) else {
            return InteractiveProcessRetireArmDisposition::Stale;
        };
        if entry.token != token || entry.metadata.agent_run_id.as_deref() != Some(agent_run_id) {
            return InteractiveProcessRetireArmDisposition::Stale;
        }

        entry.retire_after_turn = true;
        if entry.state == InteractiveProcessState::Idle {
            InteractiveProcessRetireArmDisposition::IdleReady
        } else {
            InteractiveProcessRetireArmDisposition::AwaitingTurn
        }
    }

    /// Retire exactly one staged, idle registration after the caller commits its work.
    ///
    /// The returned entry exposes the exact launch metadata to the caller. A stale
    /// owner, active entry, or unarmed idle entry remains registered and returns None.
    pub async fn retire_armed_idle_if_owner(
        &self,
        key: &InteractiveProcessKey,
        token: InteractiveProcessToken,
        agent_run_id: &str,
    ) -> Option<InteractiveProcess> {
        let mut processes = self.processes.lock().await;
        let is_exact_armed_idle = match processes.get(key) {
            Some(entry)
                if entry.token == token
                    && entry.metadata.agent_run_id.as_deref() == Some(agent_run_id) =>
            {
                entry.retire_after_turn && entry.state == InteractiveProcessState::Idle
            }
            _ => false,
        };

        if is_exact_armed_idle {
            processes.remove(key)
        } else {
            None
        }
    }

    /// Cancel a staged exact-owner retirement while preserving the entry and its state.
    pub async fn disarm_retire_after_turn_if_owner(
        &self,
        key: &InteractiveProcessKey,
        token: InteractiveProcessToken,
        agent_run_id: &str,
    ) -> bool {
        let mut processes = self.processes.lock().await;
        let Some(entry) = processes.get_mut(key) else {
            return false;
        };
        if entry.token != token || entry.metadata.agent_run_id.as_deref() != Some(agent_run_id) {
            return false;
        }
        if !entry.retire_after_turn {
            return false;
        }
        entry.retire_after_turn = false;
        true
    }

    /// Settle a turn only for its exact registration and launch run owner.
    ///
    /// Retirement removes the entry atomically so the returned disposition is enough
    /// for the caller to choose terminal event handling without a second registry read.
    pub async fn complete_turn_if_owner(
        &self,
        key: &InteractiveProcessKey,
        token: InteractiveProcessToken,
        agent_run_id: &str,
    ) -> InteractiveProcessTurnCompleteDisposition {
        let mut processes = self.processes.lock().await;
        let retire_after_turn = match processes.get(key) {
            Some(entry)
                if entry.token == token
                    && entry.metadata.agent_run_id.as_deref() == Some(agent_run_id) =>
            {
                entry.retire_after_turn
            }
            _ => return InteractiveProcessTurnCompleteDisposition::Stale,
        };

        if retire_after_turn {
            processes.remove(key);
            InteractiveProcessTurnCompleteDisposition::RetireAfterTurn
        } else if let Some(entry) = processes.get_mut(key) {
            entry.state = InteractiveProcessState::Idle;
            InteractiveProcessTurnCompleteDisposition::KeepAlive
        } else {
            InteractiveProcessTurnCompleteDisposition::Stale
        }
    }

    /// Atomically retire only an unarmed idle registration so a concurrent stdin write wins.
    ///
    /// A staged owner retirement must use `retire_armed_idle_if_owner` after commit;
    /// this keeps reversible staging from being bypassed by generic idle cleanup.
    pub async fn retire_if_idle(&self, key: &InteractiveProcessKey) -> Option<InteractiveProcess> {
        let mut processes = self.processes.lock().await;
        if processes.get(key).is_some_and(|entry| {
            entry.state == InteractiveProcessState::Idle && !entry.retire_after_turn
        }) {
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

    /// Capture the current exact owner and metadata for a key.
    ///
    /// Entries without a usable run id fail closed so callers never guess an
    /// owner from a key alone. The returned snapshot is read-only and may become
    /// stale if a subsequent registration replaces the key.
    pub async fn capture_owner(
        &self,
        key: &InteractiveProcessKey,
    ) -> Option<InteractiveProcessOwnerSnapshot> {
        let processes = self.processes.lock().await;
        let entry = processes.get(key)?;
        let agent_run_id = entry.metadata.agent_run_id.as_deref()?;
        if agent_run_id.trim().is_empty() {
            return None;
        }

        Some(InteractiveProcessOwnerSnapshot {
            token: entry.token,
            agent_run_id: agent_run_id.to_owned(),
            metadata: entry.metadata.clone(),
        })
    }

    /// Read the retirement state only when the supplied token and run id still
    /// identify the current entry. This never arms, disarms, retires, or marks
    /// the process idle, making it safe for cancellation and watchdog checks.
    pub async fn retire_after_turn_disposition_if_owner(
        &self,
        key: &InteractiveProcessKey,
        token: InteractiveProcessToken,
        agent_run_id: &str,
    ) -> InteractiveProcessRetireAfterTurnDisposition {
        if agent_run_id.trim().is_empty() {
            return InteractiveProcessRetireAfterTurnDisposition::Stale;
        }

        let processes = self.processes.lock().await;
        let Some(entry) = processes.get(key) else {
            return InteractiveProcessRetireAfterTurnDisposition::Stale;
        };
        if entry.token != token || entry.metadata.agent_run_id.as_deref() != Some(agent_run_id) {
            return InteractiveProcessRetireAfterTurnDisposition::Stale;
        }

        match entry.state {
            InteractiveProcessState::Active => {
                InteractiveProcessRetireAfterTurnDisposition::Active {
                    is_armed: entry.retire_after_turn,
                }
            }
            InteractiveProcessState::Idle => InteractiveProcessRetireAfterTurnDisposition::Idle {
                is_armed: entry.retire_after_turn,
            },
        }
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
