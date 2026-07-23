use std::str::FromStr;

use crate::domain::entities::{
    AgentRun, AgentRunActionKind, AgentRunId, AgentRunStatus, ChatConversationId,
};
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

/// Completion authority is deliberately narrower than retry eligibility: a caller may settle a
/// PR autofix claim only while it is the current running run for the exact PR/fingerprint tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrAutofixCompletionAuthority {
    Current,
    Superseded,
    AlreadyCompleted,
    Invalid,
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

/// AgentRun action metadata is the authority for a live PR-autofix attempt. The fingerprint
/// repeats the PR number so malformed or cross-PR tuples cannot become current by ordering alone.
pub(crate) fn is_exact_pr_autofix_run_for_pr(run: &AgentRun, pr_number: i64) -> bool {
    let context_id = pr_number.to_string();
    let fingerprint_prefix = format!("github_pr_autofix:{context_id}:");
    run.action_kind == Some(AgentRunActionKind::PrAutofix)
        && run.action_context_id.as_deref() == Some(context_id.as_str())
        && run
            .action_target_id
            .as_deref()
            .and_then(|fingerprint| fingerprint.strip_prefix(&fingerprint_prefix))
            .is_some_and(|target| !target.is_empty())
}

pub(crate) async fn load_latest_exact_pr_autofix_run_for_pr(
    repo: &dyn AgentRunRepository,
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> AppResult<Option<AgentRun>> {
    let mut runs: Vec<AgentRun> = repo
        .get_by_conversation(conversation_id)
        .await?
        .into_iter()
        .filter(|run| is_exact_pr_autofix_run_for_pr(run, pr_number))
        .collect();
    runs.sort_by(|left, right| {
        left.started_at
            .cmp(&right.started_at)
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    });
    Ok(runs.pop())
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

pub(crate) async fn load_pr_autofix_completion_authority(
    repo: &dyn AgentRunRepository,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    created_by_run_id: Option<&str>,
) -> AppResult<PrAutofixCompletionAuthority> {
    let Some(created_by_run_id) = created_by_run_id else {
        return Ok(PrAutofixCompletionAuthority::Invalid);
    };
    let Ok(caller_id) = AgentRunId::from_str(created_by_run_id) else {
        return Ok(PrAutofixCompletionAuthority::Invalid);
    };
    let Some(caller) = repo.get_by_id(&caller_id).await? else {
        return Ok(PrAutofixCompletionAuthority::Invalid);
    };
    let context_id = pr_number.to_string();
    if caller.conversation_id != *conversation_id
        || caller.action_kind != Some(AgentRunActionKind::PrAutofix)
        || caller.action_context_id.as_deref() != Some(context_id.as_str())
        || caller.action_target_id.as_deref().is_none_or(str::is_empty)
    {
        return Ok(PrAutofixCompletionAuthority::Invalid);
    }
    let fingerprint = caller.action_target_id.as_deref().expect("checked above");
    let attempts: Vec<AgentRun> = repo
        .get_by_conversation(conversation_id)
        .await?
        .into_iter()
        .filter(|run| {
            run.action_kind == Some(AgentRunActionKind::PrAutofix)
                && run.action_context_id.as_deref() == Some(context_id.as_str())
                && run.action_target_id.as_deref() == Some(fingerprint)
        })
        .collect();
    let active_attempt = attempts
        .iter()
        .filter(|run| run.status == AgentRunStatus::Running)
        .max_by_key(|run| run.started_at);
    if active_attempt.is_some_and(|active| active.id != caller.id) {
        return Ok(PrAutofixCompletionAuthority::Superseded);
    }
    Ok(match caller.status {
        AgentRunStatus::Running if active_attempt.is_some() => {
            PrAutofixCompletionAuthority::Current
        }
        AgentRunStatus::Running => PrAutofixCompletionAuthority::Invalid,
        AgentRunStatus::Completed => PrAutofixCompletionAuthority::AlreadyCompleted,
        AgentRunStatus::Failed | AgentRunStatus::Cancelled => PrAutofixCompletionAuthority::Invalid,
    })
}
