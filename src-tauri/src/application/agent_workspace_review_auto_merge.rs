//! Durable GitHub auto-merge coordination for workspace Reviews.

use std::sync::Arc;
use std::time::Instant;

use tracing::{info, warn};

use crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path;
use crate::application::agent_workspace_review::{
    apply_current_target_to_monitor, load_current_workspace_review_eligible,
    load_or_create_monitor, lock_workspace_review_lifecycle, resolve_review_target_for_user,
    resolve_review_target_with_materialization,
    start_agent_workspace_review_unlocked_with_revalidated_target,
    workspace_review_mode_is_eligible, AgentWorkspaceReviewStart, AgentWorkspaceReviewTarget,
    AgentWorkspaceReviewTargetMaterialization,
};
use crate::application::publish_resilience::count_unpublished_publish_commits;
use crate::application::{AppState, GitService};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
    AgentWorkspaceReviewAutoMergeGuard, AgentWorkspaceReviewAutoMergeGuardStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope,
};
use crate::domain::services::github_service::{PrHealth, PrStatus};
use crate::error::{AppError, AppResult};

pub(crate) const REVIEW_AUTO_MERGE_PAUSED_SUMMARY: &str =
    "GitHub auto-merge is paused while the workspace Review is authoritative.";
const REVIEW_AUTO_MERGE_RESTORED_SUMMARY: &str =
    "GitHub auto-merge was restored after the workspace Review passed.";
const REVIEW_AUTO_MERGE_RESTORE_FAILED_SUMMARY: &str =
    "Workspace Review passed, but GitHub auto-merge could not be restored yet.";
const WORKSPACE_REVIEW_AUTO_MERGE_LOG_TARGET: &str =
    "ralphx_lib::application::agent_workspace_review_auto_merge";

