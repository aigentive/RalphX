use std::{path::Path, str::FromStr};

use chrono::Utc;

use crate::application::agent_conversation_archive::archive_agent_conversation_for_state;
use crate::application::automation::api::{
    automation_service_for_state, automation_transition_service_for_state,
};
use crate::application::automation::transition::{
    AutomationTransitionService, AUTOMATION_RUN_UPDATED_EVENT, AUTOMATION_UPDATED_EVENT,
};
use crate::application::git_service::GitService;
use crate::application::AppState;
use crate::domain::entities::{
    ArtifactId, AutomationId, AutomationJudgeState, AutomationRunId, AutomationRunStatus,
    AutomationStatus, ChatConversation, IdeationAnalysisBaseRefKind, IdeationSessionFlow, Project,
};
use crate::domain::repositories::PlanArtifactApprovalRepository;
use crate::domain::services::kill_worktree_processes_async;
use crate::domain::state_machine::transition_handler::cleanup_helpers::remove_worktree_fast;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::sqlite::SqlitePlanArtifactApprovalRepository;
use crate::utils::path_safety::validate_absolute_non_root_path;

pub(crate) async fn delete_automation_run_with_archive(
    state: &AppState,
    automation_id: &AutomationId,
    run_id: &AutomationRunId,
) -> AppResult<()> {
    let automation = state
        .automation_repo
        .get_by_id(automation_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("automation {automation_id} not found")))?;
    let mut run = state
        .automation_run_repo
        .list_for_automation(automation_id)
        .await?
        .into_iter()
        .find(|run| run.id == *run_id)
        .ok_or_else(|| AppError::NotFound(format!("automation run {run_id} not found")))?;
    ensure_latest_run(state, automation_id, run_id).await?;
    if run.judge_state == AutomationJudgeState::InProgress
        && run
            .judge_lease_expires_at
            .is_some_and(|expires_at| expires_at > Utc::now())
    {
        return Err(AppError::Validation(
            "judge is finalizing; retry shortly".to_string(),
        ));
    }
    let service = automation_service_for_state(state);
    if run.status == AutomationRunStatus::Running {
        run = service.cancel_run(automation_id, run_id).await?;
    }
    if !matches!(
        run.status,
        AutomationRunStatus::AgentFailed | AutomationRunStatus::Cancelled
    ) {
        return Err(AppError::Conflict(format!(
            "run status {} cannot be deleted",
            run.status.as_str()
        )));
    }
    ensure_latest_run(state, automation_id, run_id).await?;

    let mut workspace_project = None;
    if let Some(conversation_id) = run.conversation_id.as_ref() {
        let conversation = state
            .chat_conversation_repo
            .get_by_id(conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("conversation {conversation_id} not found"))
            })?;
        if conversation.automation_run_id.as_ref() != Some(run_id) {
            return Err(AppError::Conflict(
                "run conversation ownership changed before delete".to_string(),
            ));
        }
        if let Some(workspace) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        {
            let project = state
                .project_repo
                .get_by_id(&workspace.project_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!("project {} not found", workspace.project_id))
                })?;
            workspace_project = Some((workspace, project));
        }
    }
    let branch = workspace_project
        .as_ref()
        .map(|(workspace, _)| workspace.branch_name.trim())
        .filter(|branch| !branch.is_empty())
        .or_else(|| {
            run.branch_name
                .as_deref()
                .map(str::trim)
                .filter(|branch| !branch.is_empty())
        })
        .map(str::to_string);
    let branch_project: Option<Result<Project, String>> = match branch.as_ref() {
        None => None,
        Some(_) => match workspace_project.as_ref() {
            Some((_, project)) => Some(Ok(project.clone())),
            None => Some(
                match state.project_repo.get_by_id(&automation.project_id).await {
                    Ok(Some(project)) => Ok(project),
                    Ok(None) => Err(format!("project {} was not found", automation.project_id)),
                    Err(error) => Err(format!(
                        "project {} lookup failed: {error}",
                        automation.project_id
                    )),
                },
            ),
        },
    };

    if state
        .automation_run_repo
        .delete_run_if_deletable(automation_id, run_id)
        .await?
        != 1
    {
        return Err(AppError::Conflict(
            "run is no longer the latest deletable run".to_string(),
        ));
    }

    if let Some(conversation_id) = run.conversation_id.as_ref() {
        if let Err(error) = archive_agent_conversation_for_state(conversation_id, state, true).await
        {
            tracing::warn!(
                %error,
                %conversation_id,
                "delete_automation_run: conversation archive failed; continuing"
            );
        }
    }
    if let Some((workspace, project)) = workspace_project.as_ref() {
        if !workspace.worktree_path.trim().is_empty() {
            cleanup_run_worktree(
                Path::new(&workspace.worktree_path),
                Path::new(&project.working_directory),
            )
            .await;
        }
    }
    if let (Some(branch), Some(project)) = (branch.as_deref(), branch_project) {
        match project {
            Ok(project) => cleanup_run_branches(state, &project, branch).await,
            Err(reason) => {
                tracing::warn!(
                    branch,
                    reason,
                    "delete_automation_run: branch cleanup skipped because project could not be resolved"
                );
            }
        };
    }
    service
        .sync_goal_items_for_closed_run_without_successor(automation_id)
        .await;
    let automation_id = automation_id.as_str();
    let run_id = run_id.as_str();
    state.events.emit(
        AUTOMATION_RUN_UPDATED_EVENT,
        serde_json::json!({"automation_id": automation_id, "automationId": automation_id, "run_id": run_id, "runId": run_id}),
    );
    state.events.emit(
        AUTOMATION_UPDATED_EVENT,
        serde_json::json!({"automation_id": automation_id, "automationId": automation_id}),
    );
    Ok(())
}

