// Recovery Queue
//
// Shared staggered recovery queue for agent restart after app crashes.
// Used by PDM-171 (ideation agent recovery) and PDM-172 (verification agent recovery).
//
// Architecture:
//   RecoveryQueue (submit side) — cloneable, sends RecoveryItems via mpsc channel
//   RecoveryQueueProcessor (consumer side) — processes items with priority sorting,
//     staggered delays, and concurrent recovery limits
//
// Spawn ordering in lib.rs (NON-NEGOTIABLE — Constraint 9):
//   1. Construct RecoveryQueue + RecoveryQueueProcessor
//   2. Spawn RecoveryQueueProcessor::run() as tokio task
//   3. Call startup_scan() (which submits items)

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::application::chat_service::{ChatService, SendMessageOptions};
use crate::application::interactive_process_registry::{InteractiveProcessKey, InteractiveProcessRegistry};
use crate::domain::entities::{ChatContextType, IdeationSessionId, IdeationSessionStatus, VerificationStatus};
use crate::domain::repositories::IdeationSessionRepository;
use crate::domain::services::{
    clear_verification_snapshot, emit_verification_status_changed,
    load_current_verification_snapshot_or_default, RunningAgentRegistry,
};

/// Configuration for the recovery queue processor.
#[derive(Debug, Clone)]
pub struct RecoveryQueueConfig {
    /// Delay between successive agent spawn attempts (prevents thundering herd).
    pub delay_between_spawns: Duration,
    /// Maximum number of recovery spawns running concurrently.
    pub max_concurrent_recoveries: usize,
    /// Timeout budget per recovery item.
    pub recovery_timeout: Duration,
    /// Max retries per item. 0 = no retries (one attempt total per Constraint 8).
    pub max_retries: usize,
}

impl Default for RecoveryQueueConfig {
    fn default() -> Self {
        Self {
            delay_between_spawns: Duration::from_secs(3),
            max_concurrent_recoveries: 2,
            recovery_timeout: Duration::from_secs(30),
            max_retries: 0,
        }
    }
}

/// Discriminates between recovery kinds for processing routing.
#[derive(Debug, Clone)]
pub enum RecoveryKind {
    /// PDM-171: orphaned ideation session agent.
    IdeationAgent,
    /// PDM-172: orphaned verification (ralphx-plan-verifier) agent in a child session.
    VerificationAgent,
}

/// Supplemental metadata for recovery context injection.
#[derive(Debug, Clone, Default)]
pub struct RecoveryMetadata {
    /// Current verification round from the parent's native verification summary.
    pub current_round: Option<u32>,
    /// Verification generation counter — must NOT be incremented during recovery (Constraint 2).
    pub verification_generation: Option<u32>,
    /// Conversation ID of the orphaned session (for --resume).
    pub conversation_id: Option<String>,
    /// Plan artifact ID (for VerificationAgent context injection).
    pub plan_artifact_id: Option<String>,
}

/// A single pending recovery request.
#[derive(Debug, Clone)]
pub struct RecoveryItem {
    /// Context type of the session to recover.
    pub context_type: ChatContextType,
    /// Context ID of the session to recover.
    /// For VerificationAgent: the child (verification) session ID.
    /// For IdeationAgent: the ideation session ID.
    pub context_id: String,
    /// Kind of recovery to perform.
    pub recovery_kind: RecoveryKind,
    /// Processing priority. Higher value = processed sooner.
    /// Convention: parent ideation agents > their verification children.
    pub priority: u8,
    /// For VerificationAgent: the parent ideation session ID.
    pub parent_session_id: Option<String>,
    /// Supplemental recovery metadata (round number, generation, etc.).
    pub metadata: RecoveryMetadata,
}

/// Submit side of the recovery queue.
///
/// Clone and share across callers (startup_scan, future recovery paths).
/// The channel closes when all `RecoveryQueue` clones are dropped, signaling
/// the processor to exit.
#[derive(Clone)]
pub struct RecoveryQueue {
    tx: Arc<mpsc::Sender<RecoveryItem>>,
    config: RecoveryQueueConfig,
}

impl RecoveryQueue {
    /// Submit a recovery item to the queue.
    ///
    /// Returns `Err` if the processor has exited (channel full or closed).
    pub fn submit(&self, item: RecoveryItem) -> Result<(), String> {
        self.tx
            .try_send(item)
            .map_err(|e| format!("RecoveryQueue: failed to submit item: {e}"))
    }

    /// Returns a reference to the queue configuration.
    pub fn config(&self) -> &RecoveryQueueConfig {
        &self.config
    }
}

