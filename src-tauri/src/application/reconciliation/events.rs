// Evidence building, event recording, prompts, and lookups for reconciliation.

use tauri::{Emitter, Runtime};
use tracing::warn;

use crate::domain::entities::{
    AgentRun, AgentRunId, ChatContextType, InternalStatus, MergeFailureSource, MergeRecoveryEvent,
    MergeRecoveryEventKind, MergeRecoveryMetadata, MergeRecoveryReasonCode, MergeRecoverySource,
    MergeRecoveryState, Task, TaskId,
};
use crate::domain::state_machine::transition_handler::set_trigger_origin;
use crate::application::harness_runtime_registry::default_reconciliation_merger_timeout_secs;

use super::policy::{RecoveryContext, RecoveryEvidence, RecoveryPromptAction, RecoveryPromptEvent};
use super::ReconciliationRunner;

impl<R: Runtime> ReconciliationRunner<R> {
    pub(crate) async fn build_run_evidence(
        &self,
        task: &Task,
        context_type: ChatContextType,
        run: Option<&AgentRun>,
    ) -> RecoveryEvidence {
        let registry_running = self
            .running_agent_registry
            .is_running(&crate::domain::services::RunningAgentKey::new(
                context_type.to_string(),
                task.id.as_str(),
            ))
            .await;

        RecoveryEvidence {
            run_status: run.map(|r| r.status),
            registry_running,
            can_start: self.execution_state.can_start_task(),
            is_stale: false,
            is_deferred: false,
        }
    }

    pub(crate) async fn load_execution_run(
        &self,
        task: &Task,
        status: InternalStatus,
    ) -> Option<AgentRun> {
        let status_history = match self.task_repo.get_status_history(&task.id).await {
            Ok(history) => history,
            Err(e) => {
                warn!(
                    task_id = task.id.as_str(),
                    error = %e,
                    "Failed to load status history for reconciliation"
                );
                return None;
            }
        };

        let latest_transition = status_history
            .iter()
            .rev()
            .find(|transition| transition.to == status && transition.agent_run_id.is_some());

        let run = if let Some(transition) = latest_transition {
            let agent_run_id = transition.agent_run_id.as_ref()?;

            match self
                .agent_run_repo
                .get_by_id(&AgentRunId::from_string(agent_run_id))
                .await
            {
                Ok(run) => run,
                Err(e) => {
                    warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to load agent run for reconciliation"
                    );
                    return None;
                }
            }
        } else {
            // Registry-aware fallback: when status_history metadata hasn't been
            // linked yet (async race between persist_status_change and
            // update_latest_state_history_metadata), check the running agent
            // registry for the correct agent_run_id before falling back to
            // the conversation-based lookup which may return a stale/cancelled run.
            let key = crate::domain::services::RunningAgentKey::new(
                ChatContextType::TaskExecution.to_string(),
                task.id.as_str(),
            );
            if let Some(info) = self.running_agent_registry.get(&key).await {
                match self
                    .agent_run_repo
                    .get_by_id(&AgentRunId::from_string(&info.agent_run_id))
                    .await
                {
                    Ok(run) => run,
                    Err(e) => {
                        warn!(
                            task_id = task.id.as_str(),
                            agent_run_id = %info.agent_run_id,
                            error = %e,
                            "Failed to load registry agent run for reconciliation"
                        );
                        None
                    }
                }
            } else {
                self.lookup_latest_run_for_task_context(task, ChatContextType::TaskExecution)
                    .await
            }
        };