async fn ensure_latest_run(
    state: &AppState,
    automation_id: &AutomationId,
    run_id: &AutomationRunId,
) -> AppResult<()> {
    if state
        .automation_run_repo
        .latest_for_automation(automation_id)
        .await?
        .is_some_and(|run| run.id == *run_id)
    {
        return Ok(());
    }
    Err(AppError::Conflict(
        "only the latest run can be deleted".to_string(),
    ))
}

async fn cleanup_run_worktree(worktree: &Path, repo: &Path) {
    let (Ok(worktree), Ok(repo)) = (
        validate_absolute_non_root_path(worktree, "automation worktree"),
        validate_absolute_non_root_path(repo, "automation project"),
    ) else {
        tracing::warn!("delete_automation_run: unsafe worktree path rejected");
        return;
    };
    kill_worktree_processes_async(
        &worktree,
        git_runtime_config().worktree_lsof_timeout_secs,
        true,
    )
    .await;
    if let Err(error) = remove_worktree_fast(&worktree, &repo).await {
        tracing::warn!(%error, "delete_automation_run: worktree cleanup failed; continuing");
    }
}

async fn cleanup_run_branches(state: &AppState, project: &Project, branch: &str) {
    if branch.eq_ignore_ascii_case(project.base_branch_or_default())
        || branch.eq_ignore_ascii_case("main")
        || branch.eq_ignore_ascii_case("master")
    {
        tracing::warn!(
            branch,
            base_branch = project.base_branch_or_default(),
            "delete_automation_run: protected base branch cleanup skipped"
        );
        return;
    }
    let repo = Path::new(&project.working_directory);
    let Ok(repo) = validate_absolute_non_root_path(repo, "automation project") else {
        tracing::warn!(branch, "unsafe run-delete branch path; skipping");
        return;
    };
    if let Err(error) = GitService::delete_branch(&repo, branch, true).await {
        tracing::warn!(branch, %error, "delete_automation_run: local branch cleanup failed; continuing");
    }
    if let Some(github) = state.github_service.as_ref() {
        if let Err(error) = github.delete_remote_branch(&repo, branch).await {
            tracing::warn!(branch, %error, "delete_automation_run: remote branch cleanup failed; continuing");
        }
    }
}