fn log_workspace_review_auto_merge_phase(
    operation: &'static str,
    workspace: &AgentConversationWorkspace,
    phase: &'static str,
    phase_started: Instant,
    total_started: Instant,
) {
    info!(
        target: WORKSPACE_REVIEW_AUTO_MERGE_LOG_TARGET,
        operation,
        phase,
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = phase_started.elapsed().as_millis(),
        total_elapsed_ms = total_started.elapsed().as_millis(),
        "Workspace Review auto-merge phase completed"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceReviewStartOrigin {
    Manual,
    Automated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReviewAutoMergePreview {
    pub target: AgentWorkspaceReviewTarget,
    pub pr_number: i64,
    pub merge_method: String,
    pub restore_after_publish: bool,
}

/// The target/effect snapshot a user explicitly accepted before a manual review start.
/// The server resolves it again before it mutates GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReviewStartConfirmation {
    pub target_scope: Option<AgentWorkspaceReviewTargetScope>,
    pub diff_fingerprint: Option<String>,
    pub head_sha: Option<String>,
    pub pr_number: Option<i64>,
    pub will_disable_auto_merge: bool,
    pub merge_method: Option<String>,
    pub restore_after_publish: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReviewManualStartPreview {
    pub target: Option<AgentWorkspaceReviewTarget>,
    pub auto_merge: Option<WorkspaceReviewAutoMergePreview>,
    pub confirmation: WorkspaceReviewStartConfirmation,
}

/// Produces the target-bound effect shown in the manual-start confirmation dialog.
pub async fn preview_manual_workspace_review_start(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<WorkspaceReviewManualStartPreview> {
    preview_workspace_review_start(state, workspace, true).await
}

async fn preview_workspace_review_start(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    user_initiated: bool,
) -> AppResult<WorkspaceReviewManualStartPreview> {
    let total_started = Instant::now();
    let phase_started = Instant::now();
    let workspace = load_current_workspace_review_eligible(state, workspace).await?;
    let workspace = &workspace;
    log_workspace_review_auto_merge_phase(
        "workspace_review_start_preview_phase",
        workspace,
        "load_workspace",
        phase_started,
        total_started,
    );
    let phase_started = Instant::now();
    let target = if user_initiated {
        resolve_current_target_for_user(
            state,
            workspace,
            AgentWorkspaceReviewTargetMaterialization::IdentityOnly,
        )
        .await?
    } else {
        resolve_current_target_with_materialization(
            state,
            workspace,
            AgentWorkspaceReviewTargetMaterialization::IdentityOnly,
        )
        .await?
    };
    log_workspace_review_auto_merge_phase(
        "workspace_review_start_preview_phase",
        workspace,
        "resolve_target",
        phase_started,
        total_started,
    );
    let pr_number = target
        .as_ref()
        .and_then(|target| review_target_pr_number(workspace, target));
    let phase_started = Instant::now();
    let auto_merge = match (target.as_ref(), pr_number) {
        (Some(target), Some(pr_number)) => {
            let github = state.github_service.as_ref().ok_or_else(|| {
                AppError::Infrastructure(
                    "GitHub integration is unavailable for this PR-backed workspace Review"
                        .to_string(),
                )
            })?;
            github
                .fetch_pr_auto_merge_state(&target.working_directory, pr_number)
                .await?
                .map(|request| {
                    let merge_method = request
                        .merge_method
                        .filter(|method| !method.trim().is_empty())
                        .unwrap_or_else(|| workspace.pr_auto_merge_method.clone());
                    WorkspaceReviewAutoMergePreview {
                        target: target.clone(),
                        pr_number,
                        merge_method,
                        restore_after_publish: target.scope
                            == AgentWorkspaceReviewTargetScope::WorkspaceDelta,
                    }
                })
        }
        _ => None,
    };
    log_workspace_review_auto_merge_phase(
        "workspace_review_start_preview_phase",
        workspace,
        "probe_github_auto_merge",
        phase_started,
        total_started,
    );
    let confirmation = WorkspaceReviewStartConfirmation {
        target_scope: target.as_ref().map(|target| target.scope),
        diff_fingerprint: target
            .as_ref()
            .map(|target| target.diff_fingerprint.clone()),
        head_sha: target.as_ref().and_then(|target| target.head_sha.clone()),
        pr_number,
        will_disable_auto_merge: auto_merge.is_some(),
        merge_method: auto_merge
            .as_ref()
            .map(|effect| effect.merge_method.clone()),
        restore_after_publish: auto_merge
            .as_ref()
            .is_some_and(|effect| effect.restore_after_publish),
    };
    let preview = WorkspaceReviewManualStartPreview {
        target,
        auto_merge,
        confirmation,
    };
    log_workspace_review_auto_merge_phase(
        "workspace_review_start_preview_phase",
        workspace,
        "total",
        total_started,
        total_started,
    );
    Ok(preview)
}

/// Re-reads GitHub state for a review target. A preview is absent when there is no open PR or
/// auto-merge is already disabled. PR-backed targets fail closed when GitHub is unavailable.
pub async fn preview_workspace_review_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<WorkspaceReviewAutoMergePreview>> {
    Ok(preview_workspace_review_start(state, workspace, false)
        .await?
        .auto_merge)
}

/// Starts the existing reviewer only after a durable guard owns a currently-enabled GitHub
/// auto-merge request and GitHub confirms it was disabled. Automated callers intentionally skip
/// UI confirmation, but retain the same backend ordering.
pub async fn start_guarded_agent_workspace_review(
    state: Arc<AppState>,
    workspace: &AgentConversationWorkspace,
    force: bool,
    origin: WorkspaceReviewStartOrigin,
    confirmation: Option<&WorkspaceReviewStartConfirmation>,
) -> AppResult<AgentWorkspaceReviewStart> {
    start_guarded_agent_workspace_review_with_runtime_override(
        state,
        workspace,
        force,
        origin,
        confirmation,
        None,
    )
    .await
}

pub async fn start_guarded_agent_workspace_review_with_runtime_override(
    state: Arc<AppState>,
    workspace: &AgentConversationWorkspace,
    force: bool,
    origin: WorkspaceReviewStartOrigin,
    confirmation: Option<&WorkspaceReviewStartConfirmation>,
    runtime_override: Option<&crate::domain::agents::ManualRoleRuntimeOverride>,
) -> AppResult<AgentWorkspaceReviewStart> {
    let total_started = Instant::now();
    let phase_started = Instant::now();
    let _lifecycle_guard = lock_workspace_review_lifecycle(&workspace.conversation_id).await;
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "wait_for_lifecycle_lock",
        phase_started,
        total_started,
    );
    let phase_started = Instant::now();
    let workspace = load_current_workspace_review_eligible(state.as_ref(), workspace).await?;
    let workspace = &workspace;
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "load_workspace",
        phase_started,
        total_started,
    );
    let phase_started = Instant::now();
    let (preview, revalidated_target) = match origin {
        WorkspaceReviewStartOrigin::Manual => {
            // A fixer run owns the worktree, so any review started now would be invalidated by its
            // very first commit. Reject with guidance rather than burn a reviewer generation. The
            // reviewer's own child run lives in a different conversation and never trips this.
            // Automated origins are deliberately excluded: they already defer through their own
            // routing seams, and the backend AwaitingReview start fires only after the fixer run
            // has completed.
            if state
                .agent_run_repo
                .get_active_for_conversation(&workspace.conversation_id)
                .await
                .map_err(|error| {
                    AppError::Conflict(format!(
                        "workspace Review start could not confirm the workspace is idle: {error}"
                    ))
                })?
                .is_some()
            {
                return Err(AppError::Conflict(
                    "An agent run (fixer) is active in this workspace. Start the review after it completes."
                        .to_string(),
                ));
            }
            let manual_preview =
                preview_manual_workspace_review_start(state.as_ref(), workspace).await?;
            let Some(confirmation) = confirmation else {
                return Err(AppError::Conflict(
                    "workspace Review start requires a fresh confirmation".to_string(),
                ));
            };
            if confirmation != &manual_preview.confirmation {
                return Err(AppError::Conflict(
                    "workspace Review target or GitHub auto-merge state changed; refresh and confirm again"
                        .to_string(),
                ));
            }
            (manual_preview.auto_merge, manual_preview.target)
        }
        WorkspaceReviewStartOrigin::Automated => (
            preview_workspace_review_auto_merge_guard(state.as_ref(), workspace).await?,
            None,
        ),
    };
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "revalidate_confirmation",
        phase_started,
        total_started,
    );
    let Some(preview) = preview else {
        let phase_started = Instant::now();
        let start = start_agent_workspace_review_unlocked_with_revalidated_target(
            Arc::clone(&state),
            workspace,
            force,
            runtime_override,
            revalidated_target.clone(),
        )
        .await?;
        log_workspace_review_auto_merge_phase(
            "workspace_review_guarded_start_phase",
            workspace,
            "start_review",
            phase_started,
            total_started,
        );
        let phase_started = Instant::now();
        let settled = settle_skipped_guarded_workspace_review_start(
            state.as_ref(),
            workspace,
            start,
            false,
            None,
        )
        .await;
        log_workspace_review_auto_merge_phase(
            "workspace_review_guarded_start_phase",
            workspace,
            "settle_guard",
            phase_started,
            total_started,
        );
        log_workspace_review_auto_merge_phase(
            "workspace_review_guarded_start_phase",
            workspace,
            "total",
            total_started,
            total_started,
        );
        return settled;
    };

    let phase_started = Instant::now();
    let monitor = load_or_create_monitor(state.as_ref(), workspace).await?;
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "load_monitor",
        phase_started,
        total_started,
    );
    if let Some(existing_guard) = monitor.auto_merge_guard.as_ref() {
        if guard_matches_target(existing_guard, workspace, &preview.target) {
            ensure_guarded_auto_merge_is_paused(state.as_ref(), workspace, existing_guard).await?;
            let current_monitor = load_or_create_monitor(state.as_ref(), workspace).await?;
            let Some(current_guard) = current_monitor.auto_merge_guard.as_ref() else {
                return Err(AppError::Conflict(
                    "workspace Review auto-merge guard is no longer authoritative; refresh and retry"
                        .to_string(),
                ));
            };
            if !guard_matches_target(current_guard, workspace, &preview.target) {
                return Err(AppError::Conflict(
                    "workspace Review auto-merge guard changed while pausing GitHub auto-merge"
                        .to_string(),
                ));
            }
            let start = start_agent_workspace_review_unlocked_with_revalidated_target(
                Arc::clone(&state),
                workspace,
                force,
                runtime_override,
                revalidated_target.clone(),
            )
            .await?;
            return settle_skipped_guarded_workspace_review_start(
                state.as_ref(),
                workspace,
                start,
                false,
                None,
            )
            .await;
        }
        return Err(AppError::Conflict(
            "another workspace Review auto-merge guard is still authoritative".to_string(),
        ));
    }

    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await?;
    let pausing = guard_for_preview(&preview, AgentWorkspaceReviewAutoMergeGuardStatus::Pausing);
    let phase_started = Instant::now();
    let claimed = state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            None,
            Some(pausing.clone()),
        )
        .await?;
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "claim_auto_merge_guard",
        phase_started,
        total_started,
    );
    if !claimed {
        return Err(AppError::Conflict(
            "workspace Review auto-merge state changed; refresh and retry".to_string(),
        ));
    }

    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::Infrastructure(
            "GitHub integration became unavailable before review start".to_string(),
        )
    })?;
    let phase_started = Instant::now();
    if let Err(error) = github
        .disable_pr_auto_merge(&preview.target.working_directory, preview.pr_number)
        .await
    {
        let _ = state
            .agent_conversation_workspace_repo
            .compare_and_set_workspace_review_auto_merge_guard(
                &workspace.conversation_id,
                Some(pausing),
                None,
            )
            .await;
        return Err(AppError::Infrastructure(format!(
            "could not disable GitHub auto-merge before workspace Review: {error}"
        )));
    }
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "disable_github_auto_merge",
        phase_started,
        total_started,
    );

    let phase_started = Instant::now();
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            &workspace.conversation_id,
            Some(false),
            Some("review_paused"),
            Some(REVIEW_AUTO_MERGE_PAUSED_SUMMARY),
        )
        .await
    {
        let _ = restore_guarded_auto_merge_after_failed_start(
            state.as_ref(),
            workspace,
            &pausing,
            &preview.target.working_directory,
        )
        .await;
        return Err(error);
    }
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "persist_auto_merge_state",
        phase_started,
        total_started,
    );
    let paused = guard_for_preview(
        &preview,
        AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
    );
    let phase_started = Instant::now();
    let paused_persisted = state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(pausing.clone()),
            Some(paused.clone()),
        )
        .await?;
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "persist_paused_guard",
        phase_started,
        total_started,
    );
    if !paused_persisted {
        let _ = restore_guarded_auto_merge_after_failed_start(
            state.as_ref(),
            workspace,
            &pausing,
            &preview.target.working_directory,
        )
        .await;
        return Err(AppError::Conflict(
            "workspace Review auto-merge guard changed after GitHub was paused".to_string(),
        ));
    }
    let phase_started = Instant::now();
    let mut paused_monitor = load_or_create_monitor(state.as_ref(), workspace).await?;
    if paused_monitor.auto_merge_guard.as_ref() != Some(&paused) {
        let _ = restore_guarded_auto_merge_after_failed_start(
            state.as_ref(),
            workspace,
            &paused,
            &preview.target.working_directory,
        )
        .await;
        return Err(AppError::Conflict(
            "workspace Review auto-merge guard changed before review target was recorded"
                .to_string(),
        ));
    }
    apply_current_target_to_monitor(&mut paused_monitor, Some(&preview.target));
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(paused_monitor)
        .await?;
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "record_review_target",
        phase_started,
        total_started,
    );
    let phase_started = Instant::now();
    append_auto_merge_guard_event(
        state.as_ref(),
        workspace,
        &paused,
        "paused",
        "succeeded",
        REVIEW_AUTO_MERGE_PAUSED_SUMMARY,
    )
    .await;
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "append_guard_event",
        phase_started,
        total_started,
    );

    let phase_started = Instant::now();
    let started = start_agent_workspace_review_unlocked_with_revalidated_target(
        Arc::clone(&state),
        workspace,
        force,
        runtime_override,
        revalidated_target,
    )
    .await;
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "start_review",
        phase_started,
        total_started,
    );
    let result = match started {
        Ok(start) if start.started => Ok(start),
        Ok(start) => {
            settle_skipped_guarded_workspace_review_start(
                state.as_ref(),
                workspace,
                start,
                true,
                Some(&preview.target.working_directory),
            )
            .await
        }
        Err(error) => {
            let restore_result = restore_guarded_auto_merge_after_failed_start(
                state.as_ref(),
                workspace,
                &paused,
                &preview.target.working_directory,
            )
            .await;
            restore_result?;
            Err(error)
        }
    };
    log_workspace_review_auto_merge_phase(
        "workspace_review_guarded_start_phase",
        workspace,
        "total",
        total_started,
        total_started,
    );
    result
}

