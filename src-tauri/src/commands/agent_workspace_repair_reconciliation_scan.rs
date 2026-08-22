use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tauri::{Emitter, Manager, Runtime};

use crate::application::agent_conversation_workspace::AgentConversationWorkspaceBaseSelection;
use crate::application::agent_conversation_workspace_base::{resolve_workspace_base, BaseStatus};
use crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrSupervisionRecoveryTrigger;
use crate::application::git_service::git_cmd;
use crate::application::publish_resilience::inspect_publish_branch_freshness_for_source_after_fetch;
use crate::application::AppState;
use crate::commands::agent_workspace_blocked_repair_base_retry_scan::run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle;
use crate::commands::unified_chat_commands::{
    resolve_agent_workspace_publish_target, schedule_pr_supervision_recovery_for_workspace,
    update_agent_conversation_workspace_from_base_for_app_state,
    AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, ChatConversationId,
};
use crate::infrastructure::agents::claude::git_runtime_config;

/// Register the periodic durable repair-reconciliation scan. The scan is a clock only: it
/// schedules the existing recovery/reconciler seams (`schedule_pr_supervision_recovery_for_workspace`)
/// and reuses their claim/dedupe TTL, so this never introduces a second reconciliation engine.
pub(crate) fn start_agent_workspace_repair_reconciliation_scan<R>(app_handle: tauri::AppHandle<R>)
where
    R: Runtime,
{
    tauri::async_runtime::spawn(async move {
        let interval_secs =
            git_runtime_config().agent_workspace_repair_reconciliation_scan_interval_secs;
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs.max(1)));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            match run_agent_workspace_repair_reconciliation_scan_tick_from_app_handle(&app_handle)
                .await
            {
                Ok(0) => {}
                Ok(count) => {
                    tracing::info!(
                        count,
                        "Agent workspace repair reconciliation scan scheduled recovery for candidates"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Agent workspace repair reconciliation scan tick failed"
                    );
                }
            }
            match run_agent_workspace_base_freshness_scan_tick_from_app_handle(&app_handle).await {
                Ok(0) => {}
                Ok(count) => {
                    tracing::info!(
                        count,
                        "Agent workspace base-freshness scan updated candidates from base"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Agent workspace base-freshness scan tick failed"
                    );
                }
            }
            match run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(
                &app_handle,
            )
            .await
            {
                Ok(0) => {}
                Ok(count) => {
                    tracing::info!(
                        count,
                        "Blocked-repair base-retry scan re-drove candidates from base"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Blocked-repair base-retry scan tick failed"
                    );
                }
            }
        }
    });
}

pub(crate) async fn run_agent_workspace_repair_reconciliation_scan_tick_from_app_handle<R>(
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
    run_agent_workspace_repair_reconciliation_scan_tick_for_state(state.inner(), &execution_state)
        .await
}

/// Fail-closed candidate listing: a repo error aborts the whole tick (the caller logs a warning
/// and the next tick retries); a per-workspace load error skips only that workspace.
pub(crate) async fn run_agent_workspace_repair_reconciliation_scan_tick_for_state(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Result<usize, String> {
    let mut conversation_ids: HashSet<ChatConversationId> = HashSet::new();

    let recoverable_attempts = state
        .agent_workspace_repair_repo
        .list_recoverable_repair_attempts()
        .await
        .map_err(|error| error.to_string())?;
    conversation_ids.extend(
        recoverable_attempts
            .into_iter()
            .map(|attempt| attempt.conversation_id),
    );

    let needs_agent_workspaces = state
        .agent_conversation_workspace_repo
        .list_active_needs_agent_workspaces()
        .await
        .map_err(|error| error.to_string())?;
    conversation_ids.extend(
        needs_agent_workspaces
            .into_iter()
            .map(|workspace| workspace.conversation_id),
    );

    let mut scheduled = 0usize;
    for conversation_id in conversation_ids {
        let workspace = match state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
        {
            Ok(Some(workspace)) => workspace,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    error = %error,
                    "Agent workspace repair reconciliation scan could not load a candidate workspace"
                );
                continue;
            }
        };
        if workspace.status != AgentConversationWorkspaceStatus::Active {
            continue;
        }

        // Unconditional: after Phase B, `schedule_pr_supervision_recovery_for_workspace` itself
        // routes GitHub projects through full PR supervision and non-GitHub projects through the
        // durable-only reconciler. Re-branching on `github_service` here would duplicate that
        // routing.
        schedule_pr_supervision_recovery_for_workspace(
            state,
            execution_state,
            &workspace,
            AgentWorkspacePrSupervisionRecoveryTrigger::PeriodicScan,
            false,
        );
        scheduled += 1;
    }

    Ok(scheduled)
}

