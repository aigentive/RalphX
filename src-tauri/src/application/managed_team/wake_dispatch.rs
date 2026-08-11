//! The missing consumer for durably claimed coordinator wake batches.
//!
//! `ManagedTeamService::claim_next_actionable_wake_batch` does the durable CAS half
//! (`Queued -> Launching` plus the preallocated coordinator run binding); this
//! dispatcher performs the actual hidden resume-in-place send and settles the
//! claimed batch, modelled on `application::delegation_park::wake_dispatch`.

use std::time::Duration;

use chrono::Utc;
use ralphx_events::EventSink;
use std::sync::Arc;

use crate::application::chat_service::{
    ChatService, SendCallerContext, SendMessageOptions, SendQueuePolicy,
};
use crate::application::managed_team::lifecycle::advance_binding_status;
use crate::application::managed_team::render_team_origin_message;
use crate::domain::entities::{
    AgentRunActionKind, AgentRunId, ChatConversation, TeamMessage, TeamMessageActorKind,
    TeamMessageDeliveryStatus, TeamMessageEnvelopeTarget, TeamRunBindingStatus, TeamSession,
    TeamSessionId, TeamWakeBatch, TeamWakeBatchStatus,
};
use crate::domain::repositories::{AgentRunRepository, ChatConversationRepository};
use crate::domain::services::QueueKey;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::delegation_config;

use super::service::ManagedTeamService;

/// Emitted whenever a managed Team wake needs operator attention, matching the
/// event name used by the wake-budget suppression path in `messaging_delivery.rs`.
const TEAM_WAKE_FAILED_EVENT: &str = "team:needs_attention";

/// Upper bound on batches drained per Team in one startup pass. A crash can
/// leave several queued batches behind; the bound keeps recovery from looping
/// on a Team whose batches keep re-queueing.
const STARTUP_DRAIN_MAX_BATCHES_PER_TEAM: usize = 8;

/// True when the coordinator is an actual recipient of `message`.
///
/// A wake batch is claimed for a coordinator-addressed delivery, but its
/// `first..=last` sequence range is a Team-wide window: `list_messages_after`
/// also returns member-targeted traffic and the coordinator's own outbound
/// messages. Re-injecting those would feed the coordinator its own instructions
/// (and other members' mail) back as new input.
fn addressed_to_coordinator(message: &TeamMessage) -> bool {
    message.sender_kind != TeamMessageActorKind::Coordinator
        && matches!(
            message.target_kind,
            TeamMessageEnvelopeTarget::Coordinator | TeamMessageEnvelopeTarget::Broadcast
        )
}

/// Application service that turns durably claimed `TeamWakeBatch` rows into a
/// hidden resume-in-place coordinator turn.
pub struct ManagedTeamWakeDispatcher {
    managed_team: Arc<ManagedTeamService>,
    chat_service: Arc<dyn ChatService>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    events: Arc<dyn EventSink>,
}