/// A skipped start can still represent a current passing Review. Settle that result through the
/// durable guard instead of treating it as a failed reviewer launch: a workspace-delta pass must
/// continue waiting for its matching publish, while a vanished target must stay disabled.
async fn settle_skipped_guarded_workspace_review_start(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    mut start: AgentWorkspaceReviewStart,
    guard_created_by_start: bool,
    failed_start_working_directory: Option<&std::path::Path>,
) -> AppResult<AgentWorkspaceReviewStart> {
    let Some(guard) = start.context.monitor.auto_merge_guard.clone() else {
        return Ok(start);
    };
    match start.context.monitor.review_outcome {
        AgentWorkspaceReviewOutcome::Passed => {
            start.context.monitor = handle_passing_workspace_review_auto_merge_guard(
                state,
                workspace,
                &start.context.monitor,
            )
            .await?;
        }
        AgentWorkspaceReviewOutcome::NoChanges => {
            cancel_guard_without_restoring(
                state,
                workspace,
                &guard,
                "Workspace Review had no remaining target, so GitHub auto-merge was not restored.",
            )
            .await?;
            start.context.monitor = load_or_create_monitor(state, workspace).await?;
        }
        _ if guard_created_by_start => {
            let working_directory = failed_start_working_directory.ok_or_else(|| {
                AppError::Infrastructure(
                    "failed workspace Review start lost its auto-merge restoration path"
                        .to_string(),
                )
            })?;
            restore_guarded_auto_merge_after_failed_start(
                state,
                workspace,
                &guard,
                working_directory,
            )
            .await?;
            start.context.monitor = load_or_create_monitor(state, workspace).await?;
        }
        _ => {}
    }
    Ok(start)
}

/// Advances only a current passing Review. Local workspace deltas remain paused until a successful
/// publish calls [`restore_guarded_auto_merge_after_publish`].
pub async fn handle_passing_workspace_review_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> AppResult<AgentWorkspaceReviewMonitor> {
    let workspace = load_current_workspace_review_eligible(state, workspace).await?;
    let workspace = &workspace;
    let current = load_or_create_monitor(state, workspace).await?;
    if !passing_monitor_is_current(&current, monitor) {
        return Ok(current);
    }
    let Some(guard) = current.auto_merge_guard.as_ref() else {
        return Ok(current);
    };
    if !monitor_has_current_passing_review_for_guard(&current, guard) {
        return Ok(current);
    }
    let Some(target) = resolve_current_target(state, workspace).await? else {
        cancel_guard_without_restoring(
            state,
            workspace,
            guard,
            "workspace Review target no longer exists after the passing run",
        )
        .await?;
        return load_or_create_monitor(state, workspace).await;
    };
    if !guard_matches_target(guard, workspace, &target) {
        cancel_guard_without_restoring(
            state,
            workspace,
            guard,
            "workspace Review target changed after the passing run",
        )
        .await?;
        return load_or_create_monitor(state, workspace).await;
    }
    match target.scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => {
            restore_guarded_auto_merge(state, workspace, guard).await?;
        }
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => {
            if workspace_delta_already_published_proves_guard(
                state,
                &current,
                workspace,
                guard,
                &target.working_directory,
            )
            .await
            {
                restore_guarded_auto_merge(state, workspace, guard).await?;
            } else {
                let awaiting_publish = AgentWorkspaceReviewAutoMergeGuard {
                    status: AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish,
                    ..guard.clone()
                };
                let transitioned = state
                    .agent_conversation_workspace_repo
                    .compare_and_set_workspace_review_auto_merge_guard(
                        &workspace.conversation_id,
                        Some(guard.clone()),
                        Some(awaiting_publish.clone()),
                    )
                    .await?;
                if transitioned {
                    if let Err(error) = append_workspace_delta_restore_deferred_event(
                        state,
                        workspace,
                        &awaiting_publish,
                        &current,
                    )
                    .await
                    {
                        let rolled_back = state
                            .agent_conversation_workspace_repo
                            .compare_and_set_workspace_review_auto_merge_guard(
                                &workspace.conversation_id,
                                Some(awaiting_publish),
                                Some(guard.clone()),
                            )
                            .await?;
                        if !rolled_back {
                            warn!(
                                conversation_id = %workspace.conversation_id,
                                "Workspace Review auto-merge guard changed while rolling back a failed publish marker"
                            );
                        }
                        return Err(error);
                    }
                }
            }
        }
    }
    load_or_create_monitor(state, workspace).await
}

