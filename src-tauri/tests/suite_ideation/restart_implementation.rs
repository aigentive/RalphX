use std::{path::Path, process::Command, sync::Arc};

use async_trait::async_trait;
use tempfile::TempDir;

use ralphx_lib::application::{
    agent_conversation_workspace::resolve_agent_conversation_workspace_path, AppState,
};
use ralphx_lib::commands::ideation_commands::{
    apply_proposals_core, restart_implementation_core, ApplyProposalsInput,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId, ExecutionPlan,
    ExecutionPlanId, ExecutionPlanStatus, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionId, IdeationSessionStatus, InternalStatus, Priority, Project, ProjectId,
    ProposalCategory, Task, TaskCategory, TaskProposal, VerificationStatus,
};
use ralphx_lib::domain::repositories::ExecutionPlanRepository;
use ralphx_lib::error::AppError;
use ralphx_lib::error::AppResult;

const RELINK_FAILURE_CONVERSATION_ID: &str = "00000000-0000-0000-0000-00000000cafe";

struct RestartFixture {
    state: AppState,
    _project_dir: TempDir,
    project_id: ProjectId,
    session_id: IdeationSessionId,
    old_execution_plan_id: ExecutionPlanId,
    old_task_ids: Vec<String>,
    older_superseded_task: Task,
}

struct StaleActiveExecutionPlanRepo {
    inner: Arc<dyn ExecutionPlanRepository>,
    stale_active_plan: ExecutionPlan,
}

#[async_trait]
impl ExecutionPlanRepository for StaleActiveExecutionPlanRepo {
    async fn create(&self, plan: ExecutionPlan) -> AppResult<ExecutionPlan> {
        self.inner.create(plan).await
    }

    async fn get_by_id(&self, id: &ExecutionPlanId) -> AppResult<Option<ExecutionPlan>> {
        self.inner.get_by_id(id).await
    }

    async fn get_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<ExecutionPlan>> {
        self.inner.get_by_session(session_id).await
    }

    async fn get_active_for_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<ExecutionPlan>> {
        if self.stale_active_plan.session_id == *session_id {
            Ok(Some(self.stale_active_plan.clone()))
        } else {
            self.inner.get_active_for_session(session_id).await
        }
    }

    async fn mark_superseded(&self, id: &ExecutionPlanId) -> AppResult<()> {
        self.inner.mark_superseded(id).await
    }

    async fn delete(&self, id: &ExecutionPlanId) -> AppResult<()> {
        self.inner.delete(id).await
    }
}

fn setup_restart_state() -> AppState {
    AppState::new_sqlite_for_apply_test()
}

