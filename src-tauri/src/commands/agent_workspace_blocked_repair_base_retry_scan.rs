//! Blocked-repair base-retry half of the periodic agent-workspace scan.
//!
//! This lane moves an already-shipped trigger from selection time to timer time. Today a stuck
//! published workspace only recovers when the user opens its conversation: the
//! `AgentsPublishPanel` auto-effect fires `update_from_base` whenever
//! `shouldAutoRefreshCleanAgentWorkspaceFromBase` holds, which reaches the retry-blocked branch of
//! `update_agent_conversation_workspace_from_base_for_app_state` and supersedes the blocked repair
//! generation. That "explicit user action" path is in fact an automation keyed to conversation
//! selection, so a workspace whose base moved while nobody was looking stays blocked indefinitely.
//!
//! The conditions here mirror that frontend gate — Edit mode, non-blocked base, base ahead, clean
//! worktree, zero unpublished commits — plus three backend-owned ones: the automation opt-in, the
//! idle gate, and one retry per base tip (the durable equivalent of the frontend's per-session
//! dedupe key). Admission itself is the same `explicit_agent_workspace_repair_retry_allowed` call
//! the UI path makes, so this grants no authority the product does not already exercise on every
//! conversation selection; it only adds reachability.
//!
//! It ships beside `agent_workspace_repair_reconciliation_scan` rather than inside it because that
//! file is already at the module size limit; the tick loop there owns all three halves.

use std::collections::HashSet;
use std::sync::Arc;

use tauri::{Emitter, Manager, Runtime};

use crate::application::agent_conversation_workspace::AgentConversationWorkspaceBaseSelection;
use crate::application::agent_conversation_workspace_base::{resolve_workspace_base, BaseStatus};
use crate::application::agent_workspace_publish_repair_state::explicit_agent_workspace_repair_retry_allowed;
use crate::application::git_service::{git_cmd, GitService};
use crate::application::publish_resilience::{
    count_publishable_commits_with_base_fallback,
    inspect_publish_branch_freshness_for_source_after_fetch,
};
use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    resolve_agent_workspace_publish_target,
    update_agent_conversation_workspace_from_base_for_app_state,
    AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus, AgentWorkspaceRepairPhase,
    ChatConversationId,
};

pub(crate) async fn run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle<R>(
    app_handle: &tauri::AppHandle<R>,
) -> Result<usize, String>
where
    R: Runtime,
{
    let state = app_handle
        .try_state::<AppState>()
        .ok_or_else(|| "AppState is not available".to_string())?;
    if state.startup_git_auth_recovery_state.is_pending() {
        return Ok(0);
    }
    let execution_state = app_handle
        .try_state::<Arc<ExecutionState>>()
        .ok_or_else(|| "ExecutionState is not available".to_string())?
        .inner()
        .clone();
    run_agent_workspace_blocked_repair_base_retry_scan_tick_for_state(
        state.inner(),
        &execution_state,
        Some(app_handle),
    )
    .await
}

/// Fail-closed candidate listing, mirroring the other two scan halves: a repo error aborts the
/// whole tick (the caller logs a warning and the next tick retries); every per-candidate failure
/// skips only that candidate.
///
/// The listing returns one row per *unsettled attempt*, so a conversation can appear more than
/// once. Candidates are deduped before any git work — otherwise a single conversation would be
/// fetched twice in one tick and could be superseded twice.
pub(crate) async fn run_agent_workspace_blocked_repair_base_retry_scan_tick_for_state<R>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> Result<usize, String>
where
    R: Runtime,
{
    let recoverable_attempts = state
        .agent_workspace_repair_repo
        .list_recoverable_repair_attempts()
        .await
        .map_err(|error| error.to_string())?;

    let mut seen: HashSet<ChatConversationId> = HashSet::new();
    let mut candidates: Vec<ChatConversationId> = Vec::new();
    for attempt in recoverable_attempts {
        // Cheap pre-filter only. The authoritative admission check runs per candidate below, and
        // the seam re-reads the current attempt itself, so this row is never the dispatch subject.
        if attempt.phase != AgentWorkspaceRepairPhase::Blocked || attempt.next_dispatch_at.is_some()
        {
            continue;
        }
        if seen.insert(attempt.conversation_id.clone()) {
            candidates.push(attempt.conversation_id);
        }
    }

    let mut dispatched = 0usize;
    for conversation_id in candidates {
        let result = git_cmd::with_git_command_lane(git_cmd::GitCommandLane::Background, async {
            process_blocked_repair_base_retry_candidate(state, execution_state, &conversation_id)
                .await
        })
        .await;
        match result {
            Ok(true) => {
                dispatched += 1;
                if let Some(app_handle) = app_handle {
                    // The `_for_app_state` seam does not emit this; only the Tauri command wrapper
                    // does. Without it an unwatched conversation stays visually stale until the
                    // next poll.
                    let _ = app_handle.emit(
                        "agent:workspace_changed",
                        serde_json::json!({ "conversation_id": conversation_id.as_str() }),
                    );
                }
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    error = %error,
                    "Blocked-repair base-retry scan failed for a candidate workspace"
                );
            }
        }
    }

    Ok(dispatched)
}