/// Called after a successful workspace publication proves that the guarded workspace delta reached
/// the same PR.
pub async fn restore_guarded_auto_merge_after_publish(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<()> {
    let workspace = load_current_workspace_review_eligible(state, workspace).await?;
    let workspace = &workspace;
    let monitor = load_or_create_monitor(state, workspace).await?;
    let Some(guard) = monitor.auto_merge_guard.as_ref() else {
        return Ok(());
    };
    if !matches!(
        guard.status,
        AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish
            | AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    ) {
        return Ok(());
    }
    let ordering_proves =
        workspace_delta_publish_proves_guard(state, &monitor, workspace, guard).await?;
    let already_published_proves = if ordering_proves {
        false
    } else {
        match resolve_workspace_working_directory(state, workspace).await {
            Ok(working_directory) => {
                workspace_delta_already_published_proves_guard(
                    state,
                    &monitor,
                    workspace,
                    guard,
                    &working_directory,
                )
                .await
            }
            Err(error) => {
                warn!(
                    target: WORKSPACE_REVIEW_AUTO_MERGE_LOG_TARGET,
                    conversation_id = %workspace.conversation_id,
                    error = %error,
                    "Workspace Review auto-merge could not resolve the workspace while checking an already-published guard"
                );
                false
            }
        }
    };
    if !(ordering_proves || already_published_proves) {
        return Ok(());
    }
    restore_guarded_auto_merge(state, workspace, guard).await
}

pub fn auto_merge_guard_blocks_enable(monitor: Option<&AgentWorkspaceReviewMonitor>) -> bool {
    monitor
        .and_then(|monitor| monitor.auto_merge_guard.as_ref())
        .is_some()
}

pub async fn cancel_workspace_review_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    cancel_workspace_review_auto_merge_guard_with_reason(
        state,
        workspace,
        "GitHub auto-merge will remain disabled because workspace supervision was turned off.",
    )
    .await
}

async fn cancel_workspace_review_auto_merge_guard_with_reason(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    reason: &str,
) -> AppResult<bool> {
    let monitor = load_or_create_monitor(state, workspace).await?;
    let Some(guard) = monitor.auto_merge_guard else {
        return Ok(false);
    };
    if guard_status_may_have_remote_auto_merge_enabled(guard.status) {
        let working_directory = resolve_workspace_working_directory(state, workspace).await?;
        let github = state.github_service.as_ref().ok_or_else(|| {
            AppError::Infrastructure(
                "GitHub integration is unavailable while cancelling auto-merge restoration"
                    .to_string(),
            )
        })?;
        let health = github
            .fetch_pr_health(&working_directory, guard.pr_number)
            .await?;
        if health.auto_merge_request.is_some() {
            github
                .disable_pr_auto_merge(&working_directory, guard.pr_number)
                .await?;
        }
    }
    let cancelled = state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(guard.clone()),
            None,
        )
        .await?;
    if cancelled {
        state
            .agent_conversation_workspace_repo
            .update_pr_auto_merge_state(
                &workspace.conversation_id,
                Some(false),
                Some("review_paused"),
                Some(reason),
            )
            .await?;
        append_auto_merge_guard_event(state, workspace, &guard, "cancelled", "cancelled", reason)
            .await;
    }
    Ok(cancelled)
}

fn guard_status_may_have_remote_auto_merge_enabled(
    status: AgentWorkspaceReviewAutoMergeGuardStatus,
) -> bool {
    matches!(
        status,
        AgentWorkspaceReviewAutoMergeGuardStatus::Pausing
            | AgentWorkspaceReviewAutoMergeGuardStatus::Restoring
            | AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    )
}

/// Reconciles durable guards after startup. This intentionally works from the monitor rows rather
/// than only reviewing runs, so a crash after pausing, passing, publishing, or restoring remains
/// fail-closed and retryable.
pub async fn reconcile_workspace_review_auto_merge_guards(state: &AppState) -> AppResult<usize> {
    let monitors = state
        .agent_conversation_workspace_repo
        .list_active_workspace_review_auto_merge_guards()
        .await?;
    let mut reconciled = 0usize;
    for monitor in monitors {
        let Some(workspace) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&monitor.conversation_id)
            .await?
        else {
            continue;
        };
        let _lifecycle_guard = lock_workspace_review_lifecycle(&workspace.conversation_id).await;
        if !workspace_review_mode_is_eligible(workspace.mode) {
            if cleanup_ineligible_workspace_review_auto_merge_guard(state, &workspace).await? {
                reconciled += 1;
            }
            continue;
        }
        if reconcile_workspace_review_auto_merge_guard(state, &workspace, &monitor).await? {
            reconciled += 1;
        }
    }
    Ok(reconciled)
}

pub(crate) async fn cleanup_ineligible_workspace_review_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    cancel_workspace_review_auto_merge_guard_with_reason(
        state,
        workspace,
        "GitHub auto-merge will remain disabled because Workspace Review is unavailable in the current workspace mode.",
    )
    .await
}

