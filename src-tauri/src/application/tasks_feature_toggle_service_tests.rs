use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, BranchUpdateCapacityOwnership,
    BranchUpdateContinuation, BranchUpdateDirection, BranchUpdateOperation,
    BranchUpdateWorkspaceOwnership, ChatConversationId, GitTargetIdentity,
    IdeationAnalysisBaseRefKind, IdeationSession, InternalStatus, Project, Task,
};
use crate::domain::ideation::TasksFeatureState;
use crate::domain::repositories::{
    BranchUpdateActivation, BranchUpdateActivationOutcome, BranchUpdateRepository,
    ProjectRepository, TaskRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryBranchUpdateRepository;

struct FailingProjectRepository;

async fn enable_tasks(state: &AppState) {
    assert!(state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap());
}

#[async_trait]
impl ProjectRepository for FailingProjectRepository {
    async fn create(&self, _project: Project) -> AppResult<Project> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn get_by_id(
        &self,
        _id: &crate::domain::entities::ProjectId,
    ) -> AppResult<Option<Project>> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn get_all(&self) -> AppResult<Vec<Project>> {
        Err(AppError::Database(
            "injected project enumeration failure".into(),
        ))
    }

    async fn update(&self, _project: &Project) -> AppResult<()> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn delete(&self, _id: &crate::domain::entities::ProjectId) -> AppResult<()> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn get_by_working_directory(&self, _path: &str) -> AppResult<Option<Project>> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn archive(&self, _id: &crate::domain::entities::ProjectId) -> AppResult<Project> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }
}

#[tokio::test]
async fn disabling_tasks_pauses_all_active_tasks_including_attached_workspaces() {
    let state = AppState::new_test();
    enable_tasks(&state).await;
    let project = state
        .project_repo
        .create(Project::new(
            "Policy project".to_string(),
            "/tmp/policy-project".to_string(),
        ))
        .await
        .unwrap();
    let session = IdeationSession::new(project.id.clone());
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-1"),
        project.id.clone(),
        AgentConversationWorkspaceMode::Tasks,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/conversation-1".to_string(),
        "/tmp/policy-worktree".to_string(),
    );
    workspace.task_pipeline_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut standalone_active = Task::new(project.id.clone(), "Standalone active".to_string());
    standalone_active.internal_status = InternalStatus::Executing;
    let standalone_active = state.task_repo.create(standalone_active).await.unwrap();

    let standalone_ready = state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Standalone ready".to_string(),
        ))
        .await
        .unwrap();

    let mut attached_active = Task::new(project.id.clone(), "Attached active".to_string());
    attached_active.internal_status = InternalStatus::Reviewing;
    attached_active.ideation_session_id = Some(session.id.clone());
    let attached_active = state.task_repo.create(attached_active).await.unwrap();

    state
        .build_tasks_feature_toggle_service_for_test()
        .set_tasks_enabled(false)
        .await
        .expect("OFF drain should succeed");

    let paused = state
        .task_repo
        .get_by_id(&standalone_active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.internal_status, InternalStatus::Paused);
    assert!(paused
        .metadata
        .as_deref()
        .is_some_and(|metadata| metadata.contains("tasks_feature_disabled")));
    assert_eq!(
        state
            .task_repo
            .get_by_id(&standalone_ready.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Backlog
    );
    assert_eq!(
        state
            .task_repo
            .get_by_id(&attached_active.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Paused
    );
    assert!(
        !state
            .ideation_settings_repo
            .get_settings()
            .await
            .unwrap()
            .tasks_enabled
    );
    assert_eq!(
        state
            .ideation_settings_repo
            .get_settings()
            .await
            .unwrap()
            .tasks_feature_state,
        TasksFeatureState::Disabled
    );
}

#[tokio::test]
async fn disabling_tasks_pauses_archived_active_tasks() {
    let state = AppState::new_test();
    enable_tasks(&state).await;
    let project = state
        .project_repo
        .create(Project::new(
            "Archived active task drain project".to_string(),
            "/tmp/archived-active-task-drain-project".to_string(),
        ))
        .await
        .unwrap();
    let mut task = Task::new(project.id, "Archived active task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task = state.task_repo.create(task).await.unwrap();
    state.task_repo.archive(&task.id).await.unwrap();

    state
        .build_tasks_feature_toggle_service_for_test()
        .set_tasks_enabled(false)
        .await
        .expect("Tasks-off drain must quiesce archived active work");

    assert_eq!(
        state
            .task_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .expect("archived task must remain persisted")
            .internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn disable_impact_counts_standalone_attached_and_paused_work() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Impact project".to_string(),
            "/tmp/impact-project".to_string(),
        ))
        .await
        .unwrap();
    let session = IdeationSession::new(project.id.clone());
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();
    let conversation_id = ChatConversationId::new();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Tasks,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/impact".to_string(),
        "/tmp/impact-worktree".to_string(),
    );
    workspace.task_pipeline_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut standalone = Task::new(project.id.clone(), "Standalone".to_string());
    standalone.internal_status = InternalStatus::Executing;
    state.task_repo.create(standalone).await.unwrap();
    let mut attached = Task::new(project.id.clone(), "Attached".to_string());
    attached.internal_status = InternalStatus::Reviewing;
    attached.ideation_session_id = Some(session.id);
    state.task_repo.create(attached).await.unwrap();
    let mut paused = Task::new(project.id.clone(), "Paused".to_string());
    paused.internal_status = InternalStatus::Paused;
    state.task_repo.create(paused).await.unwrap();

    let impact = state
        .build_tasks_feature_toggle_service_for_test()
        .get_disable_impact()
        .await
        .unwrap();

    assert_eq!(impact.active_standalone_tasks, 1);
    assert_eq!(impact.active_attached_agent_workspaces, 1);
    assert_eq!(impact.paused_or_blocked_tasks, 1);
    assert_eq!(impact.affected_task_ids.len(), 2);
    assert_eq!(
        impact.affected_conversation_ids,
        vec![conversation_id.as_str().to_string()]
    );
}

