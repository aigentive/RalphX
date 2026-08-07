//! Durable queue projection and coordinator wake scheduling.

use chrono::Utc;

use crate::application::managed_team::lifecycle::advance_binding_status;
use crate::application::managed_team::{render_team_origin_message, ManagedTeamService};
use crate::domain::entities::{
    AgentRunStatus, ChatContextType, TeamMessage, TeamMessageActorKind, TeamMessageDelivery,
    TeamMessageDeliveryStatus, TeamRunBindingStatus, TeamRunTriggerKind, TeamSessionId,
    TeamWakeBatch, TeamWakeBatchId, TeamWakeBatchStatus, TeamWakeRecipientKind,
};
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::error::{AppError, AppResult};

use super::messaging::member_can_receive;

const DELIVERY_PROJECTION_LIMIT: u32 = 128;
/// Emitted when a coordinator wake is dropped because the Team exhausted its
/// automatic wake budget. Suppression here means settlement messages sit in the
/// durable queue with nothing scheduled to drain them.
const TEAM_WAKE_SUPPRESSED_EVENT: &str = "team:needs_attention";

impl ManagedTeamService {
    pub(super) async fn project_actionable_deliveries(
        &self,
        team_id: &TeamSessionId,
    ) -> AppResult<Vec<(TeamMessage, TeamMessageDelivery)>> {
        if !self.startup_barrier().delivery_projection_released().await {
            return Err(AppError::Conflict(
                "managed Team recovery barrier has not released delivery projection".to_string(),
            ));
        }
        let deliveries = self
            .message_repo
            .list_actionable_deliveries(team_id, DELIVERY_PROJECTION_LIMIT)
            .await?;
        let mut queued = Vec::new();
        for delivery in deliveries {
            let Some(message) = self.message_repo.get_message(&delivery.message_id).await? else {
                return Err(AppError::Conflict(
                    "managed Team delivery envelope disappeared".to_string(),
                ));
            };
            if let Some(delivery) = self.project_delivery(&message, delivery).await? {
                queued.push((message, delivery));
            }
        }
        Ok(queued)
    }

    async fn project_delivery(
        &self,
        message: &TeamMessage,
        delivery: TeamMessageDelivery,
    ) -> AppResult<Option<TeamMessageDelivery>> {
        let key = match delivery.recipient_kind {
            TeamMessageActorKind::Coordinator => {
                let session = self.open_session(&message.team_id).await?;
                let conversation = self
                    .chat_conversation_repo
                    .get_by_id(&session.coordinator_conversation_id)
                    .await?
                    .ok_or_else(|| {
                        AppError::NotFound(
                            "Team coordinator conversation was not found".to_string(),
                        )
                    })?;
                // A Project conversation's `context_id` is the *project* id
                // (enforced by `ensure_team`), but the coordinator's turn-end
                // drain keys its queue by `runtime_context_id`, which for a
                // Project send carrying `conversation_id_override` is the
                // *conversation* id. Keying by `context_id` here would park
                // the settlement message on a queue nothing drains, silently
                // losing it. Same convention as `question_commands.rs` and
                // `plan_verification_service.rs`.
                QueueKey::new(conversation.context_type, conversation.id.as_str())
            }
            TeamMessageActorKind::Member => {
                let member_id = delivery.recipient_member_id.as_ref().ok_or_else(|| {
                    AppError::Conflict("member delivery has no recipient identity".to_string())
                })?;
                let member = self.team_repo.get_member(member_id).await?.ok_or_else(|| {
                    AppError::NotFound("Team delivery member was not found".to_string())
                })?;
                if member.team_id != message.team_id
                    || delivery.recipient_generation != Some(member.generation)
                    || !member_can_receive(&member)
                {
                    return self
                        .fail_delivery(delivery, "recipient_generation_stale")
                        .await;
                }
                let Some(delegated_session_id) = member.delegated_session_id else {
                    return Ok(None);
                };
                QueueKey::new(ChatContextType::Delegation, delegated_session_id.as_str())
            }
            TeamMessageActorKind::System => {
                return Err(AppError::Validation(
                    "Team deliveries cannot target system actors".to_string(),
                ));
            }
        };
        let sender_name = match message.sender_member_id.as_ref() {
            Some(member_id) => self
                .team_repo
                .get_member(member_id)
                .await?
                .map(|member| member.name),
            None => None,
        };
        let mut queued = QueuedMessage::with_id(
            delivery.id.0.clone(),
            render_team_origin_message(message, sender_name.as_deref()),
        );
        queued.created_at = message.created_at.to_rfc3339();
        if let Err(error) = self.queued_message_repo.enqueue_back(&key, &queued).await {
            // Mark the delivery replayable, but the projection failure itself
            // must surface: a swallowed queue error would report false success.
            let _ = self
                .fail_delivery(delivery, "queue_projection_failed")
                .await;
            return Err(error);
        }
        let expected = delivery.status;
        let mut updated = delivery;
        updated
            .transition_to(TeamMessageDeliveryStatus::Queued, Utc::now())
            .map_err(AppError::Validation)?;
        updated.queued_message_id = Some(queued.id);
        updated.attempt_count += 1;
        updated.next_retry_at = None;
        updated.last_error = None;
        if self
            .message_repo
            .transition_delivery(
                &updated.id,
                expected,
                TeamMessageDeliveryStatus::Queued,
                updated.clone(),
            )
            .await?
        {
            Ok(Some(updated))
        } else {
            Ok(None)
        }
    }

