use std::time::Duration;

use crate::application::chat_service::{
    SendCallerContext, SendMessageOptions, SendQueuePolicy,
};
use crate::domain::entities::{DelegationPark, DelegationParkState, DelegationWakeReason};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::delegation_config;

use super::DelegationParkService;

const PARK_WAKE_FAILED_EVENT: &str = "delegation_park:needs_attention";

impl DelegationParkService {
    /// Claim and enqueue a hidden resume-in-place wake for a parked coordinator.
    ///
    /// # Errors
    ///
    /// Returns repository and read failures; send failures are retried then durably settled.
    pub async fn dispatch_wake(
        &self,
        park: &DelegationPark,
        reason: DelegationWakeReason,
    ) -> AppResult<()> {
        self.dispatch_wake_as(park, reason, DelegationParkState::Woken)
            .await
    }

    pub(super) async fn dispatch_wake_as(
        &self,
        park: &DelegationPark,
        reason: DelegationWakeReason,
        settled_state: DelegationParkState,
    ) -> AppResult<()> {
        if !self.park_repo.claim_wake(&park.id, park.generation).await? {
            return Ok(());
        }

        let active_run = match self
            .agent_run_repo
            .get_active_for_conversation(&park.parent_conversation_id)
            .await
        {
            Ok(run) => run,
            Err(error) => {
                self.settle_failed_wake(park, &error).await?;
                return Err(error);
            }
        };
        if active_run.is_some_and(|run| run.id != park.parent_agent_run_id) {
            self.park_repo
                .settle(&park.id, DelegationParkState::Superseded, None)
                .await?;
            return Ok(());
        }

        let (conversation, content, options) = match self.prepare_wake(park, reason).await {
            Ok(prepared) => prepared,
            Err(error) => {
                self.settle_failed_wake(park, &error).await?;
                return Err(error);
            }
        };

        let config = delegation_config();
        let retry_max = config.park_wake_retry_max.max(1);
        loop {
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
                    self.park_repo.settle(&park.id, settled_state, None).await?;
                    return Ok(());
                }
                Err(error) => {
                    let error_message = error.to_string();
                    let durable_count = self
                        .park_repo
                        .record_wake_failure(&park.id, &error_message)
                        .await?;
                    if i64::from(durable_count) >= i64::from(retry_max) {
                        self.settle_failed_wake(park, &AppError::Agent(error_message))
                            .await?;
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_secs(config.park_wake_retry_backoff_secs))
                        .await;
                }
            }
        }
    }

    async fn prepare_wake(
        &self,
        park: &DelegationPark,
        reason: DelegationWakeReason,
    ) -> AppResult<(
        crate::domain::entities::ChatConversation,
        String,
        SendMessageOptions,
    )> {
        let conversation = self
            .conversation_repo
            .get_by_id(&park.parent_conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "parent conversation not found: {}",
                    park.parent_conversation_id
                ))
            })?;
        let latest_run = self
            .agent_run_repo
            .get_latest_for_conversation(&park.parent_conversation_id)
            .await?;
        let content = self.build_wake_message(park, reason).await?;
        let options = SendMessageOptions {
            // `persist_hidden_marker` selects chat-service's canonical MessageRole::System
            // persistence for hidden resume-in-place markers.
            metadata: Some(
                serde_json::json!({
                    "source": "delegation_park",
                    "resume_in_place": true,
                    "persist_hidden_marker": true,
                    "hidden_from_ui": true,
                    "recovery_context": true,
                    "wake_reason": format!("{reason:?}").to_lowercase(),
                })
                .to_string(),
            ),
            queue_policy: SendQueuePolicy::AllowQueue,
            conversation_id_override: Some(park.parent_conversation_id.clone()),
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
        Ok((conversation, content, options))
    }

    async fn settle_failed_wake(&self, park: &DelegationPark, error: &AppError) -> AppResult<()> {
        let error_message = error.to_string();
        self.park_repo
            .settle(&park.id, DelegationParkState::Failed, Some(&error_message))
            .await?;
        // Best-effort identity so the UI can name the coordinator that will never resume. This
        // runs on the failure path, where an unreadable conversation may be the failure itself,
        // so a missing lookup must degrade the payload rather than suppress the alert.
        let conversation = self
            .conversation_repo
            .get_by_id(&park.parent_conversation_id)
            .await
            .ok()
            .flatten();
        self.events.emit(
            PARK_WAKE_FAILED_EVENT,
            serde_json::json!({
                "park_id": park.id.as_str(),
                "parent_conversation_id": park.parent_conversation_id.as_str(),
                "conversation_title": conversation.as_ref().and_then(|c| c.title.clone()),
                "context_type": conversation.as_ref().map(|c| c.context_type.to_string()),
                "context_id": conversation.as_ref().map(|c| c.context_id.clone()),
                "delegate_count": park.jobs.len(),
                "error": error_message,
            }),
        );
        Ok(())
    }

    async fn build_wake_message(
        &self,
        park: &DelegationPark,
        reason: DelegationWakeReason,
    ) -> AppResult<String> {
        let mut lines = Vec::with_capacity(park.jobs.len());
        let mut unsettled = Vec::new();
        for job in &park.jobs {
            let run = self
                .agent_run_repo
                .get_by_id(&job.delegated_agent_run_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "delegated run not found: {}",
                        job.delegated_agent_run_id
                    ))
                })?;
            let agent_name = run.agent_name.unwrap_or(job.delegated_session_id.clone());
            let status = job
                .settled_status
                .clone()
                .unwrap_or_else(|| run.status.to_string());
            if job.settled_status.is_none() {
                unsettled.push(agent_name.clone());
            }
            let preview = run
                .error_message
                .as_deref()
                .map(short_preview)
                .unwrap_or_else(|| "no result preview available".to_string());
            lines.push(format!("- {agent_name}: {status} — {preview}"));
        }

        let mut body = format!(
            "Delegated work update ({reason:?}).\n{}\n\nContinue coordination from this turn.",
            lines.join("\n")
        );
        // A deadline wake can still find every job settled — `reconcile_park` checks expiry before
        // the wake decision. Claiming a timeout there would contradict the settled job list above.
        if matches!(reason, DelegationWakeReason::Deadline) && !unsettled.is_empty() {
            body.push_str(&format!(
                "\n\nTimeout notice: these jobs never settled: {}.",
                unsettled.join(", ")
            ));
        }
        Ok(body)
    }
}

fn short_preview(value: &str) -> String {
    const PREVIEW_CHARS: usize = 240;
    let mut preview = value.chars().take(PREVIEW_CHARS).collect::<String>();
    if value.chars().count() > PREVIEW_CHARS {
        preview.push('…');
    }
    preview
}
