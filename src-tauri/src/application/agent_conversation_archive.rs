use std::path::Path;
use std::sync::Arc;

use crate::application::chat_service::ChatService;
use crate::application::task_cleanup_service::{StopMode, TaskCleanupService};
use crate::application::AppState;
use crate::domain::entities::plan_branch::PrStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatContextType, ChatConversationId, ExecutionPlan, ExecutionPlanStatus, PlanBranch,
    PlanBranchStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectivePrSource {
    PlanBranch,
    Workspace,
}

#[derive(Debug, Clone)]
struct EffectivePrTarget {
    number: i64,
    url: Option<String>,
    source: EffectivePrSource,
    is_open: bool,
}

/// Archive an agent conversation and clean up linked ideation execution state.
pub async fn archive_agent_conversation_for_state(
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> Result<(), String> {
    let _conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    stop_project_agent(conversation_id, state).await?;

    if let Some(workspace) = workspace.as_ref() {
        cleanup_ideation_execution_workspace(workspace, state).await?;
        close_effective_pr_if_open(conversation_id, workspace, state).await?;
    }

    state
        .chat_conversation_repo
        .archive(conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    if workspace.is_some() {
        state
            .agent_conversation_workspace_repo
            .update_status(conversation_id, AgentConversationWorkspaceStatus::Archived)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Close the effective PR for an agent workspace, using linked PlanBranch PRs first.
pub async fn close_agent_workspace_pr_for_state(
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> Result<(), String> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            )
        })?;
    let linked_plan_branch = load_linked_plan_branch_for_pr(&workspace, state).await?;
    let target = resolve_effective_pr(&workspace, linked_plan_branch.as_ref())
        .ok_or_else(|| "No PR associated with this workspace".to_string())?;

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;

    close_remote_pr(
        state,
        Path::new(&project.working_directory),
        target.number,
        "close_agent_workspace_pr",
    )
    .await;
    mark_effective_pr_closed(
        conversation_id,
        &workspace,
        linked_plan_branch.as_ref(),
        &target,
        state,
    )
    .await
}

/// Strictly close the effective PR before replacing an implementation attempt.
///
/// Unlike archive cleanup, restart cannot continue with local state that claims an
/// open PR was closed when the remote operation failed. Once remote closure succeeds,
/// clear both active PR pointers so the replacement implementation cannot reuse it.
pub(crate) async fn close_agent_workspace_pr_for_restart(
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: &PlanBranch,
    state: &AppState,
) -> crate::error::AppResult<()> {
    let target = resolve_effective_pr(workspace, Some(linked_plan_branch));
    if let Some(target) = target.as_ref().filter(|target| target.is_open) {
        let project = state
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .map_err(|error| crate::error::AppError::Database(error.to_string()))?
            .ok_or_else(|| {
                crate::error::AppError::NotFound(format!(
                    "Project not found while closing PR {} for restart: {}",
                    target.number, workspace.project_id
                ))
            })?;
        let project_path = crate::utils::path_safety::validate_absolute_non_root_path(
            Path::new(&project.working_directory),
            "project checkout",
        )?;
        let github_svc = state.github_service.as_ref().ok_or_else(|| {
            crate::error::AppError::Validation(format!(
                "Restart cannot close existing PR {} because GitHub integration is unavailable",
                target.number
            ))
        })?;
        github_svc
            .close_pr(&project_path, target.number)
            .await
            .map_err(|error| {
                crate::error::AppError::Infrastructure(format!(
                    "Restart could not close existing PR {}: {}",
                    target.number, error
                ))
            })?;
    }

    state
        .plan_branch_repo
        .clear_pr_info(&linked_plan_branch.id)
        .await
        .map_err(|error| crate::error::AppError::Database(error.to_string()))?;
    state
        .agent_conversation_workspace_repo
        .update_publication(&workspace.conversation_id, None, None, None, None)
        .await
        .map_err(|error| crate::error::AppError::Database(error.to_string()))
}

async fn stop_project_agent(
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> Result<(), String> {
    let chat_service = state.build_chat_service();
    let conversation_id_str = conversation_id.as_str();
    chat_service
        .stop_agent(ChatContextType::Project, &conversation_id_str)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn cleanup_ideation_execution_workspace(
    workspace: &AgentConversationWorkspace,
    state: &AppState,
) -> Result<(), String> {
    if workspace.mode != AgentConversationWorkspaceMode::Ideation {
        return Ok(());
    }

    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        return Ok(());
    };

    let plan_branch = state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Linked plan branch not found: {}", plan_branch_id))?;

    if let Some(execution_plan) = resolve_current_execution_plan(&plan_branch, state).await? {
        archive_current_execution_tasks(workspace, &execution_plan, state).await?;
        if execution_plan.status == ExecutionPlanStatus::Active {
            state
                .execution_plan_repo
                .mark_superseded(&execution_plan.id)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    if plan_branch.status != PlanBranchStatus::Abandoned {
        state
            .plan_branch_repo
            .update_status(&plan_branch.id, PlanBranchStatus::Abandoned)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

async fn resolve_current_execution_plan(
    plan_branch: &PlanBranch,
    state: &AppState,
) -> Result<Option<ExecutionPlan>, String> {
    if let Some(execution_plan_id) = plan_branch.execution_plan_id.as_ref() {
        return state
            .execution_plan_repo
            .get_by_id(execution_plan_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "Linked execution plan not found: {}",
                    execution_plan_id.as_str()
                )
            })
            .map(Some);
    }

    state
        .execution_plan_repo
        .get_active_for_session(&plan_branch.session_id)
        .await
        .map_err(|e| e.to_string())
}

async fn archive_current_execution_tasks(
    workspace: &AgentConversationWorkspace,
    execution_plan: &ExecutionPlan,
    state: &AppState,
) -> Result<(), String> {
    let tasks = state
        .task_repo
        .list_paginated(
            &workspace.project_id,
            None,
            0,
            10_000,
            false,
            None,
            Some(execution_plan.id.as_str()),
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    if tasks.is_empty() {
        return Ok(());
    }

    let cleanup_service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        state.app_handle.clone(),
    )
    .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry));

    let report = cleanup_service
        .cleanup_tasks(&tasks, StopMode::DirectStop, true)
        .await;

    if !report.errors.is_empty() {
        return Err(format!(
            "Failed to archive current execution tasks: {}",
            report.errors.join("; ")
        ));
    }

    Ok(())
}

async fn close_effective_pr_if_open(
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    state: &AppState,
) -> Result<(), String> {
    let linked_plan_branch = load_linked_plan_branch_for_pr(workspace, state).await?;
    let Some(target) = resolve_effective_pr(workspace, linked_plan_branch.as_ref()) else {
        return Ok(());
    };

    if !target.is_open {
        return Ok(());
    }

    match state.project_repo.get_by_id(&workspace.project_id).await {
        Ok(Some(project)) => {
            close_remote_pr(
                state,
                Path::new(&project.working_directory),
                target.number,
                "archive_agent_conversation",
            )
            .await;
        }
        Ok(None) => {
            tracing::warn!(
                project_id = workspace.project_id.as_str(),
                pr_number = target.number,
                "archive_agent_conversation: project not found while closing PR; continuing with local status update"
            );
        }
        Err(error) => {
            tracing::warn!(
                project_id = workspace.project_id.as_str(),
                pr_number = target.number,
                error = %error,
                "archive_agent_conversation: failed to load project while closing PR; continuing with local status update"
            );
        }
    }

    mark_effective_pr_closed(
        conversation_id,
        workspace,
        linked_plan_branch.as_ref(),
        &target,
        state,
    )
    .await
}

async fn close_remote_pr(
    state: &AppState,
    working_dir: &Path,
    pr_number: i64,
    operation: &'static str,
) {
    if let Some(github_svc) = &state.github_service {
        if let Err(error) = github_svc.close_pr(working_dir, pr_number).await {
            tracing::warn!(
                pr_number,
                operation,
                error = %error,
                "failed to close PR on remote (continuing with local status update)"
            );
        }
    }
}

async fn load_linked_plan_branch_for_pr(
    workspace: &AgentConversationWorkspace,
    state: &AppState,
) -> Result<Option<PlanBranch>, String> {
    if workspace.mode != AgentConversationWorkspaceMode::Ideation {
        return Ok(None);
    }

    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        return Ok(None);
    };

    state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await
        .map_err(|e| e.to_string())
}

fn resolve_effective_pr(
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: Option<&PlanBranch>,
) -> Option<EffectivePrTarget> {
    if let Some(plan_branch) = linked_plan_branch {
        if let Some(number) = plan_branch.pr_number {
            return Some(EffectivePrTarget {
                number,
                url: plan_branch.pr_url.clone(),
                source: EffectivePrSource::PlanBranch,
                is_open: is_plan_branch_pr_open(plan_branch),
            });
        }
    }

    workspace
        .publication_pr_number
        .map(|number| EffectivePrTarget {
            number,
            url: workspace.publication_pr_url.clone(),
            source: EffectivePrSource::Workspace,
            is_open: is_workspace_pr_open(workspace.publication_pr_status.as_deref()),
        })
}

fn is_plan_branch_pr_open(plan_branch: &PlanBranch) -> bool {
    !matches!(
        plan_branch.pr_status.as_ref().map(PrStatus::to_db_string),
        Some("Closed" | "Merged")
    )
}

fn is_workspace_pr_open(status: Option<&str>) -> bool {
    !matches!(
        status,
        Some(value) if value.eq_ignore_ascii_case("closed") || value.eq_ignore_ascii_case("merged")
    )
}

async fn mark_effective_pr_closed(
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: Option<&PlanBranch>,
    target: &EffectivePrTarget,
    state: &AppState,
) -> Result<(), String> {
    if target.source == EffectivePrSource::PlanBranch {
        if let Some(plan_branch) = linked_plan_branch {
            state
                .plan_branch_repo
                .update_pr_status(&plan_branch.id, PrStatus::Closed)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    state
        .agent_conversation_workspace_repo
        .update_publication(
            conversation_id,
            Some(target.number),
            target.url.as_deref(),
            Some("closed"),
            workspace.publication_push_status.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::entities::{
        AgentConversationWorkspaceStatus, ArtifactId, ChatConversation, ExecutionPlanId,
        IdeationAnalysisBaseRefKind, IdeationSessionId, Project, Task,
    };
    use crate::domain::services::github_service::GithubServiceTrait;
    use crate::domain::services::RunningAgentKey;
    use crate::tests::mock_github_service::MockGithubService;

    async fn setup_archive_state(
        suffix: &str,
        mode: AgentConversationWorkspaceMode,
        pr_number: Option<i64>,
    ) -> (
        tempfile::TempDir,
        AppState,
        ChatConversationId,
        AgentConversationWorkspace,
        Arc<MockGithubService>,
    ) {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let project = Project::new(
            format!("Archive unit {suffix}"),
            temp.path().to_string_lossy().to_string(),
        );
        let conversation_id = ChatConversationId::new();
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            mode,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("base-sha".to_string()),
            format!("agent/archive-{suffix}"),
            temp.path().join("worktree").to_string_lossy().to_string(),
        );
        workspace.publication_pr_number = pr_number;
        workspace.publication_pr_url =
            pr_number.map(|number| format!("https://github.com/mock/repo/pull/{number}"));
        workspace.publication_pr_status = pr_number.map(|_| "open".to_string());

        let github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        let mut state = AppState::new_test();
        state.github_service = Some(github_trait);
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should be persisted");
        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should be persisted");
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should be persisted");

        (temp, state, conversation_id, workspace, github)
    }

    async fn create_linked_plan_branch(
        state: &AppState,
        conversation_id: &ChatConversationId,
        workspace: &AgentConversationWorkspace,
        suffix: &str,
        execution_plan: Option<&ExecutionPlan>,
    ) -> PlanBranch {
        let session_id = execution_plan
            .map(|plan| plan.session_id.clone())
            .unwrap_or_else(|| IdeationSessionId::from_string(format!("session-{suffix}")));
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string(format!("artifact-{suffix}")),
            session_id.clone(),
            workspace.project_id.clone(),
            format!("plan/{suffix}"),
            "main".to_string(),
        );
        plan_branch.execution_plan_id = execution_plan.map(|plan| plan.id.clone());
        let plan_branch_id = plan_branch.id.clone();
        let created = state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch should be created");
        state
            .agent_conversation_workspace_repo
            .update_links(conversation_id, Some(&session_id), Some(&plan_branch_id))
            .await
            .expect("workspace links should be updated");
        created
    }

    #[tokio::test]
    async fn archive_stops_project_agent_and_closes_workspace_pr() {
        let (_temp, state, conversation_id, _workspace, github) = setup_archive_state(
            "workspace-pr",
            AgentConversationWorkspaceMode::Edit,
            Some(42),
        )
        .await;
        let conversation_id_str = conversation_id.as_str();
        let running_key =
            RunningAgentKey::new(ChatContextType::Project.to_string(), &conversation_id_str);
        state
            .running_agent_registry
            .register(
                running_key.clone(),
                0,
                conversation_id_str,
                "run-workspace-pr".to_string(),
                None,
                None,
            )
            .await;

        archive_agent_conversation_for_state(&conversation_id, &state)
            .await
            .expect("archive should succeed");

        assert!(!state.running_agent_registry.is_running(&running_key).await);
        assert_eq!(github.state().close_pr_calls, 1);
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(workspace.status, AgentConversationWorkspaceStatus::Archived);
        assert_eq!(workspace.publication_pr_status.as_deref(), Some("closed"));
    }

    #[tokio::test]
    async fn archive_skips_remote_close_for_terminal_workspace_pr() {
        let (_temp, state, conversation_id, _workspace, github) =
            setup_archive_state("closed-pr", AgentConversationWorkspaceMode::Edit, Some(43)).await;
        state
            .agent_conversation_workspace_repo
            .update_publication(&conversation_id, Some(43), None, Some("closed"), None)
            .await
            .expect("publication update should succeed");

        archive_agent_conversation_for_state(&conversation_id, &state)
            .await
            .expect("archive should succeed");

        assert_eq!(github.state().close_pr_calls, 0);
    }

    #[tokio::test]
    async fn archive_ideation_workspace_cleans_current_execution_only() {
        let (_temp, state, conversation_id, workspace, _github) = setup_archive_state(
            "current-execution",
            AgentConversationWorkspaceMode::Ideation,
            None,
        )
        .await;
        let session_id = IdeationSessionId::from_string("session-current-execution".to_string());
        let current_plan = state
            .execution_plan_repo
            .create(ExecutionPlan::new(session_id.clone()))
            .await
            .expect("current plan should be created");
        let mut stale_plan = ExecutionPlan::new(session_id.clone());
        stale_plan.status = ExecutionPlanStatus::Superseded;
        let stale_plan = state
            .execution_plan_repo
            .create(stale_plan)
            .await
            .expect("stale plan should be created");
        let plan_branch = create_linked_plan_branch(
            &state,
            &conversation_id,
            &workspace,
            "current-execution",
            Some(&current_plan),
        )
        .await;

        let mut current_task = Task::new(workspace.project_id.clone(), "current task".to_string());
        current_task.ideation_session_id = Some(session_id.clone());
        current_task.execution_plan_id = Some(current_plan.id.clone());
        let current_task_id = current_task.id.clone();
        state
            .task_repo
            .create(current_task)
            .await
            .expect("current task should be created");

        let mut stale_task = Task::new(workspace.project_id.clone(), "stale task".to_string());
        stale_task.ideation_session_id = Some(session_id);
        stale_task.execution_plan_id = Some(stale_plan.id.clone());
        let stale_task_id = stale_task.id.clone();
        state
            .task_repo
            .create(stale_task)
            .await
            .expect("stale task should be created");

        archive_agent_conversation_for_state(&conversation_id, &state)
            .await
            .expect("archive should succeed");

        assert!(state
            .task_repo
            .get_by_id(&current_task_id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_some());
        assert!(state
            .task_repo
            .get_by_id(&stale_task_id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_none());
        assert_eq!(
            state
                .execution_plan_repo
                .get_by_id(&current_plan.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            ExecutionPlanStatus::Superseded
        );
        assert_eq!(
            state
                .plan_branch_repo
                .get_by_id(&plan_branch.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            PlanBranchStatus::Abandoned
        );
    }

    #[tokio::test]
    async fn archive_closes_plan_branch_pr_without_workspace_publication() {
        let (_temp, state, conversation_id, workspace, github) = setup_archive_state(
            "plan-branch-pr",
            AgentConversationWorkspaceMode::Ideation,
            None,
        )
        .await;
        let plan_branch =
            create_linked_plan_branch(&state, &conversation_id, &workspace, "plan-branch-pr", None)
                .await;
        state
            .plan_branch_repo
            .update_pr_info(
                &plan_branch.id,
                55,
                "https://github.com/mock/repo/pull/55".to_string(),
                PrStatus::Open,
                false,
            )
            .await
            .expect("plan branch pr update should succeed");

        archive_agent_conversation_for_state(&conversation_id, &state)
            .await
            .expect("archive should succeed");

        assert_eq!(github.state().close_pr_calls, 1);
        assert_eq!(
            state
                .plan_branch_repo
                .get_by_id(&plan_branch.id)
                .await
                .unwrap()
                .unwrap()
                .pr_status,
            Some(PrStatus::Closed)
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .unwrap()
                .unwrap()
                .publication_pr_number,
            Some(55)
        );
    }

    #[tokio::test]
    async fn explicit_close_uses_plan_branch_pr_before_workspace_publication() {
        let (_temp, state, conversation_id, workspace, github) = setup_archive_state(
            "explicit-close",
            AgentConversationWorkspaceMode::Ideation,
            Some(99),
        )
        .await;
        let plan_branch =
            create_linked_plan_branch(&state, &conversation_id, &workspace, "explicit-close", None)
                .await;
        state
            .plan_branch_repo
            .update_pr_info(
                &plan_branch.id,
                77,
                "https://github.com/mock/repo/pull/77".to_string(),
                PrStatus::Open,
                false,
            )
            .await
            .expect("plan branch pr update should succeed");

        close_agent_workspace_pr_for_state(&conversation_id, &state)
            .await
            .expect("close should succeed");

        assert_eq!(github.state().close_pr_calls, 1);
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(workspace.publication_pr_number, Some(77));
        assert_eq!(workspace.publication_pr_status.as_deref(), Some("closed"));
    }

    #[tokio::test]
    async fn archive_missing_linked_execution_plan_fails_before_archiving() {
        let (_temp, state, conversation_id, workspace, _github) = setup_archive_state(
            "missing-plan",
            AgentConversationWorkspaceMode::Ideation,
            None,
        )
        .await;
        let session_id = IdeationSessionId::from_string("session-missing-plan".to_string());
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-missing-plan"),
            session_id.clone(),
            workspace.project_id.clone(),
            "plan/missing-plan".to_string(),
            "main".to_string(),
        );
        plan_branch.execution_plan_id = Some(ExecutionPlanId::from_string(
            "missing-execution-plan".to_string(),
        ));
        let plan_branch_id = plan_branch.id.clone();
        state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch should be created");
        state
            .agent_conversation_workspace_repo
            .update_links(&conversation_id, Some(&session_id), Some(&plan_branch_id))
            .await
            .expect("workspace links should be updated");

        let error = archive_agent_conversation_for_state(&conversation_id, &state)
            .await
            .expect_err("archive should fail closed");

        assert!(error.contains("Linked execution plan not found"));
        assert!(state
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_none());
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            AgentConversationWorkspaceStatus::Active
        );
    }
}