/// Archive an automation's durable history and hard-delete its bookkeeping rows.
///
/// This is the application-level composition for automation deletion. It lives
/// here (not on `AutomationService`) because the conversation-archive
/// orchestrator takes `&AppState`, whereas `AutomationService` holds only repos
/// (critic G9). Ordering is fail-closed — destructive last — per the delete
/// spec:
///
/// 1. Gate on `Draft | Completed | Stopped` and the judge-lease predicate (E1/E2).
/// 2. If Draft, claim deletion authority by CAS `Draft -> Stopped` BEFORE any
///    side effect; a concurrent `finalize` then loses the race cleanly and no
///    archive ever commits against a survivor (E1).
/// 3. Archive every attached conversation (setup + runs) via the shared
///    orchestrator; the first failure aborts before any row deletion, and
///    already-archived conversations are skipped so a retry does not re-fire
///    stop-agent / close-PR side effects (E4/E5).
/// 4. Archive the linked spec artifact — never hard-deleted (E8); warn + continue.
/// 5. Hard-delete the automation + runs + attachments + context refs and emit
///    `AutomationDeleted` (E3/E6/E7/E9).
///
/// # Errors
///
/// Returns `NotFound` for an unknown automation, `Validation` when the status or
/// a live judge lease forbids deletion, `Conflict` when a concurrent finalize
/// wins the Draft CAS, or an `Infrastructure` error (naming the conversation)
/// when a conversation archive fails before any rows were deleted.
pub async fn delete_automation_with_archive(state: &AppState, id: &AutomationId) -> AppResult<()> {
    let automation = state
        .automation_repo
        .get_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("automation {} not found", id.as_str())))?;

    if !matches!(
        automation.status,
        AutomationStatus::Draft | AutomationStatus::Completed | AutomationStatus::Stopped
    ) {
        return Err(AppError::Validation(
            "only draft, completed, or stopped automations can be deleted".to_string(),
        ));
    }

    // Judge-lease gate: only a *live* finalizing judge blocks deletion. A crashed
    // judge (InProgress with a NULL or expired lease, lazily self-healed by the
    // scheduler) must NOT block, otherwise delete would be permanently wedged (E2).
    if let Some(latest) = state.automation_run_repo.latest_for_automation(id).await? {
        let now = Utc::now();
        if latest.judge_state == AutomationJudgeState::InProgress
            && latest
                .judge_lease_expires_at
                .is_some_and(|expires_at| expires_at > now)
        {
            return Err(AppError::Validation(
                "judge is finalizing; retry shortly".to_string(),
            ));
        }
    }

    // Claim deletion authority for drafts BEFORE any archive side effect (E1). The
    // CAS is scoped to `Draft -> Stopped`; if a concurrent finalize already moved
    // the automation to Active the swap affects zero rows and we abort here with
    // nothing archived.
    if automation.status == AutomationStatus::Draft {
        let claimed = automation_transition_service(state)
            .transition_automation_status(
                id,
                AutomationStatus::Draft,
                AutomationStatus::Stopped,
                None,
                None,
            )
            .await?;
        if !claimed {
            return Err(AppError::Conflict(format!(
                "automation {} status changed before delete could claim authority",
                id.as_str()
            )));
        }
    }

    // Archive every attached conversation that is not already archived (E4/E5).
    // `list_by_automation_id` re-returns archived rows on retry, and `archive()`
    // itself is a no-op on re-run, but stop-agent / close-PR side effects would
    // still fire — so filter archived rows out here (critic G8).
    let conversations = state
        .chat_conversation_repo
        .list_by_automation_id(id)
        .await?;
    for conversation in &conversations {
        if conversation.archived_at.is_some() {
            continue;
        }
        archive_agent_conversation_for_state(&conversation.id, state, false)
            .await
            .map_err(|error| {
                AppError::Infrastructure(format!(
                    "failed to archive automation conversation {}: {error}",
                    conversation.id.as_str()
                ))
            })?;
    }
    cleanup_plan_gate_artifacts_for_run_conversations(state, id, &conversations).await?;

    // Archive the linked spec artifact — never hard-deleted; it is versioned and
    // may be `derived_from`-linked (E8). Warn + continue on failure.
    if let Some(spec_artifact_id) = automation
        .spec_artifact_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let artifact_id = ArtifactId::from_string(spec_artifact_id.to_string());
        if let Err(error) = state.artifact_repo.archive(&artifact_id).await {
            tracing::warn!(
                automation_id = id.as_str(),
                spec_artifact_id,
                error = %error,
                "delete_automation_with_archive: failed to archive spec artifact; continuing"
            );
        }
    }

    // Remote-branch cleanup (B4): best-effort delete of the automation-owned base
    // branch we newly pushed to origin (integration-branch model). Gated to
    // automation-owned local-branch bases; per-run pr_head branches are NOT touched
    // here. Fail-open — the branch may never have been pushed (automation never
    // published), and `delete_remote_branch` already no-ops on an absent remote, so
    // delete must still succeed for never-published automations. The base ref is
    // read from the `automation` row loaded above, before any row is deleted.
    cleanup_automation_remote_base_branch(state, &automation).await;

    // Destructive last: hard-delete the bookkeeping rows and emit
    // `AutomationDeleted` (the row-delete core owns the event so `project_id` is
    // captured before the row is gone).
    automation_service_for_state(state).delete(id).await
}