/// Consumer side of the recovery queue.
///
/// Constructed alongside `RecoveryQueue` via `create_recovery_queue()`.
/// Must be spawned as a tokio task BEFORE `startup_scan()` is called (Constraint 9).
pub struct RecoveryQueueProcessor {
    rx: mpsc::Receiver<RecoveryItem>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    /// Cleared before send_message() to prevent Gate 1 from writing to a dead stdin handle
    /// (Constraint 10 — IPR entry must be removed before send_message() for recovery spawns).
    interactive_process_registry: Arc<InteractiveProcessRegistry>,
    /// For fallback state resets on failed re-spawns and for reading parent metadata.
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    /// For re-spawning verification agents via send_message().
    chat_service: Arc<dyn ChatService>,
    /// For emitting agent:session_recovered and agent:error events.
    app_handle: Option<tauri::AppHandle<tauri::Wry>>,
    config: RecoveryQueueConfig,
}

impl RecoveryQueueProcessor {
    /// Run the processor loop. Call this inside a `tokio::spawn`.
    ///
    /// Exits when all `RecoveryQueue` senders are dropped (channel closes).
    pub async fn run(mut self) {
        tracing::info!("RecoveryQueueProcessor: started");

        loop {
            // Block until an item arrives (or channel closes)
            let first = match self.rx.recv().await {
                Some(item) => item,
                None => {
                    tracing::info!("RecoveryQueueProcessor: channel closed, exiting");
                    return;
                }
            };

            // Drain all additional pending items for priority-sorted batch processing
            let mut batch = vec![first];
            while let Ok(item) = self.rx.try_recv() {
                batch.push(item);
            }

            // Sort by priority descending: higher priority items processed first.
            // Priority ordering: parent ideation agents > their verification children.
            batch.sort_by(|a, b| b.priority.cmp(&a.priority));

            tracing::info!(
                batch_size = batch.len(),
                "RecoveryQueueProcessor: processing batch"
            );

            for (idx, item) in batch.into_iter().enumerate() {
                // Apply staggered delay between spawns (Constraint 7 — no thundering herd)
                if idx > 0 {
                    tokio::time::sleep(self.config.delay_between_spawns).await;
                }
                self.process_item(&item).await;
            }
        }
    }

    /// Process a single recovery item.
    ///
    /// Performs common Gate 2 cleanup (cleanup_stale_entry) then dispatches to
    /// kind-specific recovery handlers.
    async fn process_item(&self, item: &RecoveryItem) {
        tracing::info!(
            context_type = ?item.context_type,
            context_id = %item.context_id,
            recovery_kind = ?item.recovery_kind,
            priority = item.priority,
            parent_session_id = ?item.parent_session_id,
            "RecoveryQueueProcessor: processing recovery item"
        );

        // Eagerly clean stale running_agents row (Gate 2) so try_register() doesn't find a
        // blocking placeholder (pid=0 or dead pid). cleanup_stale_entry() is safe: it only
        // removes rows where !is_process_alive(pid).
        use crate::domain::services::RunningAgentKey;
        let context_type_str = format!("{}", item.context_type);
        let key = RunningAgentKey::new(context_type_str, item.context_id.clone());
        match self.running_agent_registry.cleanup_stale_entry(&key).await {
            Ok(Some(info)) => {
                tracing::info!(
                    context_id = %item.context_id,
                    pid = info.pid,
                    "RecoveryQueueProcessor: cleaned stale running_agents row for dead process"
                );
            }
            Ok(None) => {
                tracing::debug!(
                    context_id = %item.context_id,
                    "RecoveryQueueProcessor: no stale entry found (process alive or unregistered)"
                );
            }
            Err(e) => {
                tracing::warn!(
                    context_id = %item.context_id,
                    error = %e,
                    "RecoveryQueueProcessor: cleanup_stale_entry failed"
                );
            }
        }

        // Dispatch to kind-specific recovery handler
        match item.recovery_kind {
            RecoveryKind::VerificationAgent => {
                self.process_verification_recovery(item).await;
            }
            RecoveryKind::IdeationAgent => {
                // PDM-171: ideation agent recovery — placeholder for future implementation
                tracing::info!(
                    context_id = %item.context_id,
                    "RecoveryQueueProcessor: IdeationAgent recovery not yet implemented (PDM-171)"
                );
            }
        }
    }

