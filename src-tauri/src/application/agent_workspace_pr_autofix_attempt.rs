use crate::domain::entities::{AgentRun, AgentRunActionKind, AgentRunStatus, ChatConversationId};
use crate::domain::repositories::AgentRunRepository;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrAutofixAttemptDecision {
    StartFirst,
    StartRetry,
    Active,
    RetryExhausted,
    CompletedUnresolved,
    Cancelled,
    LegacyUnbound,
}

impl PrAutofixAttemptDecision {
    pub(crate) fn allows_start(self) -> bool {
        matches!(self, Self::StartFirst | Self::StartRetry)
    }

    pub(crate) fn manual_summary(self) -> Option<&'static str> {
        match self {
            Self::RetryExhausted => Some(
                "PR autofix retry budget is exhausted for the current unresolved issue; manual action is required.",
            ),
            Self::CompletedUnresolved => Some(
                "The PR fixer completed but the same issue is still unresolved; manual action is required.",
            ),
            Self::Cancelled => Some(
                "The PR fixer was cancelled while the same issue remains unresolved; manual action is required.",
            ),
            Self::LegacyUnbound => Some(
                "A legacy PR autofix event has no exact fixer attempt; manual action is required.",
            ),
            _ => None,
        }
    }
}

pub(crate) fn pr_autofix_action_metadata(pr_number: i64, fingerprint: &str) -> String {
    serde_json::json!({
        "ralphx_action_kind": AgentRunActionKind::PrAutofix.to_string(),
        "ralphx_action_context_id": pr_number.to_string(),
        "ralphx_action_target_id": fingerprint,
    })
    .to_string()
}

pub(crate) async fn load_pr_autofix_attempt_decision(
    repo: &dyn AgentRunRepository,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    fingerprint: &str,
    legacy_event_exists: bool,
) -> AppResult<PrAutofixAttemptDecision> {
    let context_id = pr_number.to_string();
    let mut attempts: Vec<AgentRun> = repo
        .get_by_conversation(conversation_id)
        .await?
        .into_iter()
        .filter(|run| {
            run.action_kind == Some(AgentRunActionKind::PrAutofix)
                && run.action_context_id.as_deref() == Some(context_id.as_str())
                && run.action_target_id.as_deref() == Some(fingerprint)
        })
        .collect();
    attempts.sort_by_key(|run| run.started_at);

    if attempts
        .iter()
        .any(|run| run.status == AgentRunStatus::Running)
    {
        return Ok(PrAutofixAttemptDecision::Active);
    }
    let Some(latest) = attempts.last() else {
        return Ok(if legacy_event_exists {
            PrAutofixAttemptDecision::LegacyUnbound
        } else {
            PrAutofixAttemptDecision::StartFirst
        });
    };
    let failed_count = attempts
        .iter()
        .filter(|run| run.status == AgentRunStatus::Failed)
        .count();
    Ok(match latest.status {
        AgentRunStatus::Running => PrAutofixAttemptDecision::Active,
        AgentRunStatus::Failed if failed_count < 2 => PrAutofixAttemptDecision::StartRetry,
        AgentRunStatus::Failed => PrAutofixAttemptDecision::RetryExhausted,
        AgentRunStatus::Completed => PrAutofixAttemptDecision::CompletedUnresolved,
        AgentRunStatus::Cancelled => PrAutofixAttemptDecision::Cancelled,
    })
}