/// Evaluates one candidate and, when every gate holds, re-drives the shared update-from-base seam.
/// Returns `Ok(true)` when a repair successor was actually dispatched, `Ok(false)` for every
/// benign skip, and `Err` only for genuine failures (unreadable repo state, fetch/inspection
/// errors) that the caller logs without failing the tick.
///
/// Every ambiguous read fails closed toward "no dispatch": an unattended timer that guesses wrong
/// spends agent budget on a workspace nobody asked it to touch.
async fn process_blocked_repair_base_retry_candidate(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: &ChatConversationId,
) -> Result<bool, String> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };

    // Edit mode is checked before anything resolves a publish target: for an Ideation workspace
    // with a linked plan branch, `resolve_agent_workspace_publish_target` calls
    // `ensure_linked_plan_branch_agent_worktree`, which *creates a worktree*. An unattended timer
    // must never materialize one.
    if workspace.mode != AgentConversationWorkspaceMode::Edit
        || workspace.status != AgentConversationWorkspaceStatus::Active
    {
        return Ok(false);
    }

    // Consent gate, consistent with the existing unattended base-update lane. Non-opted-in
    // workspaces keep today's selection-triggered behavior.
    if !(workspace.auto_publish_enabled || workspace.auto_publish_initial_pr_enabled) {
        return Ok(false);
    }

    let Some(attempt) = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    // The same backend-owned admission the UI retry path uses. It also applies the open-effect
    // fence, so an attempt with durable work in flight is never superseded from here.
    if !explicit_agent_workspace_repair_retry_allowed(
        state.agent_workspace_repair_repo.as_ref(),
        &attempt,
    )
    .await
    .map_err(|error| error.to_string())?
    {
        return Ok(false);
    }
    // Without a recorded target base commit, base motion cannot be proven for this attempt, so
    // there is no evidence that a retry would behave differently than the blocked run. Leave it to
    // the selection lane.
    let Some(recorded_target_base_commit) = attempt.target_base_commit.clone() else {
        return Ok(false);
    };

    if state
        .agent_run_repo
        .get_active_for_conversation(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(false);
    }

    // The reviewer runs in a child conversation, so the active-run gate above cannot see it. This
    // retry merges from base into the same worktree, so it must stand down for review-owned work.
    match crate::application::agent_workspace_review::workspace_review_owns_worktree(
        state.agent_conversation_workspace_repo.as_ref(),
        conversation_id,
    )
    .await
    {
        Ok(true) => return Ok(false),
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Skipping blocked repair base retry: Workspace Review gate failed closed"
            );
            return Ok(false);
        }
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;

    let base_resolution = resolve_workspace_base(&project, &workspace)
        .await
        .map_err(|error| error.to_string())?;
    if base_resolution.status == BaseStatus::Blocked {
        // A blocked base is not stale (mirrors `AutoPublishSkipReason::BaseBlocked` semantics).
        return Ok(false);
    }
    let effective_base_ref = base_resolution
        .effective_checkout_ref()
        .map_err(|error| error.to_string())?
        .to_string();

    let publish_target =
        match resolve_agent_workspace_publish_target(state, &project, &workspace).await {
            Ok(target) => target,
            Err(reason) => {
                // Worktree missing or checked out on another branch is a structurally normal,
                // often persistent state during repair — not a per-tick failure.
                tracing::debug!(
                    conversation_id = conversation_id.as_str(),
                    reason = %reason,
                    "Blocked-repair base-retry candidate is not currently inspectable"
                );
                return Ok(false);
            }
        };

    let freshness = inspect_publish_branch_freshness_for_source_after_fetch(
        &publish_target.worktree_path,
        &effective_base_ref,
        &publish_target.branch_name,
        workspace.base_commit.as_deref(),
    )
    .await
    .map_err(|error| error.to_string())?;

    // One retry per base tip. A tip this attempt already targets is not new evidence and must not
    // authorize another supersede, which is what keeps an opted-in workspace off a retry storm.
    if !freshness.is_base_ahead || recorded_target_base_commit == freshness.target_base_commit {
        return Ok(false);
    }

    // The exact cleanliness pair the full-freshness command runs, so this lane cannot re-drive a
    // workspace the UI gate would have refused.
    let (has_uncommitted_changes, unpublished_commit_count) = tokio::join!(
        GitService::has_uncommitted_changes(&publish_target.worktree_path),
        count_publishable_commits_with_base_fallback(
            &publish_target.worktree_path,
            &publish_target.branch_name,
            &effective_base_ref,
        ),
    );
    let has_uncommitted_changes = has_uncommitted_changes.map_err(|error| error.to_string())?;
    let unpublished_commit_count = unpublished_commit_count.map_err(|error| error.to_string())?;
    if has_uncommitted_changes || unpublished_commit_count != 0 {
        return Ok(false);
    }

    dispatch_blocked_repair_base_retry(state, execution_state, conversation_id).await
}

/// Re-drives the shared update-from-base seam with an **empty** base selection.
///
/// The all-`None` selection is load-bearing: `normalize_explicit_publish_base_selection` returns
/// `None` only for an absent base ref, and the retry-blocked branch requires
/// `explicit_base.is_none() && created_by_run_id.is_none()`. A populated selection —
/// `AgentConversationWorkspaceBaseSelection::for_workspace_reuse` in particular — normalizes into
/// an explicit re-target and skips that branch entirely. This is exactly the shape the UI invoke
/// sends.
async fn dispatch_blocked_repair_base_retry(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: &ChatConversationId,
) -> Result<bool, String> {
    match update_agent_conversation_workspace_from_base_for_app_state(
        state,
        execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection::default(),
    )
    .await
    {
        // `repair_started` is the seam's own signal that the retry-blocked branch fired. Any other
        // success means the seam took a different route and this lane did not re-drive a repair.
        Ok(response) => Ok(response.repair_started),
        Err(error) if error == AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE => {
            // Benign: the third concurrency layer after recovery-scheduling dedupe and the
            // repair-attempt CAS fence. The next tick retries.
            tracing::debug!(
                conversation_id = conversation_id.as_str(),
                "Skipped unattended blocked-repair base retry: publish guard busy"
            );
            Ok(false)
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Unattended blocked-repair base retry failed"
            );
            Ok(false)
        }
    }
}