#[tokio::test]
async fn disabling_tasks_pauses_plan_and_task_branch_updates_without_auto_resuming() {
    let mut state = AppState::new_test();
    let branch_repo = Arc::new(
        MemoryBranchUpdateRepository::new().with_task_repository(Arc::clone(&state.task_repo)),
    );
    state.branch_update_repo = branch_repo.clone();
    enable_tasks(&state).await;
    let project = state
        .project_repo
        .create(Project::new(
            "Branch drain project".to_string(),
            "/tmp/branch-drain-project".to_string(),
        ))
        .await
        .unwrap();
    let plan_task = state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Plan branch update".to_string(),
        ))
        .await
        .unwrap();
    let plan_operation = BranchUpdateOperation::new(
        plan_task.id.clone(),
        BranchUpdateDirection::PlanBranch,
        BranchUpdateContinuation::ResumeExecution,
        "branch-drain-history",
        "main",
        "ralphx/branch-drain",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        GitTargetIdentity::new(
            PathBuf::from("/tmp/branch-drain-project/.git"),
            "refs/heads/ralphx/branch-drain",
        )
        .unwrap(),
        Utc::now(),
    );
    assert!(matches!(
        branch_repo
            .activate(BranchUpdateActivation {
                operation: plan_operation,
                expected_status: InternalStatus::Backlog,
                update_status: InternalStatus::UpdatingPlanBranch,
                trigger: "tasks-off-test".to_string(),
            })
            .await
            .unwrap(),
        BranchUpdateActivationOutcome::Applied { .. }
    ));

    let task_task = state
        .task_repo
        .create(Task::new(project.id, "Task branch update".to_string()))
        .await
        .unwrap();
    let task_operation = BranchUpdateOperation::new(
        task_task.id.clone(),
        BranchUpdateDirection::TaskBranch,
        BranchUpdateContinuation::ResumeExecution,
        "task-branch-drain-history",
        "main",
        "ralphx/task-branch-drain",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        GitTargetIdentity::new(
            PathBuf::from("/tmp/branch-drain-project/.git"),
            "refs/heads/ralphx/task-branch-drain",
        )
        .unwrap(),
        Utc::now(),
    );
    assert!(matches!(
        branch_repo
            .activate(BranchUpdateActivation {
                operation: task_operation,
                expected_status: InternalStatus::Backlog,
                update_status: InternalStatus::UpdatingTaskBranch,
                trigger: "tasks-off-test".to_string(),
            })
            .await
            .unwrap(),
        BranchUpdateActivationOutcome::Applied { .. }
    ));

    let settings = state
        .build_tasks_feature_toggle_service_for_test()
        .set_tasks_enabled(false)
        .await
        .expect("branch update should quiesce through its authority repository");

    assert_eq!(settings.tasks_feature_state, TasksFeatureState::Disabled);
    assert_eq!(
        state
            .task_repo
            .get_by_id(&plan_task.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Paused
    );
    let paused_plan_task = state
        .task_repo
        .get_by_id(&plan_task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(paused_plan_task
        .metadata
        .as_deref()
        .is_some_and(|metadata| metadata.contains("tasks_feature_disabled")));
    assert_eq!(
        state
            .task_repo
            .get_by_id(&task_task.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Paused
    );

    state
        .build_tasks_feature_toggle_service_for_test()
        .set_tasks_enabled(true)
        .await
        .expect("re-enabling must not auto-resume paused branch updates");
    assert_eq!(
        state
            .task_repo
            .get_by_id(&plan_task.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Paused
    );
    assert_eq!(
        state
            .build_tasks_feature_toggle_service_for_test()
            .get_disable_impact()
            .await
            .unwrap()
            .active_branch_update_operations,
        0,
        "an already-paused operation is not active impact"
    );
}

#[tokio::test]
async fn disabling_tasks_fences_an_active_branch_update_for_an_already_paused_task() {
    let mut state = AppState::new_test();
    let branch_repo = Arc::new(
        MemoryBranchUpdateRepository::new().with_task_repository(Arc::clone(&state.task_repo)),
    );
    state.branch_update_repo = branch_repo.clone();
    enable_tasks(&state).await;
    let project = state
        .project_repo
        .create(Project::new(
            "Paused branch drain project".to_string(),
            "/tmp/paused-branch-drain-project".to_string(),
        ))
        .await
        .unwrap();
    let mut task = Task::new(project.id, "Paused branch update".to_string());
    task.internal_status = InternalStatus::Paused;
    let task = state.task_repo.create(task).await.unwrap();
    let operation = BranchUpdateOperation::new(
        task.id.clone(),
        BranchUpdateDirection::TaskBranch,
        BranchUpdateContinuation::ResumeExecution,
        "paused-branch-drain-history",
        "main",
        "ralphx/paused-branch-drain",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        GitTargetIdentity::new(
            PathBuf::from("/tmp/paused-branch-drain-project/.git"),
            "refs/heads/ralphx/paused-branch-drain",
        )
        .unwrap(),
        Utc::now(),
    );
    assert!(matches!(
        branch_repo
            .activate(BranchUpdateActivation {
                operation,
                expected_status: InternalStatus::Paused,
                update_status: InternalStatus::Paused,
                trigger: "tasks-off-paused-test".to_string(),
            })
            .await
            .unwrap(),
        BranchUpdateActivationOutcome::Applied { .. }
    ));

    let impact = state
        .build_tasks_feature_toggle_service_for_test()
        .get_disable_impact()
        .await
        .expect("disable impact must include operations that need the branch-update pause fence");
    assert_eq!(impact.active_branch_update_operations, 1);

    let settings = state
        .build_tasks_feature_toggle_service_for_test()
        .set_tasks_enabled(false)
        .await
        .expect("an active branch update on a paused task must be fenced before disabling");

    assert_eq!(settings.tasks_feature_state, TasksFeatureState::Disabled);
    let paused_task = state.task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(paused_task.internal_status, InternalStatus::Paused);
    assert!(
        paused_task
            .metadata
            .as_deref()
            .is_some_and(|metadata| metadata.contains("tasks_feature_disabled")),
        "the active operation must pass through the branch-update pause fence"
    );
}

#[tokio::test]
async fn disabling_tasks_keeps_off_when_drain_cannot_enumerate_projects() {
    let mut state = AppState::new_test();
    enable_tasks(&state).await;
    state.project_repo = Arc::new(FailingProjectRepository);

    let error = state
        .build_tasks_feature_toggle_service_for_test()
        .set_tasks_enabled(false)
        .await
        .expect_err("drain failure must be reported after committing OFF");

    assert!(error
        .to_string()
        .starts_with("ralphx:tasks_drain_incomplete"));
    assert!(
        !state
            .ideation_settings_repo
            .get_settings()
            .await
            .unwrap()
            .tasks_enabled
    );
    assert_eq!(
        state
            .ideation_settings_repo
            .get_settings()
            .await
            .unwrap()
            .tasks_feature_state,
        TasksFeatureState::Draining
    );
}

#[tokio::test]
async fn enabling_tasks_persists_the_setting_without_an_app_handle() {
    let state = AppState::new_test();

    let updated = state
        .build_tasks_feature_toggle_service_for_test()
        .set_tasks_enabled(true)
        .await
        .expect("re-enabling Tasks without the desktop app handle must persist the setting");

    assert!(updated.tasks_enabled);
    assert_eq!(updated.tasks_feature_state, TasksFeatureState::Enabled);
    assert!(
        state
            .ideation_settings_repo
            .get_settings()
            .await
            .unwrap()
            .tasks_enabled
    );
}

#[tokio::test]
async fn enabling_tasks_when_already_enabled_keeps_the_setting_enabled() {
    let state = AppState::new_test();
    enable_tasks(&state).await;

    let updated = state
        .build_tasks_feature_toggle_service_for_test()
        .set_tasks_enabled(true)
        .await
        .expect("an already enabled Tasks setting must stay enabled");

    assert!(updated.tasks_enabled);
}
