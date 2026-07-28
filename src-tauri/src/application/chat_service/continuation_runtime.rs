use std::sync::Arc;

use crate::application::agent_lane_resolution::ResolvedAgentSpawnSettings;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{AgentRun, ChatConversation, ChatConversationId};
use crate::domain::repositories::AgentRunRepository;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContinuationRuntime {
    pub harness: AgentHarnessKind,
    pub provider_session_id: String,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
    pub service_tier: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RuntimeOverridePresence {
    pub model: bool,
    pub logical_effort: bool,
    pub service_tier: bool,
    pub approval_policy: bool,
    pub sandbox_mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModelIdentityComparison {
    Same,
    Changed,
    Unknown,
}

impl ContinuationRuntime {
    fn from_run(run: AgentRun, harness: AgentHarnessKind, provider_session_id: &str) -> Self {
        Self {
            harness,
            provider_session_id: provider_session_id.to_string(),
            logical_model: run.logical_model,
            effective_model_id: run.effective_model_id,
            logical_effort: run.logical_effort,
            service_tier: run.service_tier,
            approval_policy: run.approval_policy,
            sandbox_mode: run.sandbox_mode,
        }
    }

    pub(super) fn effective_model(&self) -> Option<&str> {
        self.effective_model_id
            .as_deref()
            .or(self.logical_model.as_deref())
    }

    pub(super) fn compare_model_identity(&self, requested_model: &str) -> ModelIdentityComparison {
        let requested_model = normalize_model_identity(requested_model);
        let known_models = [
            self.logical_model.as_deref(),
            self.effective_model_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|model| !model.trim().is_empty())
        .collect::<Vec<_>>();

        if known_models.is_empty() {
            return ModelIdentityComparison::Unknown;
        }

        if known_models
            .into_iter()
            .any(|model| normalize_model_identity(model) == requested_model)
        {
            ModelIdentityComparison::Same
        } else {
            ModelIdentityComparison::Changed
        }
    }

    pub(super) fn apply_defaults(
        &self,
        resolved: &mut ResolvedAgentSpawnSettings,
        overrides: RuntimeOverridePresence,
    ) {
        if resolved.effective_harness != self.harness {
            return;
        }

        if !overrides.model {
            if let Some(model) = self.effective_model() {
                resolved.configured_model = self.logical_model.clone();
                resolved.model = model.to_string();
            }
        }
        if !overrides.logical_effort {
            resolved.configured_logical_effort = self.logical_effort;
            resolved.logical_effort = self.logical_effort;
            resolved.claude_effort = self
                .logical_effort
                .map(|effort| effort.to_legacy_claude_effort().to_string());
        }
        if !overrides.service_tier {
            resolved.configured_service_tier = self.service_tier.clone();
            resolved.service_tier = self.service_tier.clone();
        }
        if !overrides.approval_policy {
            resolved.configured_approval_policy = self.approval_policy.clone();
            resolved.approval_policy = self.approval_policy.clone();
        }
        if !overrides.sandbox_mode {
            resolved.configured_sandbox_mode = self.sandbox_mode.clone();
            resolved.sandbox_mode = self.sandbox_mode.clone();
        }
    }
}

fn normalize_model_identity(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

pub(super) async fn resolve_for_conversation(
    repository: &Arc<dyn AgentRunRepository>,
    conversation: &ChatConversation,
) -> AppResult<Option<ContinuationRuntime>> {
    let Some(session_ref) = conversation.provider_session_ref() else {
        return Ok(None);
    };
    resolve_for_provider_session(
        repository,
        &conversation.id,
        session_ref.harness,
        &session_ref.provider_session_id,
    )
    .await
}

pub(super) async fn resolve_for_provider_session(
    repository: &Arc<dyn AgentRunRepository>,
    conversation_id: &ChatConversationId,
    harness: AgentHarnessKind,
    provider_session_id: &str,
) -> AppResult<Option<ContinuationRuntime>> {
    Ok(repository
        .get_latest_completed_for_provider_session(conversation_id, harness, provider_session_id)
        .await?
        .map(|run| ContinuationRuntime::from_run(run, harness, provider_session_id)))
}