/// Base-freshness half of the periodic scan (F6): detects base-ahead drift for active,
/// unpublished Edit workspaces and — for opted-in, idle, repair-settled workspaces with a
/// non-blocked base — re-drives the same `update_agent_conversation_workspace_from_base_for_app_state`
/// seam the selection-driven "Update from base" action uses. This never introduces a parallel
/// update path; it only adds invocation frequency to an existing idempotent seam.
pub(crate) async fn run_agent_workspace_base_freshness_scan_tick_from_app_handle<R>(
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
    run_agent_workspace_base_freshness_scan_tick_for_state(
        state.inner(),
        &execution_state,
        Some(app_handle),
    )
    .await
}

/// Fail-closed candidate listing, mirroring the repair-reconciliation tick: a repo error aborts
/// the whole tick (the caller logs a warning and the next tick retries); a per-workspace error
/// skips only that workspace and leaves its persisted `stale_base_detected_at` untouched.
pub(crate) async fn run_agent_workspace_base_freshness_scan_tick_for_state<R>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> Result<usize, String>
where
    R: Runtime,
{
    let candidates = state
        .agent_conversation_workspace_repo
        .list_active_unpublished_edit_workspaces()
        .await
        .map_err(|error| error.to_string())?;

    let mut updated = 0usize;
    for workspace in candidates {
        if workspace.status != AgentConversationWorkspaceStatus::Active {
            continue;
        }
        let conversation_id = workspace.conversation_id.clone();
        let result = git_cmd::with_git_command_lane(git_cmd::GitCommandLane::Background, async {
            process_unpublished_workspace_base_freshness(
                state,
                execution_state,
                app_handle,
                workspace,
            )
            .await
        })
        .await;
        match result {
            Ok(BaseFreshnessCandidateOutcome::Updated(true)) => updated += 1,
            Ok(BaseFreshnessCandidateOutcome::Updated(false)) => {}
            Ok(BaseFreshnessCandidateOutcome::NotInspectable(reason)) => {
                // Worktree missing or checked out on another branch is a normal, often
                // persistent state during repair/merge — not a genuine per-tick failure. Logging
                // it at `warn` forever would drown the real failures this scan needs to surface.
                tracing::debug!(
                    conversation_id = conversation_id.as_str(),
                    reason = %reason,
                    "Agent workspace base-freshness scan candidate is not currently inspectable"
                );
            }
            Err(error) => {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    error = %error,
                    "Agent workspace base-freshness scan failed for a candidate workspace"
                );
            }
        }
    }

    Ok(updated)
}

/// Outcome of evaluating one base-freshness candidate. `NotInspectable` covers the structurally
/// normal case (worktree missing or checked out on another branch) so the caller can log it at
/// `debug` instead of `warn`; every candidate from `list_active_unpublished_edit_workspaces` is
/// Edit-mode and non-execution-owned, so `resolve_agent_workspace_publish_target` can only fail
/// there via `resolve_valid_agent_conversation_workspace_path`.
enum BaseFreshnessCandidateOutcome {
    Updated(bool),
    NotInspectable(String),
}

/// Detects base-ahead drift for one unpublished Edit workspace, persists the transition, and —
/// only when the base is currently ahead — attempts a gated auto-update. Returns
/// `Ok(Updated(true))` when an auto-update was performed, `Ok(Updated(false))` when detection ran
/// but no update was performed or needed, `Ok(NotInspectable(_))` when the workspace path/branch
/// could not be resolved, and `Err` only for genuine detection failures (missing project,
/// blocked-base resolution errors, freshness inspection failures).
async fn process_unpublished_workspace_base_freshness<R>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: Option<&tauri::AppHandle<R>>,
    workspace: AgentConversationWorkspace,
) -> Result<BaseFreshnessCandidateOutcome, String>
where
    R: Runtime,
{
    let conversation_id = workspace.conversation_id.clone();
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
        // Blocked base is not stale (mirrors AutoPublishSkipReason::BaseBlocked semantics);
        // leave any previously persisted `stale_base_detected_at` untouched.
        return Ok(BaseFreshnessCandidateOutcome::Updated(false));
    }
    let effective_base_ref = base_resolution
        .effective_checkout_ref()
        .map_err(|error| error.to_string())?
        .to_string();

    let publish_target =
        match resolve_agent_workspace_publish_target(state, &project, &workspace).await {
            Ok(target) => target,
            Err(reason) => return Ok(BaseFreshnessCandidateOutcome::NotInspectable(reason)),
        };
    let freshness = inspect_publish_branch_freshness_for_source_after_fetch(
        &publish_target.worktree_path,
        &effective_base_ref,
        &publish_target.branch_name,
        workspace.base_commit.as_deref(),
    )
    .await
    .map_err(|error| error.to_string())?;

    let Some(workspace) = persist_stale_base_detected_at_transition(
        state,
        app_handle,
        workspace,
        freshness.is_base_ahead,
    )
    .await?
    else {
        // The workspace was deleted between listing and the transition write; nothing left to
        // auto-update.
        return Ok(BaseFreshnessCandidateOutcome::Updated(false));
    };

    if !freshness.is_base_ahead {
        return Ok(BaseFreshnessCandidateOutcome::Updated(false));
    }

    Ok(BaseFreshnessCandidateOutcome::Updated(
        attempt_gated_agent_workspace_base_auto_update(
            state,
            execution_state,
            &conversation_id,
            &workspace,
        )
        .await,
    ))
}