async fn reconcile_workspace_review_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> AppResult<bool> {
    let Some(guard) = monitor.auto_merge_guard.as_ref() else {
        return Ok(false);
    };
    if !workspace.pr_auto_merge_desired {
        return cancel_workspace_review_auto_merge_guard(state, workspace).await;
    }
    if guarded_pr_is_terminal(workspace, guard.pr_number) {
        return cancel_guard_without_restoring(
            state,
            workspace,
            guard,
            "GitHub auto-merge was not restored because the guarded pull request is terminal.",
        )
        .await
        .map(|_| true);
    }

    if guard.target_scope == AgentWorkspaceReviewTargetScope::WorkspaceDelta
        && matches!(
            guard.status,
            AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish
                | AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
        )
    {
        let ordering_proves =
            workspace_delta_publish_proves_guard(state, monitor, workspace, guard).await?;
        let already_published_proves = if ordering_proves {
            false
        } else {
            match resolve_workspace_working_directory(state, workspace).await {
                Ok(working_directory) => {
                    workspace_delta_already_published_proves_guard(
                        state,
                        monitor,
                        workspace,
                        guard,
                        &working_directory,
                    )
                    .await
                }
                Err(error) => {
                    warn!(
                        target: WORKSPACE_REVIEW_AUTO_MERGE_LOG_TARGET,
                        conversation_id = %workspace.conversation_id,
                        error = %error,
                        "Workspace Review auto-merge could not resolve the workspace while reconciling an already-published guard"
                    );
                    false
                }
            }
        };
        if !(ordering_proves || already_published_proves) {
            return Ok(false);
        }
        restore_guarded_auto_merge(state, workspace, guard).await?;
        return Ok(true);
    }

    if monitor_has_current_passing_review_for_guard(monitor, guard) {
        let before = guard.clone();
        let after =
            handle_passing_workspace_review_auto_merge_guard(state, workspace, monitor).await?;
        return Ok(after.auto_merge_guard.as_ref() != Some(&before));
    }

    if guard.status == AgentWorkspaceReviewAutoMergeGuardStatus::Restoring {
        return reconcile_interrupted_restore(state, workspace, guard).await;
    }

    ensure_guarded_auto_merge_is_paused(state, workspace, guard).await
}

async fn ensure_guarded_auto_merge_is_paused(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
) -> AppResult<bool> {
    let Some(target) = resolve_current_target(state, workspace).await? else {
        return cancel_guard_without_restoring(
            state,
            workspace,
            guard,
            "Workspace Review target no longer exists, so GitHub auto-merge will remain disabled.",
        )
        .await
        .map(|_| true);
    };
    if !guard_matches_target(guard, workspace, &target) {
        return cancel_guard_without_restoring(
            state,
            workspace,
            guard,
            "Workspace Review target changed, so GitHub auto-merge will remain disabled.",
        )
        .await
        .map(|_| true);
    }
    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::Infrastructure(
            "GitHub integration is unavailable while reconciling workspace Review auto-merge"
                .to_string(),
        )
    })?;
    let health = github
        .fetch_pr_health(&target.working_directory, guard.pr_number)
        .await?;
    if pr_health_is_terminal(&health) {
        return cancel_guard_without_restoring(
            state,
            workspace,
            guard,
            "GitHub auto-merge will remain disabled because the guarded pull request is terminal.",
        )
        .await
        .map(|_| true);
    }
    let mut changed = false;
    if health.auto_merge_request.is_some() {
        github
            .disable_pr_auto_merge(&target.working_directory, guard.pr_number)
            .await?;
        changed = true;
    }
    state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            &workspace.conversation_id,
            Some(false),
            Some("review_paused"),
            Some(REVIEW_AUTO_MERGE_PAUSED_SUMMARY),
        )
        .await?;
    if guard.status == AgentWorkspaceReviewAutoMergeGuardStatus::Pausing {
        let paused = AgentWorkspaceReviewAutoMergeGuard {
            status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
            ..guard.clone()
        };
        changed |= state
            .agent_conversation_workspace_repo
            .compare_and_set_workspace_review_auto_merge_guard(
                &workspace.conversation_id,
                Some(guard.clone()),
                Some(paused),
            )
            .await?;
    }
    Ok(changed)
}

async fn reconcile_interrupted_restore(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
) -> AppResult<bool> {
    let monitor = load_or_create_monitor(state, workspace).await?;
    let working_directory =
        if workspace_delta_publish_proves_guard(state, &monitor, workspace, guard).await? {
            resolve_workspace_working_directory(state, workspace).await?
        } else {
            let Some(target) = resolve_current_target(state, workspace).await? else {
                return cancel_guard_without_restoring(
                state,
                workspace,
                guard,
                "Workspace Review target no longer exists, so GitHub auto-merge was not restored.",
            )
            .await
            .map(|_| true);
            };
            if !guard_matches_target(guard, workspace, &target) {
                return cancel_guard_without_restoring(
                    state,
                    workspace,
                    guard,
                    "Workspace Review target changed, so GitHub auto-merge was not restored.",
                )
                .await
                .map(|_| true);
            }
            target.working_directory
        };
    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::Infrastructure(
            "GitHub integration is unavailable while reconciling auto-merge restoration"
                .to_string(),
        )
    })?;
    let health = github
        .fetch_pr_health(&working_directory, guard.pr_number)
        .await?;
    if health.auto_merge_request.is_none() {
        return mark_restore_failed(
            state,
            workspace,
            guard.clone(),
            "GitHub did not report auto-merge as enabled after an interrupted restoration"
                .to_string(),
        )
        .await
        .map(|_| true);
    }
    finalize_confirmed_auto_merge_restore(state, workspace, guard, &working_directory).await
}

async fn resolve_current_target(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    resolve_current_target_with_materialization(
        state,
        workspace,
        AgentWorkspaceReviewTargetMaterialization::FullPacket,
    )
    .await
}

async fn resolve_current_target_with_materialization(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    materialization: AgentWorkspaceReviewTargetMaterialization,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    resolve_review_target_with_materialization(workspace, &project, materialization).await
}

async fn resolve_current_target_for_user(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    materialization: AgentWorkspaceReviewTargetMaterialization,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    resolve_review_target_for_user(workspace, &project, materialization).await
}