    /// Handle recovery for a `RecoveryKind::VerificationAgent` item.
    ///
    /// Flow:
    /// 1. Remove stale IPR entry (Gate 1 — prevents stdin-to-dead-pipe write)
    /// 2. Build recovery prompt with `<recovery_note>` tag
    /// 3. Call `chat_service.send_message()` to re-spawn the ralphx-plan-verifier agent
    /// 4. On success: emit `agent:session_recovered` for frontend notification
    /// 5. On failure: reset parent to Unverified, archive child, emit `agent:error`
    async fn process_verification_recovery(&self, item: &RecoveryItem) {
        let child_session_id = &item.context_id;
        let parent_session_id = match &item.parent_session_id {
            Some(id) => id,
            None => {
                tracing::warn!(
                    context_id = %child_session_id,
                    "RecoveryQueueProcessor: VerificationAgent recovery item missing parent_session_id — skipping"
                );
                return;
            }
        };

        tracing::info!(
            child_session_id = %child_session_id,
            parent_session_id = %parent_session_id,
            current_round = ?item.metadata.current_round,
            generation = ?item.metadata.verification_generation,
            "RecoveryQueueProcessor: processing VerificationAgent recovery"
        );

        // Step 1: Clean stale IPR entry (Gate 1 prevention — Constraint 10).
        // The running_agents row cleanup (Gate 2) was already done in process_item().
        let ipr_key = InteractiveProcessKey::new("ideation", child_session_id.as_str());
        self.interactive_process_registry.remove(&ipr_key).await;
        tracing::debug!(
            context_id = %child_session_id,
            "RecoveryQueueProcessor: cleared stale IPR entry for recovery"
        );

        // Step 2: Build recovery prompt with <recovery_note> tag.
        // Includes current round and generation so ralphx-plan-verifier's Phase 0 RECOVER can resume.
        let current_round = item.metadata.current_round.unwrap_or(0);
        let generation = item.metadata.verification_generation.unwrap_or(0);
        let recovery_prompt = build_verification_recovery_prompt(current_round, generation);

        // Step 3: Build recovery metadata for audit trail
        let recovery_metadata = build_verification_recovery_metadata(
            parent_session_id,
            current_round,
            generation,
            item.metadata.plan_artifact_id.as_deref(),
        );

        // Step 4: Spawn the ralphx-plan-verifier agent via send_message().
        // Note: agent:run_started is emitted automatically by chat_service.send_message() spawn flow.
        let send_result = self
            .chat_service
            .send_message(
                ChatContextType::Ideation,
                child_session_id,
                &recovery_prompt,
                SendMessageOptions {
                    metadata: Some(recovery_metadata),
                    ..SendMessageOptions::default()
                },
            )
            .await;

        match send_result {
            Ok(_result) => {
                tracing::info!(
                    child_session_id = %child_session_id,
                    parent_session_id = %parent_session_id,
                    "RecoveryQueueProcessor: verification agent re-spawned successfully"
                );

                // Step 5: Emit agent:session_recovered for frontend notification.
                if let Some(ref handle) = self.app_handle {
                    use tauri::Emitter;
                    handle
                        .emit(
                            "agent:session_recovered",
                            serde_json::json!({
                                "session_id": child_session_id,
                                "parent_session_id": parent_session_id,
                                "context_type": "ideation",
                                "context_id": child_session_id,
                                "message": "Verification agent recovered after app restart"
                            }),
                        )
                        .ok();
                }
            }
            Err(e) => {
                tracing::error!(
                    child_session_id = %child_session_id,
                    parent_session_id = %parent_session_id,
                    error = %e,
                    "RecoveryQueueProcessor: verification agent re-spawn failed — falling back to Unverified"
                );

                // Step 6: Fallback — reset parent verification state to Unverified.
                // Matches current cold-boot behavior so the user can re-trigger verification.
                let parent_id = IdeationSessionId::from_string(parent_session_id.clone());
                match self.ideation_session_repo.get_by_id(&parent_id).await {
                    Ok(Some(parent_session)) => {
                        let reset_result = async {
                            let mut snapshot = load_current_verification_snapshot_or_default(
                                self.ideation_session_repo.as_ref(),
                                &parent_session,
                                VerificationStatus::Unverified,
                                false,
                            )
                            .await?;
                            clear_verification_snapshot(
                                &mut snapshot,
                                VerificationStatus::Unverified,
                                false,
                            );
                            self.ideation_session_repo
                                .save_verification_run_snapshot(&parent_id, &snapshot)
                                .await
                        }
                        .await;

                        if let Err(repo_err) = reset_result {
                            tracing::error!(
                                parent_session_id = %parent_session_id,
                                error = %repo_err,
                                "RecoveryQueueProcessor: failed to reset parent verification state after recovery failure"
                            );
                        }
                    }
                    Ok(None) => {
                        tracing::error!(
                            parent_session_id = %parent_session_id,
                            "RecoveryQueueProcessor: parent session missing during recovery failure fallback"
                        );
                    }
                    Err(repo_err) => {
                        tracing::error!(
                            parent_session_id = %parent_session_id,
                            error = %repo_err,
                            "RecoveryQueueProcessor: failed to load parent session for recovery failure fallback"
                        );
                    }
                }

                // Step 7: Archive the child session (unrecoverable — spawn failed).
                let child_id = IdeationSessionId::from_string(child_session_id.clone());
                if let Err(repo_err) = self
                    .ideation_session_repo
                    .update_status(&child_id, IdeationSessionStatus::Archived)
                    .await
                {
                    tracing::error!(
                        child_session_id = %child_session_id,
                        error = %repo_err,
                        "RecoveryQueueProcessor: failed to archive child session after recovery failure"
                    );
                }

                // Step 8: Emit frontend events for recovery failure.
                if let Some(ref handle) = self.app_handle {
                    use tauri::Emitter;
                    handle
                        .emit(
                            "agent:error",
                            serde_json::json!({
                                "session_id": child_session_id,
                                "parent_session_id": parent_session_id,
                                "context_type": "ideation",
                                "context_id": child_session_id,
                                "error": format!("Failed to recover verification agent: {}", e),
                                "recovery_failed": true,
                            }),
                        )
                        .ok();

                    // Emit verification status change so VerificationBadge shows Unverified
                    emit_verification_status_changed(
                        handle,
                        parent_session_id,
                        VerificationStatus::Unverified,
                        false,
                        None,
                        Some("recovery_failed"),
                        None, // generation not available without re-reading from DB
                    );
                }
            }
        }
    }
}

