//! Shared trusted-caller run authority for MCP transport-owned identity.
//!
//! Authority is the caller run's own liveness, never its recency relative to sibling rows:
//! `get_active_for_conversation` returns the newest `running` row, which can be an orphan from a
//! killed process (repaired only at next boot) or can outrank a live run whose `started_at` was
//! never refreshed on interactive turn reuse.

use std::sync::Arc;

use crate::domain::entities::{AgentRun, AgentRunId, AgentRunStatus, ChatConversationId};
use crate::domain::repositories::AgentRunRepository;

/// Why a transport-supplied caller run was refused authority.
#[derive(Debug)]
pub enum TrustedRunRejection {
    /// No run row exists for the transport-supplied id.
    RunNotFound,
    /// The run exists but belongs to a different conversation.
    ConversationMismatch,
    /// The run exists and is bound correctly, but its turn has already finished.
    RunTerminal { status: AgentRunStatus },
    /// The run could not be read. Fails closed: never resolves to "authorized".
    RepositoryError(String),
}

/// Resolve the transport-supplied caller run, requiring only that it is live and correctly bound.
///
/// Guard order is load-bearing and must not be reordered:
/// 1. read failure fails closed, missing row is `RunNotFound`;
/// 2. conversation binding — the nested-delegation guard, with no lineage/ancestor fallback;
/// 3. terminal status.
///
/// `AgentRunStatus` has exactly four variants, so `!is_terminal()` is precisely `Running`: this
/// accepts live runs and nothing else.
///
/// # Errors
///
/// Returns [`TrustedRunRejection`] when the run is missing, bound to another conversation, already
/// terminal, or unreadable.
pub async fn resolve_live_caller_run(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
) -> Result<AgentRun, TrustedRunRejection> {
    let run = agent_run_repo
        .get_by_id(run_id)
        .await
        .map_err(|error| TrustedRunRejection::RepositoryError(error.to_string()))?
        .ok_or(TrustedRunRejection::RunNotFound)?;

    if run.conversation_id != *conversation_id {
        return Err(TrustedRunRejection::ConversationMismatch);
    }

    if run.status.is_terminal() {
        return Err(TrustedRunRejection::RunTerminal { status: run.status });
    }

    Ok(run)
}