    async fn fail_delivery(
        &self,
        delivery: TeamMessageDelivery,
        error_code: &str,
    ) -> AppResult<Option<TeamMessageDelivery>> {
        if delivery.status == TeamMessageDeliveryStatus::Failed {
            return Ok(None);
        }
        let expected = delivery.status;
        let mut failed = delivery;
        failed
            .transition_to(TeamMessageDeliveryStatus::Failed, Utc::now())
            .map_err(AppError::Validation)?;
        failed.attempt_count += 1;
        failed.last_error = Some(error_code.to_string());
        if self
            .message_repo
            .transition_delivery(
                &failed.id,
                expected,
                TeamMessageDeliveryStatus::Failed,
                failed.clone(),
            )
            .await?
        {
            Ok(Some(failed))
        } else {
            Ok(None)
        }
    }

    /// Terminalizes wake run bindings whose coordinator run has already ended.
    ///
    /// A coordinator wake binding is created at `Planned` by
    /// `claim_next_actionable_wake_batch` and advanced to `Running` once the
    /// wake send is accepted, but nothing terminalizes it afterwards: the
    /// coordinator's run finishes through the generic chat-service
    /// finalization path, which knows nothing about Team bindings, and the
    /// member settlement hook (`settle_member_assignment`) only ever fires for
    /// member runs. Without this reconciliation every *successful* wake would
    /// permanently consume automatic-wake budget, so a Team would stop waking
    /// for good after `automatic_wake_limit` successes.
    ///
    /// The agent-run row is the authority. A run that is still `Running`, or
    /// whose row is not visible yet (the send may still be in flight against a
    /// preallocated id), is left alone — a read must never manufacture budget.
    /// Repo errors propagate rather than being read as "the run ended".
    async fn reconcile_wake_run_bindings(&self, team_id: &TeamSessionId) -> AppResult<()> {
        for binding in self.run_binding_repo.list_for_team(team_id).await? {
            if binding.trigger_kind != TeamRunTriggerKind::WakeBatch
                || !matches!(
                    binding.status,
                    TeamRunBindingStatus::Planned
                        | TeamRunBindingStatus::Launching
                        | TeamRunBindingStatus::Running
                )
            {
                continue;
            }
            let Some(run) = self.agent_run_repo.get_by_id(&binding.agent_run_id).await? else {
                continue;
            };
            let next = match run.status {
                AgentRunStatus::Running => continue,
                AgentRunStatus::Completed => TeamRunBindingStatus::Terminal,
                AgentRunStatus::Failed => TeamRunBindingStatus::Failed,
                AgentRunStatus::Cancelled => TeamRunBindingStatus::Cancelled,
            };
            let expected_version = binding.version;
            let mut resolved = binding;
            if !advance_binding_status(&mut resolved, next, Utc::now()) {
                continue;
            }
            resolved.version = expected_version + 1;
            let binding_id = resolved.id.clone();
            // A lost CAS means a concurrent dispatcher/exit already settled the
            // binding, which is the same end state this pass wanted.
            let _ = self
                .run_binding_repo
                .transition(&binding_id, expected_version, resolved)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn schedule_coordinator_wakes(
        &self,
        queued: &[(TeamMessage, TeamMessageDelivery)],
    ) -> AppResult<()> {
        if !self.startup_barrier().delivery_projection_released().await {
            return Ok(());
        }
        for (message, delivery) in queued {
            if delivery.recipient_kind != TeamMessageActorKind::Coordinator {
                continue;
            }
            let session = self.open_session(&message.team_id).await?;
            self.admit_dispatch(&session.id).await?;
            if self
                .agent_run_repo
                .get_active_for_conversation(&session.coordinator_conversation_id)
                .await?
                .is_some()
            {
                tracing::debug!(
                    team_id = session.id.as_str(),
                    "managed Team coordinator wake suppressed: coordinator already has an active run"
                );
                continue;
            }
            self.reconcile_wake_run_bindings(&session.id).await?;
            let bindings = self.run_binding_repo.list_for_team(&session.id).await?;
            let running_count = bindings
                .iter()
                .filter(|binding| binding.status == TeamRunBindingStatus::Running)
                .count() as u32;
            // Only live (not-yet-settled) wake bindings consume automatic wake
            // budget. `TeamRunBindingRepository` has no delete method, so
            // Terminal/Failed/Cancelled WakeBatch bindings persist for the
            // life of the Team row; counting them unfiltered would make the
            // budget decay to zero and never recover.
            let wake_count = bindings
                .iter()
                .filter(|binding| {
                    binding.trigger_kind == TeamRunTriggerKind::WakeBatch
                        && matches!(
                            binding.status,
                            TeamRunBindingStatus::Planned
                                | TeamRunBindingStatus::Launching
                                | TeamRunBindingStatus::Running
                        )
                })
                .count() as u32;
            if running_count >= session.effective_concurrency
                || wake_count >= session.automatic_wake_limit
            {
                let wake_budget_exhausted = wake_count >= session.automatic_wake_limit;
                tracing::warn!(
                    team_id = session.id.as_str(),
                    running_count,
                    effective_concurrency = session.effective_concurrency,
                    wake_count,
                    automatic_wake_limit = session.automatic_wake_limit,
                    wake_budget_exhausted,
                    "managed Team coordinator wake suppressed: concurrency or automatic wake budget exhausted"
                );
                if wake_budget_exhausted {
                    // A Team that has stopped waking must be diagnosable from the
                    // UI, not only from a queue inspection.
                    self.event_sink.emit(
                        TEAM_WAKE_SUPPRESSED_EVENT,
                        serde_json::json!({
                            "team_id": session.id.as_str(),
                            "coordinator_conversation_id":
                                session.coordinator_conversation_id.as_str(),
                            "wake_count": wake_count,
                            "automatic_wake_limit": session.automatic_wake_limit,
                            "reason": "automatic_wake_budget_exhausted",
                        }),
                    );
                }
                continue;
            }
            let now = Utc::now();
            self.wake_batch_repo
                .create_or_extend_active(TeamWakeBatch {
                    id: TeamWakeBatchId::new(),
                    team_id: session.id.clone(),
                    recipient_kind: TeamWakeRecipientKind::Coordinator,
                    recipient_member_id: None,
                    recipient_generation: None,
                    first_message_sequence: message.sequence,
                    last_message_sequence: message.sequence,
                    delivery_ids: vec![delivery.id.clone()],
                    status: TeamWakeBatchStatus::Queued,
                    planned_agent_run_id: None,
                    bound_agent_run_id: None,
                    attempt_count: 0,
                    budget_count: 1,
                    lease_token: None,
                    version: 0,
                    last_error: None,
                    created_at: now,
                    updated_at: now,
                    settled_at: None,
                })
                .await?;
        }
        Ok(())
    }
}