        run
    }

    #[doc(hidden)]
    pub async fn latest_status_transition_age(
        &self,
        task: &Task,
        status: InternalStatus,
    ) -> Option<chrono::Duration> {
        let status_history = match self.task_repo.get_status_history(&task.id).await {
            Ok(history) => history,
            Err(e) => {
                warn!(
                    task_id = task.id.as_str(),
                    status = ?status,
                    error = %e,
                    "Failed to load status history; falling back to task.updated_at for age"
                );
                return Some(chrono::Utc::now() - task.updated_at);
            }
        };

        if let Some(latest_transition) = status_history
            .iter()
            .rev()
            .find(|transition| transition.to == status)
        {
            return Some(chrono::Utc::now() - latest_transition.timestamp);
        }

        // Fallback for manually-edited tasks (status changed without status history row).
        Some(chrono::Utc::now() - task.updated_at)
    }

    /// Record auto-retry event with optional source SHA (for SHA comparison guard).
    pub(crate) async fn record_merge_auto_retry_event_with_sha(
        &self,
        task: &Task,
        attempt: u32,
        failure_source: MergeFailureSource,
        retry_reason: &str,
        source_sha: Option<&str>,
        reason_code: Option<MergeRecoveryReasonCode>,
    ) -> Result<(), String> {
        let mut updated = task.clone();
        let mut recovery = MergeRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
            .unwrap_or(None)
            .unwrap_or_default();

        let reason = if failure_source == MergeFailureSource::TargetBranchBusy {
            MergeRecoveryReasonCode::TargetBranchBusy
        } else {
            reason_code.unwrap_or(MergeRecoveryReasonCode::GitError)
        };
        let mut event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AutoRetryTriggered,
            MergeRecoverySource::Auto,
            reason,
            format!(
                "Auto-retry triggered (attempt {}, source={:?}): {}",
                attempt, failure_source, retry_reason
            ),
        )
        .with_attempt(attempt)
        .with_failure_source(failure_source);

        if let Some(ref source_branch) = updated.task_branch {
            event = event.with_source_branch(source_branch.clone());
        }
        if let Some(sha) = source_sha {
            event = event.with_source_sha(sha);
        }

        recovery.append_event_with_state(event, MergeRecoveryState::Retrying);
        updated.metadata = Some(
            recovery
                .update_task_metadata(updated.metadata.as_deref())
                .map_err(|e| e.to_string())?,
        );
        set_trigger_origin(&mut updated, "recovery");
        updated.touch();

        self.task_repo
            .update(&updated)
            .await
            .map_err(|e| e.to_string())
    }

    #[doc(hidden)]
    pub fn latest_deferred_blocker_id(&self, task: &Task) -> Option<TaskId> {
        MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
            .ok()
            .flatten()
            .and_then(|meta| {
                meta.events
                    .iter()
                    .rev()
                    .find_map(|event| event.blocking_task_id.clone())
            })
    }

    pub(crate) async fn deferred_blocker_is_active(&self, blocker_id: &TaskId) -> bool {
        match self.task_repo.get_by_id(blocker_id).await {
            Ok(Some(blocker)) => {
                blocker.archived_at.is_none()
                    && matches!(
                        blocker.internal_status,
                        InternalStatus::PendingMerge | InternalStatus::Merging
                    )
            }
            Ok(None) => false,
            Err(e) => {
                warn!(
                    blocker_id = blocker_id.as_str(),
                    error = %e,
                    "Failed to load deferred merge blocker status"
                );
                true
            }
        }
    }

    pub(crate) async fn record_merge_auto_retry_event(
        &self,
        task: &Task,
        attempt: u32,
        failure_source: MergeFailureSource,
        retry_reason: &str,
        reason_code: Option<MergeRecoveryReasonCode>,
    ) -> Result<(), String> {
        let mut updated = task.clone();
        let mut recovery = MergeRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
            .unwrap_or(None)
            .unwrap_or_default();

        let reason = if failure_source == MergeFailureSource::TargetBranchBusy {
            MergeRecoveryReasonCode::TargetBranchBusy
        } else {
            reason_code.unwrap_or(MergeRecoveryReasonCode::GitError)
        };
        let mut event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AutoRetryTriggered,
            MergeRecoverySource::Auto,
            reason,
            format!(
                "Auto-retry triggered (attempt {}, source={:?}): {}",
                attempt, failure_source, retry_reason
            ),
        )
        .with_attempt(attempt)
        .with_failure_source(failure_source);

        if let Some(ref source_branch) = updated.task_branch {
            event = event.with_source_branch(source_branch.clone());
        }

        recovery.append_event_with_state(event, MergeRecoveryState::Retrying);
        updated.metadata = Some(
            recovery
                .update_task_metadata(updated.metadata.as_deref())
                .map_err(|e| e.to_string())?,
        );
        set_trigger_origin(&mut updated, "recovery");
        updated.touch();

        self.task_repo
            .update(&updated)
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn record_merge_timeout_event(&self, task: &Task, _age: chrono::Duration) {
        let mut updated = task.clone();

        let mut recovery = MergeRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
            .unwrap_or(None)
            .unwrap_or_default();

        // Use configured timeout, not effective_age, because effective_age accumulates
        // across retries (ExecuteEntryActions doesn't create new status_history entries,
        // so latest_status_transition_age returns the original Merging timestamp).
        let timeout_secs = default_reconciliation_merger_timeout_secs() as i64;

        let failed_event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AttemptFailed,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::GitError,
            format!(
                "Merge timed out after {}s without completion signal",
                timeout_secs
            ),
        )
        .with_failure_source(MergeFailureSource::TransientGit);
        recovery.append_event_with_state(failed_event, MergeRecoveryState::Failed);

        let mut metadata = match recovery.update_task_metadata(updated.metadata.as_deref()) {
            Ok(json) => serde_json::from_str::<serde_json::Value>(&json)
                .unwrap_or_else(|_| serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        };
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "error".to_string(),
                serde_json::json!(format!(
                    "Merge timed out after {}s without complete_merge callback",
                    timeout_secs
                )),
            );
            obj.insert(
                "merge_timeout_seconds".to_string(),
                serde_json::json!(timeout_secs),
            );
            obj.insert(
                "merge_timeout_at".to_string(),
                serde_json::json!(chrono::Utc::now().to_rfc3339()),
            );
        }

        updated.metadata = Some(metadata.to_string());
        updated.touch();
        if let Err(e) = self.task_repo.update(&updated).await {
            warn!(
                task_id = task.id.as_str(),
                error = %e,
                "Failed to persist merge timeout metadata"
            );
        }
    }

    pub(crate) async fn emit_recovery_prompt(
        &self,
        task: &Task,
        status: InternalStatus,
        context: RecoveryContext,
        reason: String,
    ) -> bool {
        let Some(handle) = &self.app_handle else {
            return false;
        };

        let key = format!("{}:{:?}", task.id.as_str(), status);
        {
            let mut prompted = self.prompt_tracker.lock().await;
            if prompted.contains(&key) {
                return false;
            }
            prompted.insert(key);
        }

        let context_type = match context {
            RecoveryContext::Execution => "execution",
            RecoveryContext::Review => "review",
            RecoveryContext::Merge => "merge",
            RecoveryContext::PendingMerge => "pending_merge",
            RecoveryContext::QaRefining => "qa_refining",
            RecoveryContext::QaTesting => "qa_testing",
        };

        let payload = RecoveryPromptEvent {
            task_id: task.id.as_str().to_string(),
            status,
            context_type: context_type.to_string(),
            reason,
            primary_action: RecoveryPromptAction {
                id: "restart".to_string(),
                label: "Restart".to_string(),
            },
            secondary_action: RecoveryPromptAction {
                id: "cancel".to_string(),
                label: "Cancel".to_string(),
            },
        };

        let _ = handle.emit("recovery:prompt", payload);
        true
    }

    pub(crate) async fn clear_prompt_marker(&self, task_id: &str, status: InternalStatus) {
        let key = format!("{}:{:?}", task_id, status);
        let mut prompted = self.prompt_tracker.lock().await;
        prompted.remove(&key);
    }

    pub(crate) async fn lookup_latest_run_for_task_context(
        &self,
        task: &Task,
        context_type: ChatContextType,
    ) -> Option<AgentRun> {
        let conversations = self
            .chat_conversation_repo
            .get_by_context(context_type, task.id.as_str())
            .await
            .ok()?;

        let latest_conversation = conversations
            .into_iter()
            .max_by_key(|conv| conv.created_at)?;

        self.agent_run_repo
            .get_latest_for_conversation(&latest_conversation.id)
            .await
            .ok()
            .flatten()
    }
}