/// Persists `stale_base_detected_at` only on a detection transition (ahead → set, current →
/// clear) via a targeted column setter — never a whole-row upsert, so a concurrent targeted
/// write elsewhere (publication linkage, toggles, status) is never reverted by this unattended
/// timer. Emits `agent:workspace_changed` only when a transition actually happened, so unwatched
/// conversations in the Agents sidebar (Phase D) and unselected-conversation polling stay quiet
/// on steady-state ticks. Returns a freshly re-read workspace (not the pre-fetch snapshot) so the
/// auto-update gate evaluates current state; returns `Ok(None)` if the workspace was deleted
/// between listing and this write, which the caller treats as a benign skip.
pub(crate) async fn persist_stale_base_detected_at_transition<R>(
    state: &AppState,
    app_handle: Option<&tauri::AppHandle<R>>,
    workspace: AgentConversationWorkspace,
    is_base_ahead: bool,
) -> Result<Option<AgentConversationWorkspace>, String>
where
    R: Runtime,
{
    let next_value = match (workspace.stale_base_detected_at.is_some(), is_base_ahead) {
        (false, true) => Some(Utc::now()),
        (true, false) => None,
        _ => return Ok(Some(workspace)),
    };

    let conversation_id = workspace.conversation_id.clone();
    state
        .agent_conversation_workspace_repo
        .set_stale_base_detected_at(&conversation_id, next_value)
        .await
        .map_err(|error| error.to_string())?;

    let refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let Some(refreshed) = refreshed else {
        return Ok(None);
    };

    if let Some(app_handle) = app_handle {
        let _ = app_handle.emit(
            "agent:workspace_changed",
            serde_json::json!({ "conversation_id": refreshed.conversation_id.as_str() }),
        );
    }
    Ok(Some(refreshed))
}

/// Gated auto-update from base (all must hold: opt-in, idle, settled repair, non-blocked base —
/// the base-blocked case is already excluded by the caller). Every gate fails closed: a repo
/// error skips the update rather than risking an unattended mutation on unreadable state.
/// Publish-guard contention is a benign, expected skip (the third concurrency layer after
/// `claim_recovery` scheduling dedupe and the repair-attempt CAS fence), logged at `debug`; any
/// other update failure is a `warn` skip. Never propagates an error — a failed auto-update must
/// not turn into a scan-tick failure when detection already succeeded.
async fn attempt_gated_agent_workspace_base_auto_update(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
) -> bool {
    if !(workspace.auto_publish_enabled || workspace.auto_publish_initial_pr_enabled) {
        return false;
    }

    let active_run = match state
        .agent_run_repo
        .get_active_for_conversation(conversation_id)
        .await
    {
        Ok(active_run) => active_run,
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Skipping unattended agent workspace base auto-update: active-run idle gate failed closed"
            );
            return false;
        }
    };
    if active_run.is_some() {
        return false;
    }

    // The active-run gate above is blind to the reviewer, which runs in a dedicated child
    // conversation. Without this, an unattended base merge can land under an in-flight review and
    // invalidate the delta it is reviewing.
    match crate::application::agent_workspace_review::workspace_review_owns_worktree(
        state.agent_conversation_workspace_repo.as_ref(),
        conversation_id,
    )
    .await
    {
        Ok(true) => return false,
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Skipping unattended agent workspace base auto-update: Workspace Review gate failed closed"
            );
            return false;
        }
    }

    let repair_attempt = match state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
    {
        Ok(repair_attempt) => repair_attempt,
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Skipping unattended agent workspace base auto-update: durable repair gate failed closed"
            );
            return false;
        }
    };
    if repair_attempt.is_some_and(|attempt| attempt.is_unsettled()) {
        return false;
    }

    let selection = AgentConversationWorkspaceBaseSelection::for_workspace_reuse(workspace);
    match update_agent_conversation_workspace_from_base_for_app_state(
        state,
        execution_state,
        conversation_id.clone(),
        selection,
    )
    .await
    {
        Ok(_) => true,
        Err(error) if error == AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE => {
            tracing::debug!(
                conversation_id = conversation_id.as_str(),
                "Skipped unattended agent workspace base auto-update: publish guard busy"
            );
            false
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Unattended agent workspace base auto-update failed"
            );
            false
        }
    }
}