async fn resolve_workspace_working_directory(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<std::path::PathBuf> {
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    resolve_valid_agent_conversation_workspace_path(&project, workspace).await
}

fn review_target_pr_number(
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
) -> Option<i64> {
    let pr_number = target
        .source_pull_request_number
        .or(workspace.publication_pr_number)?;
    (!guarded_pr_is_terminal(workspace, pr_number)).then_some(pr_number)
}

fn guarded_pr_is_terminal(workspace: &AgentConversationWorkspace, pr_number: i64) -> bool {
    workspace.publication_pr_number == Some(pr_number)
        && workspace.has_terminal_publication_pr_status()
}

fn pr_health_is_terminal(health: &PrHealth) -> bool {
    pr_status_is_terminal(&health.sync_state.status)
}

fn pr_status_is_terminal(status: &PrStatus) -> bool {
    matches!(status, PrStatus::Closed | PrStatus::Merged { .. })
}

fn guard_for_preview(
    preview: &WorkspaceReviewAutoMergePreview,
    status: AgentWorkspaceReviewAutoMergeGuardStatus,
) -> AgentWorkspaceReviewAutoMergeGuard {
    AgentWorkspaceReviewAutoMergeGuard {
        status,
        pr_number: preview.pr_number,
        merge_method: preview.merge_method.clone(),
        target_scope: preview.target.scope,
        diff_fingerprint: preview.target.diff_fingerprint.clone(),
        head_sha: preview.target.head_sha.clone(),
        last_error: None,
    }
}

fn guard_matches_target(
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
) -> bool {
    review_target_pr_number(workspace, target) == Some(guard.pr_number)
        && guard.target_scope == target.scope
        && guard.diff_fingerprint == target.diff_fingerprint
        && (target.scope == AgentWorkspaceReviewTargetScope::WorkspaceDelta
            || guard.head_sha == target.head_sha)
}

fn passing_monitor_is_current(
    current: &AgentWorkspaceReviewMonitor,
    completed: &AgentWorkspaceReviewMonitor,
) -> bool {
    current.review_outcome == AgentWorkspaceReviewOutcome::Passed
        && current.last_run_id == completed.last_run_id
        && current.current_target_scope == completed.current_target_scope
        && current.current_diff_fingerprint == completed.current_diff_fingerprint
        && current.reviewed_target_scope == completed.reviewed_target_scope
        && current.reviewed_diff_fingerprint == completed.reviewed_diff_fingerprint
}

fn monitor_has_current_passing_review_for_guard(
    monitor: &AgentWorkspaceReviewMonitor,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
) -> bool {
    monitor.has_current_passing_review_for_target(
        guard.target_scope,
        guard.head_sha.as_deref(),
        &guard.diff_fingerprint,
    )
}

fn workspace_delta_guard_review_is_current(
    monitor: &AgentWorkspaceReviewMonitor,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
) -> bool {
    guard.target_scope == AgentWorkspaceReviewTargetScope::WorkspaceDelta
        && monitor.review_outcome == AgentWorkspaceReviewOutcome::Passed
        && monitor.reviewed_target_scope == Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
        && monitor.reviewed_diff_fingerprint.as_deref() == Some(guard.diff_fingerprint.as_str())
        && workspace.publication_pr_number == Some(guard.pr_number)
        && workspace.has_pr_status_pollable_push_status()
        && !workspace.has_terminal_publication_pr_status()
}

async fn workspace_delta_publish_proves_guard(
    state: &AppState,
    monitor: &AgentWorkspaceReviewMonitor,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
) -> AppResult<bool> {
    if !workspace_delta_guard_review_is_current(monitor, workspace, guard) {
        return Ok(false);
    }
    let Some(marker) = workspace_delta_restore_deferred_classification(guard, monitor) else {
        return Ok(false);
    };
    let publish_classification = format!("published:{}", guard.pr_number);
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    let Some(marker_index) = events
        .iter()
        .position(|event| event.classification.as_deref() == Some(marker.as_str()))
    else {
        return Ok(false);
    };
    Ok(events[marker_index + 1..].iter().any(|event| {
        event.step == "published"
            && event.status == "succeeded"
            && event.classification.as_deref() == Some(publish_classification.as_str())
    }))
}

async fn workspace_delta_already_published_proves_guard(
    state: &AppState,
    monitor: &AgentWorkspaceReviewMonitor,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    working_directory: &std::path::Path,
) -> bool {
    if !workspace_delta_guard_review_is_current(monitor, workspace, guard) {
        return false;
    }
    match count_unpublished_publish_commits(working_directory, &workspace.branch_name).await {
        Ok(Some(0)) => {}
        Ok(Some(_)) | Ok(None) => return false,
        Err(error) => {
            warn!(
                target: WORKSPACE_REVIEW_AUTO_MERGE_LOG_TARGET,
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Workspace Review auto-merge could not count unpublished commits for an already-published guard"
            );
            return false;
        }
    }
    let Some(github) = state.github_service.as_ref() else {
        warn!(
            target: WORKSPACE_REVIEW_AUTO_MERGE_LOG_TARGET,
            conversation_id = %workspace.conversation_id,
            "Workspace Review auto-merge cannot prove an already-published guard without GitHub"
        );
        return false;
    };
    let health = match github
        .fetch_pr_health(working_directory, guard.pr_number)
        .await
    {
        Ok(health) => health,
        Err(error) => {
            warn!(
                target: WORKSPACE_REVIEW_AUTO_MERGE_LOG_TARGET,
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Workspace Review auto-merge could not fetch guarded pull request health"
            );
            return false;
        }
    };
    if pr_health_is_terminal(&health) {
        return false;
    }
    let Some(remote_head_oid) = health.sync_state.head_ref_oid.as_deref() else {
        return false;
    };
    let local_head_oid = match GitService::resolve_ref_sha(
        working_directory,
        &workspace.branch_name,
    )
    .await
    {
        Ok(oid) => oid,
        Err(error) => {
            warn!(
                target: WORKSPACE_REVIEW_AUTO_MERGE_LOG_TARGET,
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Workspace Review auto-merge could not resolve the local head for an already-published guard"
            );
            return false;
        }
    };
    remote_head_oid == local_head_oid
}

fn workspace_delta_restore_deferred_classification(
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    monitor: &AgentWorkspaceReviewMonitor,
) -> Option<String> {
    monitor.last_run_id.as_deref().map(|run_id| {
        format!(
            "workspace_review_auto_merge:restore_deferred:{}:{}:{run_id}",
            guard.pr_number, guard.diff_fingerprint
        )
    })
}

async fn append_workspace_delta_restore_deferred_event(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    monitor: &AgentWorkspaceReviewMonitor,
) -> AppResult<()> {
    let Some(classification) = workspace_delta_restore_deferred_classification(guard, monitor)
    else {
        return Err(AppError::Infrastructure(
            "Workspace Review passed without a run id for deferred auto-merge restoration"
                .to_string(),
        ));
    };
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    if events
        .iter()
        .any(|entry| entry.classification.as_deref() == Some(classification.as_str()))
    {
        return Ok(());
    }
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id.clone(),
            "workspace_review_auto_merge",
            "waiting",
            "Workspace Review passed; GitHub auto-merge will resume after these changes are published.",
            Some(classification),
        ))
        .await
}

async fn cancel_guard_without_restoring(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    reason: &str,
) -> AppResult<()> {
    if !state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(guard.clone()),
            None,
        )
        .await?
    {
        return Ok(());
    }
    state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            &workspace.conversation_id,
            Some(false),
            Some("review_paused"),
            Some(reason),
        )
        .await?;
    append_auto_merge_guard_event(state, workspace, guard, "cancelled", "cancelled", reason).await;
    Ok(())
}