async fn cleanup_plan_gate_artifacts_for_run_conversations(
    state: &AppState,
    automation_id: &AutomationId,
    conversations: &[ChatConversation],
) -> AppResult<()> {
    let approval_repo = SqlitePlanArtifactApprovalRepository::new(state.db.clone());
    for conversation in conversations
        .iter()
        .filter(|conversation| conversation.automation_run_id.is_some())
    {
        let Some(workspace) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation.id)
            .await?
        else {
            continue;
        };
        let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
            continue;
        };
        let Some(session) = state.ideation_session_repo.get_by_id(session_id).await? else {
            approval_repo.delete_by_session(session_id).await?;
            continue;
        };
        if session.session_flow != IdeationSessionFlow::Planning {
            continue;
        }
        if let Some(plan_artifact_id) = session.plan_artifact_id.as_ref() {
            archive_plan_artifact_chain(
                state,
                automation_id,
                session_id.as_str(),
                plan_artifact_id,
            )
            .await;
        }
        if let Some(plan_blueprint_artifact_id) = session.plan_blueprint_artifact_id.as_ref() {
            archive_plan_artifact_chain(
                state,
                automation_id,
                session_id.as_str(),
                plan_blueprint_artifact_id,
            )
            .await;
        }
        approval_repo.delete_by_session(session_id).await?;
        state.ideation_session_repo.delete(session_id).await?;
    }
    Ok(())
}

async fn archive_plan_artifact_chain(
    state: &AppState,
    automation_id: &AutomationId,
    session_id: &str,
    plan_artifact_id: &ArtifactId,
) {
    let latest_id = match state
        .artifact_repo
        .resolve_latest_artifact_id(plan_artifact_id)
        .await
    {
        Ok(latest_id) => latest_id,
        Err(error) => {
            tracing::warn!(
                automation_id = automation_id.as_str(),
                session_id,
                plan_artifact_id = plan_artifact_id.as_str(),
                error = %error,
                "delete_automation_with_archive: failed to resolve latest plan artifact; archiving known artifact"
            );
            plan_artifact_id.clone()
        }
    };

    let artifact_ids = match state.artifact_repo.get_version_history(&latest_id).await {
        Ok(history) if !history.is_empty() => {
            history.into_iter().map(|summary| summary.id).collect()
        }
        Ok(_) => vec![latest_id],
        Err(error) => {
            tracing::warn!(
                automation_id = automation_id.as_str(),
                session_id,
                plan_artifact_id = plan_artifact_id.as_str(),
                error = %error,
                "delete_automation_with_archive: failed to read plan artifact version chain; archiving known artifact"
            );
            vec![plan_artifact_id.clone()]
        }
    };

    for artifact_id in artifact_ids {
        if let Err(error) = state.artifact_repo.archive(&artifact_id).await {
            tracing::warn!(
                automation_id = automation_id.as_str(),
                session_id,
                plan_artifact_id = artifact_id.as_str(),
                error = %error,
                "delete_automation_with_archive: failed to archive plan artifact; continuing"
            );
        }
    }
}

/// Best-effort deletion of an automation's origin base branch. Never fails the
/// caller — every error path warns and returns. See B4 in the integration-branch
/// spec.
async fn cleanup_automation_remote_base_branch(
    state: &AppState,
    automation: &crate::domain::entities::Automation,
) {
    let is_local_branch = IdeationAnalysisBaseRefKind::from_str(automation.base_ref_kind.trim())
        .map(|kind| kind == IdeationAnalysisBaseRefKind::LocalBranch)
        .unwrap_or(false);
    if !is_local_branch {
        return;
    }
    let base_ref = automation.base_ref.trim();
    if base_ref.is_empty() {
        return;
    }
    let Some(github) = state.github_service.as_ref() else {
        return;
    };
    let project = match state.project_repo.get_by_id(&automation.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            tracing::warn!(
                automation_id = automation.id.as_str(),
                project_id = automation.project_id.as_str(),
                "delete_automation_with_archive: project missing; skipping remote base branch cleanup"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(
                automation_id = automation.id.as_str(),
                error = %error,
                "delete_automation_with_archive: failed to load project for remote base branch cleanup; continuing"
            );
            return;
        }
    };
    let working_dir = std::path::Path::new(&project.working_directory);
    if let Err(error) = github.delete_remote_branch(working_dir, base_ref).await {
        tracing::warn!(
            automation_id = automation.id.as_str(),
            base_ref,
            error = %error,
            "delete_automation_with_archive: failed to delete remote automation base branch; continuing"
        );
    }
}

fn automation_transition_service(state: &AppState) -> AutomationTransitionService {
    automation_transition_service_for_state(state)
}