/// Build the recovery prompt for a verification agent re-spawn.
///
/// Injects a `<recovery_note>` tag so the ralphx-plan-verifier agent's Phase 0 RECOVER
/// logic can detect the restart and resume from the current round rather than
/// starting from round 1.
pub(crate) fn build_verification_recovery_prompt(current_round: u32, generation: u32) -> String {
    format!(
        "<recovery_note>Agent recovered after app restart. Resume verification from current \
         state. Current round: {current_round}, generation: {generation}.</recovery_note>"
    )
}

/// Build recovery metadata JSON for a verification agent spawn.
///
/// Used as `SendMessageOptions::metadata` for audit trail and context injection.
/// Includes parent session ID, current round, generation counter, and plan artifact ID.
pub(crate) fn build_verification_recovery_metadata(
    parent_session_id: &str,
    current_round: u32,
    generation: u32,
    plan_artifact_id: Option<&str>,
) -> String {
    serde_json::json!({
        "recovery_type": "verification_agent",
        "parent_session_id": parent_session_id,
        "current_round": current_round,
        "verification_generation": generation,
        "plan_artifact_id": plan_artifact_id,
    })
    .to_string()
}

/// Create a linked `(RecoveryQueue, RecoveryQueueProcessor)` pair.
///
/// The channel capacity is 256 items — sufficient for any realistic number of orphaned agents.
///
/// # Spawn ordering (NON-NEGOTIABLE — Constraint 9)
///
/// ```text
/// let (queue, processor) = create_recovery_queue(...);
/// tokio::spawn(processor.run());   // spawn FIRST
/// startup_scan().await;            // submits items — processor must be ready
/// ```
pub fn create_recovery_queue(
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    interactive_process_registry: Arc<InteractiveProcessRegistry>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    chat_service: Arc<dyn ChatService>,
    app_handle: Option<tauri::AppHandle<tauri::Wry>>,
    config: RecoveryQueueConfig,
) -> (RecoveryQueue, RecoveryQueueProcessor) {
    let (tx, rx) = mpsc::channel(256);
    let tx = Arc::new(tx);
    let queue = RecoveryQueue {
        tx,
        config: config.clone(),
    };
    let processor = RecoveryQueueProcessor {
        rx,
        running_agent_registry,
        interactive_process_registry,
        ideation_session_repo,
        chat_service,
        app_handle,
        config,
    };
    (queue, processor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::ChatContextType;

    fn make_item(priority: u8, kind: RecoveryKind) -> RecoveryItem {
        RecoveryItem {
            context_type: ChatContextType::Ideation,
            context_id: format!("session-{priority}"),
            recovery_kind: kind,
            priority,
            parent_session_id: None,
            metadata: RecoveryMetadata::default(),
        }
    }

    #[test]
    fn test_priority_sort_descending() {
        let mut batch = vec![
            make_item(1, RecoveryKind::VerificationAgent),
            make_item(5, RecoveryKind::IdeationAgent),
            make_item(3, RecoveryKind::VerificationAgent),
            make_item(10, RecoveryKind::IdeationAgent),
        ];
        batch.sort_by(|a, b| b.priority.cmp(&a.priority));
        assert_eq!(batch[0].priority, 10, "highest priority processed first");
        assert_eq!(batch[1].priority, 5);
        assert_eq!(batch[2].priority, 3);
        assert_eq!(batch[3].priority, 1, "lowest priority processed last");
    }

    #[test]
    fn test_parent_before_child_ordering() {
        // Parent ideation agents have higher priority than verification children
        let parent_item = make_item(10, RecoveryKind::IdeationAgent);
        let child_item = RecoveryItem {
            parent_session_id: Some("parent-session".to_string()),
            ..make_item(5, RecoveryKind::VerificationAgent)
        };
        let mut batch = vec![child_item, parent_item];
        batch.sort_by(|a, b| b.priority.cmp(&a.priority));
        assert!(
            matches!(batch[0].recovery_kind, RecoveryKind::IdeationAgent),
            "parent IdeationAgent processed before VerificationAgent child"
        );
    }

    #[test]
    fn test_config_defaults() {
        let config = RecoveryQueueConfig::default();
        assert_eq!(config.delay_between_spawns, Duration::from_secs(3));
        assert_eq!(config.max_concurrent_recoveries, 2);
        assert_eq!(config.recovery_timeout, Duration::from_secs(30));
        assert_eq!(config.max_retries, 0, "max_retries must be 0 per Constraint 8");
    }

    #[tokio::test]
    async fn test_submit_and_channel_delivery() {
        // Verify channel delivers submitted items in order
        let (tx, mut rx) = mpsc::channel::<RecoveryItem>(16);
        let tx = Arc::new(tx);
        let queue = RecoveryQueue {
            tx,
            config: RecoveryQueueConfig::default(),
        };
        queue
            .submit(make_item(10, RecoveryKind::IdeationAgent))
            .expect("submit should succeed on open channel");
        let received = rx.recv().await.expect("channel should deliver item");
        assert_eq!(received.priority, 10);
        assert!(matches!(received.recovery_kind, RecoveryKind::IdeationAgent));
    }

    #[tokio::test]
    async fn test_submit_fails_on_closed_channel() {
        let (tx, rx) = mpsc::channel::<RecoveryItem>(1);
        let tx = Arc::new(tx);
        let queue = RecoveryQueue {
            tx,
            config: RecoveryQueueConfig::default(),
        };
        drop(rx); // close receiver side
        let result = queue.submit(make_item(1, RecoveryKind::VerificationAgent));
        assert!(result.is_err(), "submit should fail when channel is closed");
    }

    #[test]
    fn test_build_verification_recovery_prompt() {
        let prompt = build_verification_recovery_prompt(3, 2);
        assert!(
            prompt.contains("<recovery_note>"),
            "prompt must contain recovery_note tag"
        );
        assert!(
            prompt.contains("Current round: 3"),
            "prompt must include current round"
        );
        assert!(
            prompt.contains("generation: 2"),
            "prompt must include generation"
        );
        assert!(
            prompt.contains("</recovery_note>"),
            "prompt must close recovery_note tag"
        );
    }

    #[test]
    fn test_build_verification_recovery_prompt_zero_round() {
        // Round 0 = not yet started, generation 0 = first generation
        let prompt = build_verification_recovery_prompt(0, 0);
        assert!(prompt.contains("Current round: 0"));
        assert!(prompt.contains("generation: 0"));
    }

    #[test]
    fn test_build_verification_recovery_metadata() {
        let metadata = build_verification_recovery_metadata(
            "parent-session-123",
            3,
            2,
            Some("artifact-456"),
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&metadata).expect("metadata must be valid JSON");
        assert_eq!(parsed["recovery_type"], "verification_agent");
        assert_eq!(parsed["parent_session_id"], "parent-session-123");
        assert_eq!(parsed["current_round"], 3);
        assert_eq!(parsed["verification_generation"], 2);
        assert_eq!(parsed["plan_artifact_id"], "artifact-456");
    }

    #[test]
    fn test_build_verification_recovery_metadata_no_artifact() {
        let metadata =
            build_verification_recovery_metadata("parent-123", 1, 1, None);
        let parsed: serde_json::Value =
            serde_json::from_str(&metadata).expect("metadata must be valid JSON");
        assert!(
            parsed["plan_artifact_id"].is_null(),
            "plan_artifact_id should be null when not provided"
        );
    }
}