async fn restore_guarded_auto_merge_after_failed_start(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    working_directory: &std::path::Path,
) -> AppResult<()> {
    if !matches!(
        guard.status,
        AgentWorkspaceReviewAutoMergeGuardStatus::Pausing
            | AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview
    ) {
        return Err(AppError::Conflict(
            "failed-start auto-merge restoration requires an attempt-owned guard".to_string(),
        ));
    }
    let working_directory = crate::utils::path_safety::validate_absolute_non_root_path(
        working_directory,
        "failed workspace Review start working directory",
    )?;
    restore_guarded_auto_merge_with_failed_start_path(
        state,
        workspace,
        guard,
        Some(&working_directory),
    )
    .await
}

pub(crate) async fn restore_guarded_auto_merge(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
) -> AppResult<()> {
    restore_guarded_auto_merge_with_failed_start_path(state, workspace, guard, None).await
}

async fn restore_guarded_auto_merge_with_failed_start_path(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    failed_start_working_directory: Option<&std::path::Path>,
) -> AppResult<()> {
    let current_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Agent conversation workspace not found".to_string()))?;
    if !current_workspace.pr_auto_merge_desired {
        return cancel_guard_without_restoring(
            state,
            &current_workspace,
            guard,
            "GitHub auto-merge will remain disabled because workspace supervision was turned off.",
        )
        .await;
    }
    if guarded_pr_is_terminal(&current_workspace, guard.pr_number) {
        return cancel_guard_without_restoring(
            state,
            &current_workspace,
            guard,
            "GitHub auto-merge will remain disabled because the guarded pull request is terminal.",
        )
        .await;
    }

    let workspace_publish_requires_proof = guard.target_scope
        == AgentWorkspaceReviewTargetScope::WorkspaceDelta
        && matches!(
            guard.status,
            AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish
                | AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
        );
    if workspace_publish_requires_proof {
        let monitor = load_or_create_monitor(state, &current_workspace).await?;
        if !workspace_delta_publish_proves_guard(state, &monitor, &current_workspace, guard).await?
        {
            return Ok(());
        }
    }

    let restoring = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        last_error: None,
        ..guard.clone()
    };
    if !state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(guard.clone()),
            Some(restoring.clone()),
        )
        .await?
    {
        return Ok(());
    }
    let monitor = load_or_create_monitor(state, &current_workspace).await?;
    let working_directory = if let Some(working_directory) = failed_start_working_directory {
        working_directory.to_path_buf()
    } else if workspace_publish_requires_proof {
        if !workspace_delta_publish_proves_guard(state, &monitor, &current_workspace, &restoring)
            .await?
        {
            let _ = state
                .agent_conversation_workspace_repo
                .compare_and_set_workspace_review_auto_merge_guard(
                    &workspace.conversation_id,
                    Some(restoring),
                    Some(guard.clone()),
                )
                .await?;
            return Ok(());
        }
        resolve_workspace_working_directory(state, &current_workspace).await?
    } else {
        let target = resolve_current_target(state, &current_workspace).await?;
        let Some(target) = target else {
            return cancel_guard_without_restoring(
                state,
                &current_workspace,
                &restoring,
                "Workspace Review target no longer exists, so GitHub auto-merge was not restored.",
            )
            .await;
        };
        if !guard_matches_target(&restoring, &current_workspace, &target) {
            return cancel_guard_without_restoring(
                state,
                &current_workspace,
                &restoring,
                "Workspace Review target changed, so GitHub auto-merge was not restored.",
            )
            .await;
        }
        target.working_directory
    };
    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::Infrastructure(
            "GitHub integration became unavailable before auto-merge restoration".to_string(),
        )
    })?;
    let pr_status = match github
        .check_pr_status(&working_directory, restoring.pr_number)
        .await
    {
        Ok(status) => status,
        Err(error) => {
            return mark_restore_failed(state, &current_workspace, restoring, error.to_string())
                .await;
        }
    };
    if pr_status_is_terminal(&pr_status) {
        return cancel_guard_without_restoring(
            state,
            &current_workspace,
            &restoring,
            "GitHub auto-merge will remain disabled because the guarded pull request is terminal.",
        )
        .await;
    }
    if let Err(error) = github
        .enable_pr_auto_merge(
            &working_directory,
            restoring.pr_number,
            &restoring.merge_method,
        )
        .await
    {
        return mark_restore_failed(state, &current_workspace, restoring, error.to_string()).await;
    }
    let confirmed = github
        .fetch_pr_health(&working_directory, restoring.pr_number)
        .await
        .map(|health| health.auto_merge_request.is_some());
    match confirmed {
        Ok(true) => {}
        Ok(false) => {
            return mark_restore_failed(
                state,
                &current_workspace,
                restoring,
                "GitHub did not report auto-merge as enabled after restoration".to_string(),
            )
            .await;
        }
        Err(error) => {
            return mark_restore_failed(state, &current_workspace, restoring, error.to_string())
                .await;
        }
    }
    let finalized = if failed_start_working_directory.is_some() {
        finalize_confirmed_failed_start_auto_merge_restore(
            state,
            &current_workspace,
            &restoring,
            &working_directory,
        )
        .await?
    } else {
        finalize_confirmed_auto_merge_restore(
            state,
            &current_workspace,
            &restoring,
            &working_directory,
        )
        .await?
    };
    let _ = finalized;
    Ok(())
}

async fn finalize_confirmed_auto_merge_restore(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: &AgentWorkspaceReviewAutoMergeGuard,
    working_directory: &std::path::Path,
) -> AppResult<bool> {
    finalize_confirmed_auto_merge_restore_with_authority(
        state,
        workspace,
        restoring,
        working_directory,
        false,
    )
    .await
}

async fn finalize_confirmed_failed_start_auto_merge_restore(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: &AgentWorkspaceReviewAutoMergeGuard,
    working_directory: &std::path::Path,
) -> AppResult<bool> {
    finalize_confirmed_auto_merge_restore_with_authority(
        state,
        workspace,
        restoring,
        working_directory,
        true,
    )
    .await
}