async fn create_project_and_session(state: &AppState) -> (TempDir, ProjectId, IdeationSessionId) {
    let project_dir = tempfile::tempdir_in(std::env::current_dir().unwrap())
        .expect("create contained project dir");
    let mut project = Project::new(
        "Restart Project".to_string(),
        project_dir.path().to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(
        project_dir
            .path()
            .join("agent-worktrees")
            .to_string_lossy()
            .to_string(),
    );
    let project = state.project_repo.create(project).await.unwrap();

    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    (project_dir, project.id, session.id)
}

fn run_git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_branch(path: &Path, branch_name: &str) {
    std::fs::create_dir_all(path).expect("create contained git workspace");
    run_git(path, &["init"]);
    run_git(path, &["config", "user.email", "test@example.com"]);
    run_git(path, &["config", "user.name", "Restart Test"]);
    std::fs::write(path.join("README.md"), "restart workspace\n")
        .expect("seed contained git workspace");
    run_git(path, &["add", "README.md"]);
    run_git(path, &["commit", "-m", "initial"]);
    run_git(path, &["checkout", "-B", branch_name]);
}

async fn install_workspace_relink_failure_trigger(state: &AppState) {
    state
        .db
        .run(|conn| {
            conn.execute(
                "CREATE TRIGGER fail_restart_workspace_relink
                 BEFORE UPDATE OF linked_ideation_session_id, linked_plan_branch_id
                 ON agent_conversation_workspaces
                 WHEN OLD.conversation_id = '00000000-0000-0000-0000-00000000cafe'
                 BEGIN
                     SELECT RAISE(FAIL, 'injected update_links failure');
                 END",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}

async fn create_proposal(
    state: &AppState,
    session_id: &IdeationSessionId,
    title: &str,
    priority: Priority,
    steps: &[&str],
) -> TaskProposal {
    let mut proposal = TaskProposal::new(
        session_id.clone(),
        title.to_string(),
        ProposalCategory::Feature,
        priority,
    );
    proposal.steps = Some(serde_json::to_string(steps).unwrap());
    state.task_proposal_repo.create(proposal).await.unwrap()
}

async fn create_accepted_plan_with_tasks() -> RestartFixture {
    let state = setup_restart_state();
    let (project_dir, project_id, session_id) = create_project_and_session(&state).await;

    let blocker = create_proposal(
        &state,
        &session_id,
        "Blocker Task",
        Priority::High,
        &["Design", "Implement"],
    )
    .await;
    let dependent = create_proposal(
        &state,
        &session_id,
        "Dependent Task",
        Priority::Medium,
        &["Wire"],
    )
    .await;

    state
        .proposal_dependency_repo
        .add_dependency(&dependent.id, &blocker.id, None, Some("test"))
        .await
        .unwrap();
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session_id.as_str())
        .await
        .unwrap();

    let applied = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session_id.as_str().to_string(),
            proposal_ids: vec![
                blocker.id.as_str().to_string(),
                dependent.id.as_str().to_string(),
            ],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .unwrap();

    let old_execution_plan_id =
        ExecutionPlanId::from_string(applied.execution_plan_id.expect("execution plan id"));
    let older_superseded_task =
        create_older_superseded_attempt(&state, &project_id, &session_id).await;
    state
        .active_plan_repo
        .set(&project_id, &session_id)
        .await
        .unwrap();
    state
        .active_plan_repo
        .set_execution_plan_id(&project_id, &old_execution_plan_id)
        .await
        .unwrap();
    state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Verified, false)
        .await
        .unwrap();

    RestartFixture {
        state,
        _project_dir: project_dir,
        project_id,
        session_id,
        old_execution_plan_id,
        old_task_ids: applied.created_task_ids,
        older_superseded_task,
    }
}

async fn create_older_superseded_attempt(
    state: &AppState,
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
) -> Task {
    let mut plan = ExecutionPlan::new(session_id.clone());
    plan.status = ExecutionPlanStatus::Superseded;
    let plan = state.execution_plan_repo.create(plan).await.unwrap();

    let mut task = Task::new(project_id.clone(), "Older Superseded Attempt".to_string());
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(plan.id);
    task.internal_status = InternalStatus::Ready;
    state.task_repo.create(task).await.unwrap()
}

async fn tasks_for_plan(
    state: &AppState,
    project_id: &ProjectId,
    execution_plan_id: &ExecutionPlanId,
    include_archived: bool,
) -> Vec<Task> {
    let count = state
        .task_repo
        .count_tasks(
            project_id,
            include_archived,
            None,
            Some(execution_plan_id.as_str()),
        )
        .await
        .unwrap();
    state
        .task_repo
        .list_paginated(
            project_id,
            None,
            0,
            count.max(1),
            include_archived,
            None,
            Some(execution_plan_id.as_str()),
            None,
        )
        .await
        .unwrap()
}

async fn durable_active_plan_pointer(
    state: &AppState,
    project_id: &ProjectId,
) -> Option<(IdeationSessionId, Option<ExecutionPlanId>)> {
    let project_id = project_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            match conn.query_row(
                "SELECT ideation_session_id, execution_plan_id FROM project_active_plan WHERE project_id = ?1",
                [project_id.as_str()],
                |row| {
                    let session_id: String = row.get(0)?;
                    let execution_plan_id: Option<String> = row.get(1)?;
                    Ok((session_id, execution_plan_id))
                },
            ) {
                Ok((session_id, execution_plan_id)) => Ok(Some((
                    IdeationSessionId::from_string(session_id),
                    execution_plan_id.map(ExecutionPlanId::from_string),
                ))),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(error) => Err(AppError::Database(error.to_string())),
            }
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn restart_implementation_replaces_active_attempt_and_preserves_plan_state() {
    let fixture = create_accepted_plan_with_tasks().await;

    let result = restart_implementation_core(&fixture.state, &fixture.session_id)
        .await
        .expect("restart should succeed");

    assert_eq!(result.project_id, fixture.project_id.as_str());
    assert_eq!(result.session_id, fixture.session_id.as_str());
    assert_eq!(
        result.old_execution_plan_id,
        fixture.old_execution_plan_id.as_str()
    );
    assert_ne!(result.new_execution_plan_id, result.old_execution_plan_id);
    assert_eq!(result.archived_task_count, 3, "two tasks plus merge task");
    assert_eq!(result.tasks_created, 2);
    assert_eq!(result.dependencies_created, 1);
    assert_eq!(result.created_task_ids.len(), 2);
    assert!(result.any_ready_tasks);

    let old_plan = fixture
        .state
        .execution_plan_repo
        .get_by_id(&fixture.old_execution_plan_id)
        .await
        .unwrap()
        .expect("old plan exists");
    assert_eq!(old_plan.status, ExecutionPlanStatus::Superseded);

    for task_id in &fixture.old_task_ids {
        let task = fixture
            .state
            .task_repo
            .get_by_id(&ralphx_lib::domain::entities::TaskId::from_string(
                task_id.clone(),
            ))
            .await
            .unwrap()
            .expect("old task exists");
        assert!(task.archived_at.is_some(), "old task should be archived");
    }

    let old_visible = tasks_for_plan(
        &fixture.state,
        &fixture.project_id,
        &fixture.old_execution_plan_id,
        false,
    )
    .await;
    assert!(old_visible.is_empty(), "old attempt should be hidden");

    let older_superseded_task = fixture
        .state
        .task_repo
        .get_by_id(&fixture.older_superseded_task.id)
        .await
        .unwrap()
        .expect("older superseded task exists");
    assert_eq!(
        older_superseded_task.execution_plan_id,
        fixture.older_superseded_task.execution_plan_id
    );
    assert!(
        older_superseded_task.archived_at.is_none(),
        "older superseded attempts must not be archived by restart"
    );
    assert_eq!(older_superseded_task.internal_status, InternalStatus::Ready);

    let active_plan = fixture
        .state
        .execution_plan_repo
        .get_active_for_session(&fixture.session_id)
        .await
        .unwrap()
        .expect("new active execution plan");
    assert_eq!(active_plan.id.as_str(), result.new_execution_plan_id);

    let new_tasks =
        tasks_for_plan(&fixture.state, &fixture.project_id, &active_plan.id, false).await;
    assert_eq!(new_tasks.len(), 3, "two plan tasks plus merge task");

    let blocker_task = new_tasks
        .iter()
        .find(|task| task.title == "Blocker Task")
        .expect("blocker recreated");
    let dependent_task = new_tasks
        .iter()
        .find(|task| task.title == "Dependent Task")
        .expect("dependent recreated");
    assert_eq!(blocker_task.internal_status, InternalStatus::Ready);
    assert_eq!(dependent_task.internal_status, InternalStatus::Blocked);
    assert_eq!(
        dependent_task.execution_plan_id,
        Some(active_plan.id.clone())
    );

    let dependent_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&dependent_task.id)
        .await
        .unwrap();
    assert_eq!(dependent_blockers, vec![blocker_task.id.clone()]);

    let blocker_steps = fixture
        .state
        .task_step_repo
        .get_by_task(&blocker_task.id)
        .await
        .unwrap();
    assert_eq!(blocker_steps.len(), 2, "proposal steps should be recreated");

    let branch = fixture
        .state
        .plan_branch_repo
        .get_by_session_id(&fixture.session_id)
        .await
        .unwrap()
        .expect("plan branch exists");
    assert_eq!(branch.execution_plan_id, Some(active_plan.id.clone()));
    let merge_task_id = branch.merge_task_id.expect("merge task linked");
    let merge_task = fixture
        .state
        .task_repo
        .get_by_id(&merge_task_id)
        .await
        .unwrap()
        .expect("merge task exists");
    assert_eq!(merge_task.category, TaskCategory::PlanMerge);
    assert_eq!(merge_task.execution_plan_id, Some(active_plan.id.clone()));
    let merge_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&merge_task.id)
        .await
        .unwrap();
    assert_eq!(merge_blockers.len(), 2);

    let proposals = fixture
        .state
        .task_proposal_repo
        .get_by_session(&fixture.session_id)
        .await
        .unwrap();
    for proposal in proposals {
        let created_task_id = proposal.created_task_id.expect("proposal relinked");
        assert!(
            result
                .created_task_ids
                .iter()
                .any(|id| id == created_task_id.as_str()),
            "proposal should point at a newly created task"
        );
        assert!(
            !fixture
                .old_task_ids
                .iter()
                .any(|id| id == created_task_id.as_str()),
            "proposal should not point at an old task"
        );
    }

    let active_pointer = durable_active_plan_pointer(&fixture.state, &fixture.project_id)
        .await
        .expect("durable active plan pointer");
    assert_eq!(active_pointer.0, fixture.session_id);
    assert_eq!(active_pointer.1, Some(active_plan.id.clone()));

    let session = fixture
        .state
        .ideation_session_repo
        .get_by_id(&fixture.session_id)
        .await
        .unwrap()
        .expect("session exists");
    assert_eq!(session.status, IdeationSessionStatus::Accepted);
    assert_eq!(session.verification_status, VerificationStatus::Verified);
    assert!(!session.verification_in_progress);
}

#[tokio::test]
async fn restart_implementation_keeps_old_attempt_visible_when_supersede_guard_fails() {
    let fixture = create_accepted_plan_with_tasks().await;
    let stale_active_plan = fixture
        .state
        .execution_plan_repo
        .get_by_id(&fixture.old_execution_plan_id)
        .await
        .unwrap()
        .expect("old plan exists");
    let old_plan_id = fixture.old_execution_plan_id.as_str().to_string();
    fixture
        .state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE execution_plans SET status = ?1 WHERE id = ?2",
                rusqlite::params![
                    ExecutionPlanStatus::Superseded.to_db_string(),
                    old_plan_id.as_str(),
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let mut state = fixture.state.clone();
    state.execution_plan_repo = Arc::new(StaleActiveExecutionPlanRepo {
        inner: Arc::clone(&fixture.state.execution_plan_repo),
        stale_active_plan,
    });

    let err = restart_implementation_core(&state, &fixture.session_id)
        .await
        .expect_err("restart should reject stale active plan authority");
    assert!(
        err.to_string()
            .contains("Active execution plan changed before restart"),
        "unexpected error: {err}"
    );

    let old_visible = tasks_for_plan(
        &state,
        &fixture.project_id,
        &fixture.old_execution_plan_id,
        false,
    )
    .await;
    assert_eq!(
        old_visible.len(),
        3,
        "failed restart authority must not hide old attempt tasks"
    );
    assert!(
        old_visible.iter().all(|task| task.archived_at.is_none()),
        "old attempt tasks should remain unarchived after rejected restart"
    );

    let active_session = state
        .active_plan_repo
        .get(&fixture.project_id)
        .await
        .unwrap();
    assert_eq!(active_session, Some(fixture.session_id));
    let active_execution_plan = state
        .active_plan_repo
        .get_execution_plan_id(&fixture.project_id)
        .await
        .unwrap();
    assert_eq!(active_execution_plan, Some(fixture.old_execution_plan_id));
}

#[tokio::test]
async fn restart_implementation_succeeds_when_workspace_relink_fails_after_commit() {
    let fixture = create_accepted_plan_with_tasks().await;
    let project = fixture
        .state
        .project_repo
        .get_by_id(&fixture.project_id)
        .await
        .unwrap()
        .expect("project exists");
    let conversation_id = ChatConversationId::from_string(RELINK_FAILURE_CONVERSATION_ID);
    let branch_name = "ralphx/restart/linked-workspace";
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_git_branch(&worktree_path, branch_name);

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        fixture.project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        branch_name.to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(fixture.session_id.clone());
    fixture
        .state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    install_workspace_relink_failure_trigger(&fixture.state).await;

    let result = restart_implementation_core(&fixture.state, &fixture.session_id)
        .await
        .expect("committed restart should survive workspace relink failure");
    assert_eq!(result.tasks_created, 2);
    assert!(result.any_ready_tasks);
    assert!(
        result.warnings.iter().any(|warning| {
            warning.contains("Failed to link agent conversation workspace to restarted plan branch")
        }),
        "restart should expose non-fatal workspace relink warning: {:?}",
        result.warnings
    );

    let active_pointer = durable_active_plan_pointer(&fixture.state, &fixture.project_id)
        .await
        .expect("durable active plan pointer");
    assert_eq!(active_pointer.0, fixture.session_id);
    assert_eq!(
        active_pointer.1.as_ref().map(ExecutionPlanId::as_str),
        Some(result.new_execution_plan_id.as_str())
    );
}

#[tokio::test]
async fn restart_implementation_rejects_missing_active_execution_plan() {
    let state = setup_restart_state();
    let (_project_dir, project_id, session_id) = create_project_and_session(&state).await;
    let proposal = create_proposal(&state, &session_id, "Task", Priority::Medium, &[]).await;
    state
        .ideation_session_repo
        .update_status(&session_id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();
    state
        .active_plan_repo
        .set(&project_id, &session_id)
        .await
        .unwrap();

    let err = restart_implementation_core(&state, &session_id)
        .await
        .expect_err("restart should reject missing active execution plan");
    assert!(
        err.to_string().contains("No active execution plan"),
        "unexpected error: {err}"
    );

    let proposals = state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .unwrap();
    assert_eq!(proposals[0].id, proposal.id);
    assert!(proposals[0].created_task_id.is_none());
}

#[tokio::test]
async fn restart_implementation_rejects_inactive_project_context() {
    let fixture = create_accepted_plan_with_tasks().await;
    fixture
        .state
        .active_plan_repo
        .clear(&fixture.project_id)
        .await
        .unwrap();

    let err = restart_implementation_core(&fixture.state, &fixture.session_id)
        .await
        .expect_err("restart should reject inactive session");
    assert!(
        err.to_string().contains("active implementation plan"),
        "unexpected error: {err}"
    );

    let old_plan = fixture
        .state
        .execution_plan_repo
        .get_by_id(&fixture.old_execution_plan_id)
        .await
        .unwrap()
        .expect("old plan exists");
    assert_eq!(old_plan.status, ExecutionPlanStatus::Active);
}

#[tokio::test]
async fn restart_implementation_rejects_non_accepted_session() {
    let state = setup_restart_state();
    let (_project_dir, _project_id, session_id) = create_project_and_session(&state).await;
    create_proposal(&state, &session_id, "Task", Priority::Medium, &[]).await;

    let err = restart_implementation_core(&state, &session_id)
        .await
        .expect_err("restart should reject draft session");
    assert!(
        err.to_string().contains("Accepted"),
        "unexpected error: {err}"
    );
}