impl ManagedTeamWakeDispatcher {
    pub fn new(
        managed_team: Arc<ManagedTeamService>,
        chat_service: Arc<dyn ChatService>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            managed_team,
            chat_service,
            agent_run_repo,
            conversation_repo,
            events,
        }
    }

    /// Claims and dispatches at most one durable wake batch for `team_id`.
    ///
    /// # Errors
    ///
    /// Propagates claim/read failures. Send failures are retried, then durably
    /// settled to `Failed` and returned as `Ok(())` so a stuck Team is
    /// diagnosable without failing the caller (settlement effect, e.g. member
    /// settlement, must not depend on wake delivery).
    pub async fn dispatch_pending_wakes(&self, team_id: &TeamSessionId) -> AppResult<()> {
        self.dispatch_next_wake(team_id).await.map(|_| ())
    }

    /// Drains queued wake batches for every open Team, bounded per Team.
    ///
    /// Startup recovery calls this after
    /// `release_delivery_projection_after_recovery` re-projects deliveries and
    /// re-queues their batches: a process that crashed between queue and
    /// dispatch has durable batches that no settlement will ever re-trigger.
    /// A failing Team is logged and skipped so one bad Team cannot stall
    /// recovery for the rest.
    ///
    /// Returns the number of batches claimed and processed (dispatched,
    /// cancelled as redundant, or durably failed).
    ///
    /// # Errors
    ///
    /// Propagates only the open-session enumeration failure; per-Team dispatch
    /// failures are logged and skipped.
    pub async fn drain_open_team_wakes(&self) -> AppResult<usize> {
        let mut processed = 0usize;
        for session in self.managed_team.team_repo.list_open_sessions().await? {
            for _ in 0..STARTUP_DRAIN_MAX_BATCHES_PER_TEAM {
                match self.dispatch_next_wake(&session.id).await {
                    Ok(true) => processed += 1,
                    Ok(false) => break,
                    Err(error) => {
                        tracing::warn!(
                            team_id = session.id.as_str(),
                            %error,
                            "managed Team startup wake drain failed for this Team; the next settlement will retry"
                        );
                        break;
                    }
                }
            }
        }
        Ok(processed)
    }

    /// Claims and processes at most one batch. `Ok(false)` means nothing was
    /// queued, which is the drain loop's stop condition.
    async fn dispatch_next_wake(&self, team_id: &TeamSessionId) -> AppResult<bool> {
        let Some(batch) = self
            .managed_team
            .claim_next_actionable_wake_batch(team_id)
            .await?
        else {
            return Ok(false);
        };
        self.dispatch_claimed_batch(batch).await?;
        Ok(true)
    }

    async fn dispatch_claimed_batch(&self, batch: TeamWakeBatch) -> AppResult<()> {
        let session = self.managed_team.open_session(&batch.team_id).await?;
        // `project_delivery` already enqueued the settlement message onto the
        // coordinator's durable `QueueKey`, so a mid-turn coordinator drains it
        // at turn end. Dispatching here too would produce a duplicate turn.
        //
        // This is only safe because `project_delivery` keys that queue by the
        // coordinator's *conversation* id, which is exactly the
        // `runtime_context_id` the turn-end drain reads. Re-keying it by
        // `context_id` (the project id) would silently strand the message and
        // turn the cancellation below into lost work.
        if self
            .agent_run_repo
            .get_active_for_conversation(&session.coordinator_conversation_id)
            .await?
            .is_some()
        {
            tracing::debug!(
                team_id = session.id.as_str(),
                wake_batch_id = batch.id.0.as_str(),
                "managed Team wake dispatch suppressed: coordinator already has an active run"
            );
            // The claimed batch and its preallocated run binding must not stay
            // stuck at Launching/Planned forever (that would permanently
            // consume automatic-wake budget): the mid-turn coordinator drains
            // the already-enqueued message through the normal queue path at
            // turn end, so this wake path is no longer needed for it.
            self.cancel_batch(&batch).await;
            return Ok(());
        }

        let Some(planned_run_id) = batch.planned_agent_run_id.clone() else {
            return self
                .fail_batch(&batch, "wake batch is missing its planned run id")
                .await;
        };

        let mut running = batch.clone();
        if let Err(error) = running.transition_to(TeamWakeBatchStatus::Running, Utc::now()) {
            return self.fail_batch(&batch, &error).await;
        }
        let claimed_version = batch.version;
        running.version += 1;
        if !self
            .managed_team
            .wake_batch_repo
            .transition(
                &batch.id,
                claimed_version,
                TeamWakeBatchStatus::Launching,
                running.clone(),
            )
            .await?
        {
            // Lost the Launching -> Running race to a concurrent dispatcher, a
            // Team exit cancellation, or startup recovery. Not our turn to run.
            return Ok(());
        }

        let (conversation, content, options) =
            match self.prepare_wake(&session, &running, &planned_run_id).await {
                // Nothing in the batch's range is addressed to the coordinator,
                // so there is no turn to run. Settle rather than fail: the batch
                // was handled correctly and must release its wake budget.
                Ok(None) => return self.settle_batch(&running).await,
                Ok(Some(prepared)) => prepared,
                Err(error) => return self.fail_batch(&running, &error.to_string()).await,
            };

        let config = delegation_config();
        let retry_max = config.park_wake_retry_max.max(1);
        let mut attempts: u32 = 0;
        loop {
            attempts += 1;
            match self
                .chat_service
                .send_message(
                    conversation.context_type,
                    &conversation.context_id,
                    &content,
                    options.clone(),
                )
                .await
            {
                Ok(_) => {
                    self.retire_delivered_queue_entries(&conversation, &running)
                        .await;
                    return self.settle_batch(&running).await;
                }
                Err(error) => {
                    let error_message = error.to_string();
                    if attempts >= retry_max {
                        return self.fail_batch(&running, &error_message).await;
                    }
                    match self.record_retry(&running, &error_message).await? {
                        Some(updated) => running = updated,
                        None => return Ok(()),
                    }
                    tokio::time::sleep(Duration::from_secs(config.park_wake_retry_backoff_secs))
                        .await;
                }
            }
        }
    }

    /// Retires the durable queue entries `project_delivery` created for this
    /// batch's coordinator deliveries. `project_delivery` enqueues each
    /// coordinator-addressed delivery onto the coordinator's normal `QueueKey`
    /// keyed by delivery id (`messaging_delivery.rs`), so once this dispatch
    /// has successfully delivered the same content via a hidden resume send,
    /// leaving those entries queued means the turn-end drain
    /// (`process_queued_messages`) starts a second, redundant coordinator turn
    /// with identical content.
    ///
    /// Best-effort: a failed delete or delivery transition is a duplicate
    /// turn, not lost work, and must not fail an already-delivered wake.
    async fn retire_delivered_queue_entries(
        &self,
        conversation: &ChatConversation,
        batch: &TeamWakeBatch,
    ) {
        let key = QueueKey::new(conversation.context_type, conversation.id.as_str());
        for delivery_id in &batch.delivery_ids {
            if let Err(error) = self
                .managed_team
                .queued_message_repo
                .delete(&key, delivery_id.0.as_str())
                .await
            {
                tracing::warn!(
                    team_id = batch.team_id.as_str(),
                    wake_batch_id = batch.id.0.as_str(),
                    delivery_id = delivery_id.0.as_str(),
                    %error,
                    "managed Team wake dispatch failed to retire a delivered queue entry; the turn-end drain may re-deliver it"
                );
            }
            let Ok(Some(delivery)) = self
                .managed_team
                .message_repo
                .get_delivery(delivery_id)
                .await
            else {
                continue;
            };
            if delivery.status != TeamMessageDeliveryStatus::Queued {
                continue;
            }
            let expected = delivery.status;
            let mut delivered = delivery;
            if delivered
                .transition_to(TeamMessageDeliveryStatus::Delivered, Utc::now())
                .is_err()
            {
                continue;
            }
            let _ = self
                .managed_team
                .message_repo
                .transition_delivery(
                    delivery_id,
                    expected,
                    TeamMessageDeliveryStatus::Delivered,
                    delivered,
                )
                .await;
        }
    }

    async fn prepare_wake(
        &self,
        session: &TeamSession,
        batch: &TeamWakeBatch,
        planned_run_id: &AgentRunId,
    ) -> AppResult<Option<(ChatConversation, String, SendMessageOptions)>> {
        let conversation = self
            .conversation_repo
            .get_by_id(&session.coordinator_conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Team coordinator conversation not found: {}",
                    session.coordinator_conversation_id
                ))
            })?;
        let latest_run = self
            .agent_run_repo
            .get_latest_for_conversation(&session.coordinator_conversation_id)
            .await?;
        let Some(content) = self.build_wake_message(batch).await? else {
            return Ok(None);
        };
        let options = SendMessageOptions {
            preallocated_agent_run_id: Some(planned_run_id.clone()),
            // `persist_hidden_marker` selects chat-service's canonical MessageRole::System
            // persistence for hidden resume-in-place markers, same contract as delegation park.
            metadata: Some(
                serde_json::json!({
                    "source": "managed_team",
                    "resume_in_place": true,
                    "persist_hidden_marker": true,
                    "hidden_from_ui": true,
                    "recovery_context": true,
                    "ralphx_action_kind": AgentRunActionKind::ManagedTeamWake.to_string(),
                    "ralphx_action_context_id": session.coordinator_conversation_id.as_str(),
                    "ralphx_action_target_id": batch.id.0.as_str(),
                })
                .to_string(),
            ),
            queue_policy: SendQueuePolicy::AllowQueue,
            conversation_id_override: Some(session.coordinator_conversation_id),
            harness_override: latest_run.as_ref().and_then(|run| run.harness),
            model_override: latest_run.as_ref().and_then(|run| {
                run.logical_model
                    .clone()
                    .or_else(|| run.effective_model_id.clone())
            }),
            logical_effort_override: latest_run.as_ref().and_then(|run| run.logical_effort),
            service_tier_override: latest_run.and_then(|run| run.service_tier),
            caller_context: SendCallerContext::DrainService,
            ..Default::default()
        };
        Ok(Some((conversation, content, options)))
    }

    /// Renders the wake payload, or `None` when the batch's sequence range
    /// holds nothing the coordinator should actually read.
    async fn build_wake_message(&self, batch: &TeamWakeBatch) -> AppResult<Option<String>> {
        let span = batch.last_message_sequence - batch.first_message_sequence + 1;
        let limit = u32::try_from(span).unwrap_or(u32::MAX);
        let messages: Vec<_> = self
            .managed_team
            .message_repo
            .list_messages_after(&batch.team_id, batch.first_message_sequence - 1, limit)
            .await?
            .into_iter()
            .filter(addressed_to_coordinator)
            .collect();
        if messages.is_empty() {
            return Ok(None);
        }
        let mut lines = Vec::with_capacity(messages.len());
        for message in &messages {
            let sender_name = match message.sender_member_id.as_ref() {
                Some(member_id) => self
                    .managed_team
                    .team_repo
                    .get_member(member_id)
                    .await?
                    .map(|member| member.name),
                None => None,
            };
            lines.push(render_team_origin_message(message, sender_name.as_deref()));
        }
        Ok(Some(format!(
            "Team update ({} message{}).\n{}\n\nContinue coordination from this turn.",
            messages.len(),
            if messages.len() == 1 { "" } else { "s" },
            lines.join("\n")
        )))
    }

    /// Persists one retry attempt on the still-`Running` batch. Returns the
    /// updated batch on success, or `None` when a concurrent settle/cancel
    /// lost the race (the caller must stop retrying, not overwrite it).
    async fn record_retry(
        &self,
        running: &TeamWakeBatch,
        error_message: &str,
    ) -> AppResult<Option<TeamWakeBatch>> {
        let mut retried = running.clone();
        retried.attempt_count += 1;
        retried.last_error = Some(error_message.to_string());
        retried.updated_at = Utc::now();
        let expected_version = running.version;
        retried.version += 1;
        let persisted = self
            .managed_team
            .wake_batch_repo
            .transition(
                &running.id,
                expected_version,
                TeamWakeBatchStatus::Running,
                retried.clone(),
            )
            .await?;
        Ok(persisted.then_some(retried))
    }

    async fn settle_batch(&self, batch: &TeamWakeBatch) -> AppResult<()> {
        let mut settled = batch.clone();
        settled
            .transition_to(TeamWakeBatchStatus::Settled, Utc::now())
            .map_err(AppError::Validation)?;
        let expected_version = batch.version;
        settled.version += 1;
        self.managed_team
            .wake_batch_repo
            .transition(
                &batch.id,
                expected_version,
                TeamWakeBatchStatus::Running,
                settled,
            )
            .await?;
        // The wake send was accepted, so the preallocated coordinator run is
        // now live: move its binding off `Planned` to the status that actually
        // describes it. This is what later lets the run-ended reconciliation in
        // `messaging_delivery.rs` reach `Terminal` (only `Running -> Terminal`
        // is legal) instead of leaving the binding consuming wake budget for
        // the life of the Team. `Running` still satisfies Team authority, so a
        // coordinator woken this way keeps its Team tools.
        self.advance_run_binding_to_running(&batch.team_id, batch.planned_agent_run_id.as_ref())
            .await;
        Ok(())
    }

    /// Best-effort `Planned -> Launching -> Running` advance for a dispatched
    /// wake binding. Failures are non-fatal: the wake itself already
    /// succeeded, and the run-ended reconciliation can still ladder the
    /// binding to a terminal status from `Planned`.
    async fn advance_run_binding_to_running(
        &self,
        team_id: &TeamSessionId,
        planned_agent_run_id: Option<&AgentRunId>,
    ) {
        let Some(run_id) = planned_agent_run_id else {
            return;
        };
        let Ok(Some(binding)) = self
            .managed_team
            .run_binding_repo
            .get_by_agent_run_id(run_id)
            .await
        else {
            return;
        };
        if binding.team_id != *team_id {
            return;
        }
        let expected_version = binding.version;
        let mut running = binding;
        if !advance_binding_status(&mut running, TeamRunBindingStatus::Running, Utc::now()) {
            return;
        }
        running.version = expected_version + 1;
        let binding_id = running.id.clone();
        let _ = self
            .managed_team
            .run_binding_repo
            .transition(&binding_id, expected_version, running)
            .await;
    }

    async fn fail_batch(&self, batch: &TeamWakeBatch, error_message: &str) -> AppResult<()> {
        let expected_status = batch.status;
        let mut failed = batch.clone();
        if failed
            .transition_to(TeamWakeBatchStatus::Failed, Utc::now())
            .is_err()
        {
            // Already terminal (settled/cancelled by a concurrent actor); nothing to fail.
            return Ok(());
        }
        failed.last_error = Some(error_message.to_string());
        let expected_version = batch.version;
        failed.version += 1;
        let persisted = self
            .managed_team
            .wake_batch_repo
            .transition(&batch.id, expected_version, expected_status, failed.clone())
            .await?;
        if !persisted {
            return Ok(());
        }
        self.resolve_run_binding(
            &batch.team_id,
            batch.planned_agent_run_id.as_ref(),
            TeamRunBindingStatus::Failed,
            Some("managed_team_wake_dispatch_failed"),
        )
        .await;
        self.events.emit(
            TEAM_WAKE_FAILED_EVENT,
            serde_json::json!({
                "team_id": batch.team_id.as_str(),
                "wake_batch_id": batch.id.0.as_str(),
                "attempt_count": failed.attempt_count,
                "reason": "wake_dispatch_failed",
                "error": error_message,
            }),
        );
        Ok(())
    }

    /// Cancels a claimed batch (and its preallocated run binding) when the
    /// coordinator is already mid-turn: the settlement message is already
    /// enqueued on the coordinator's normal `QueueKey` and will drain at turn
    /// end, so this wake path is no longer needed for it. Cancelling — rather
    /// than leaving the batch at `Launching` forever — keeps it from
    /// permanently consuming automatic-wake budget.
    async fn cancel_batch(&self, batch: &TeamWakeBatch) {
        let expected_status = batch.status;
        let mut cancelled = batch.clone();
        if cancelled
            .transition_to(TeamWakeBatchStatus::Cancelled, Utc::now())
            .is_err()
        {
            return;
        }
        let expected_version = batch.version;
        cancelled.version += 1;
        let persisted = self
            .managed_team
            .wake_batch_repo
            .transition(&batch.id, expected_version, expected_status, cancelled)
            .await
            .unwrap_or(false);
        if !persisted {
            return;
        }
        self.resolve_run_binding(
            &batch.team_id,
            batch.planned_agent_run_id.as_ref(),
            TeamRunBindingStatus::Cancelled,
            None,
        )
        .await;
    }

    /// Resolves the wake-triggered run binding created by the claim to a
    /// terminal status so it stops consuming automatic-wake budget
    /// (`messaging_delivery.rs` only counts `Planned|Launching|Running`
    /// `WakeBatch` bindings as live).
    async fn resolve_run_binding(
        &self,
        team_id: &TeamSessionId,
        planned_agent_run_id: Option<&AgentRunId>,
        next: TeamRunBindingStatus,
        error_message: Option<&str>,
    ) {
        let Some(run_id) = planned_agent_run_id else {
            return;
        };
        let Ok(Some(binding)) = self
            .managed_team
            .run_binding_repo
            .get_by_agent_run_id(run_id)
            .await
        else {
            return;
        };
        if binding.team_id != *team_id {
            return;
        }
        let expected_version = binding.version;
        let mut resolved = binding;
        if resolved.transition_to(next, Utc::now()).is_err() {
            return;
        }
        resolved.last_error = error_message.map(str::to_string);
        resolved.version = expected_version + 1;
        let _ = self
            .managed_team
            .run_binding_repo
            .transition(&resolved.id.clone(), expected_version, resolved)
            .await;
    }
}