async fn finalize_confirmed_auto_merge_restore_with_authority(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: &AgentWorkspaceReviewAutoMergeGuard,
    working_directory: &std::path::Path,
    failed_start_authority: bool,
) -> AppResult<bool> {
    let authority_is_current = match if failed_start_authority {
        failed_start_restoration_authority_is_current(state, workspace, restoring).await
    } else {
        restoration_authority_is_current(state, workspace, restoring).await
    } {
        Ok(is_current) => is_current,
        Err(error) => {
            re_pause_auto_merge_after_restore_finalization_error(
                state,
                workspace,
                restoring,
                working_directory,
                &error.to_string(),
            )
            .await?;
            return Err(error);
        }
    };
    if !authority_is_current {
        re_pause_auto_merge_after_lost_restore_authority(
            state,
            workspace,
            restoring,
            working_directory,
        )
        .await?;
        return Ok(false);
    }
    let completed = match state
        .agent_conversation_workspace_repo
        .complete_workspace_review_auto_merge_restore(&workspace.conversation_id, restoring.clone())
        .await
    {
        Ok(completed) => completed,
        Err(error) => {
            re_pause_auto_merge_after_restore_finalization_error(
                state,
                workspace,
                restoring,
                working_directory,
                &error.to_string(),
            )
            .await?;
            return Err(error);
        }
    };
    if !completed {
        re_pause_auto_merge_after_lost_restore_authority(
            state,
            workspace,
            restoring,
            working_directory,
        )
        .await?;
        return Ok(false);
    }
    append_auto_merge_guard_event(
        state,
        workspace,
        restoring,
        "restored",
        "succeeded",
        REVIEW_AUTO_MERGE_RESTORED_SUMMARY,
    )
    .await;
    Ok(true)
}

async fn failed_start_restoration_authority_is_current(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: &AgentWorkspaceReviewAutoMergeGuard,
) -> AppResult<bool> {
    let Some(current_workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
    else {
        return Ok(false);
    };
    if !current_workspace.pr_auto_merge_desired
        || guarded_pr_is_terminal(&current_workspace, restoring.pr_number)
    {
        return Ok(false);
    }
    let monitor = load_or_create_monitor(state, &current_workspace).await?;
    Ok(monitor.auto_merge_guard.as_ref() == Some(restoring))
}

async fn re_pause_auto_merge_after_restore_finalization_error(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: &AgentWorkspaceReviewAutoMergeGuard,
    working_directory: &std::path::Path,
    finalization_error: &str,
) -> AppResult<()> {
    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::Infrastructure(
            "GitHub integration became unavailable while re-pausing auto-merge".to_string(),
        )
    })?;
    github
        .disable_pr_auto_merge(working_directory, restoring.pr_number)
        .await?;
    let Some(current_workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
    else {
        return Ok(());
    };
    let monitor = load_or_create_monitor(state, &current_workspace).await?;
    if monitor.auto_merge_guard.as_ref() == Some(restoring) {
        mark_restore_failed(
            state,
            &current_workspace,
            restoring.clone(),
            finalization_error.to_string(),
        )
        .await?;
    }
    Ok(())
}

async fn restoration_authority_is_current(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: &AgentWorkspaceReviewAutoMergeGuard,
) -> AppResult<bool> {
    let Some(current_workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
    else {
        return Ok(false);
    };
    if !current_workspace.pr_auto_merge_desired
        || guarded_pr_is_terminal(&current_workspace, restoring.pr_number)
    {
        return Ok(false);
    }
    let monitor = load_or_create_monitor(state, &current_workspace).await?;
    if monitor.auto_merge_guard.as_ref() != Some(restoring) {
        return Ok(false);
    }
    if restoring.target_scope == AgentWorkspaceReviewTargetScope::WorkspaceDelta
        && monitor.review_outcome == AgentWorkspaceReviewOutcome::Passed
    {
        return workspace_delta_publish_proves_guard(
            state,
            &monitor,
            &current_workspace,
            restoring,
        )
        .await;
    }
    let Some(target) = resolve_current_target(state, &current_workspace).await? else {
        return Ok(false);
    };
    Ok(
        review_target_pr_number(&current_workspace, &target) == Some(restoring.pr_number)
            && guard_matches_target(restoring, &current_workspace, &target),
    )
}

async fn re_pause_auto_merge_after_lost_restore_authority(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: &AgentWorkspaceReviewAutoMergeGuard,
    working_directory: &std::path::Path,
) -> AppResult<()> {
    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::Infrastructure(
            "GitHub integration became unavailable while re-pausing auto-merge".to_string(),
        )
    })?;
    if let Err(error) = github
        .disable_pr_auto_merge(working_directory, restoring.pr_number)
        .await
    {
        let current_workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await?;
        if let Some(current_workspace) = current_workspace {
            let monitor = load_or_create_monitor(state, &current_workspace).await?;
            if monitor.auto_merge_guard.as_ref() == Some(restoring) {
                mark_restore_failed(
                    state,
                    &current_workspace,
                    restoring.clone(),
                    error.to_string(),
                )
                .await?;
            }
        }
        return Err(error);
    }
    let Some(current_workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
    else {
        return Ok(());
    };
    let monitor = load_or_create_monitor(state, &current_workspace).await?;
    if monitor.auto_merge_guard.as_ref() == Some(restoring) {
        cancel_guard_without_restoring(
            state,
            &current_workspace,
            restoring,
            "GitHub auto-merge will remain disabled because restore authority changed.",
        )
        .await?;
    }
    Ok(())
}

async fn mark_restore_failed(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: AgentWorkspaceReviewAutoMergeGuard,
    error: String,
) -> AppResult<()> {
    let failed = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed,
        last_error: Some(error),
        ..restoring.clone()
    };
    let changed = state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(restoring),
            Some(failed.clone()),
        )
        .await?;
    if !changed {
        return Ok(());
    }
    state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            &workspace.conversation_id,
            Some(false),
            Some("review_restore_failed"),
            Some(REVIEW_AUTO_MERGE_RESTORE_FAILED_SUMMARY),
        )
        .await?;
    append_auto_merge_guard_event(
        state,
        workspace,
        &failed,
        "restore_failed",
        "failed",
        REVIEW_AUTO_MERGE_RESTORE_FAILED_SUMMARY,
    )
    .await;
    Ok(())
}

async fn append_auto_merge_guard_event(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    event: &str,
    status: &str,
    summary: &str,
) {
    let classification = format!(
        "workspace_review_auto_merge:{event}:{}:{}",
        guard.pr_number, guard.diff_fingerprint
    );
    append_auto_merge_guard_event_with_classification(
        state,
        workspace,
        status,
        summary,
        classification,
    )
    .await;
}

async fn append_auto_merge_guard_event_with_classification(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    status: &str,
    summary: &str,
    classification: String,
) {
    let already_recorded = match state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
    {
        Ok(events) => events
            .iter()
            .any(|entry| entry.classification.as_deref() == Some(classification.as_str())),
        Err(error) => {
            warn!(
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Failed to inspect workspace Review auto-merge events"
            );
            return;
        }
    };
    if already_recorded {
        return;
    }
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id.clone(),
            "workspace_review_auto_merge",
            status,
            summary,
            Some(classification),
        ))
        .await
    {
        warn!(
            conversation_id = %workspace.conversation_id,
            error = %error,
            "Failed to record workspace Review auto-merge event"
        );
    }
}
