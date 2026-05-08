use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::application::pr_startup_recovery::{
    cleanup_terminal_agent_workspace_local_artifacts_on_startup,
    cleanup_terminal_plan_branch_local_artifacts_on_startup,
};
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as PlanPrStatus};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrDescription, ArtifactId, ChatConversationId, ExecutionPlanId,
    IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, PlanBranchId, PlanBranchStatus,
    Project, ProjectId, TaskId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, PlanBranchRepository, ProjectRepository,
};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryPlanBranchRepository, MemoryProjectRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn repo_error() -> AppError {
    AppError::Database("forced repository failure".to_string())
}

fn cleanup_project() -> Project {
    let mut project = Project::new(
        "Startup Coverage".to_string(),
        "/tmp/ralphx-startup-coverage".to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some("/tmp/ralphx-startup-worktrees".to_string());
    project.github_pr_enabled = true;
    project
}

fn merged_plan_branch(project: &Project, branch_name: &str) -> PlanBranch {
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string(format!("artifact-{branch_name}")),
        IdeationSessionId::from_string(format!("session-{branch_name}")),
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Merged;
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(101);
    plan_branch.pr_status = Some(PlanPrStatus::Merged);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    plan_branch
}

fn terminal_workspace(project: &Project, pr_status: Option<&str>) -> AgentConversationWorkspace {
    let conversation_id = ChatConversationId::new();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "manual/branch".to_string(),
        "/tmp/not-the-expected-worktree".to_string(),
    );
    workspace.publication_pr_number = Some(101);
    workspace.publication_pr_status = pr_status.map(ToString::to_string);
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.status = AgentConversationWorkspaceStatus::Active;
    workspace
}

#[tokio::test]
async fn startup_terminal_cleanup_returns_when_project_listing_fails() {
    let project_repo: Arc<dyn ProjectRepository> = Arc::new(ProjectListErrorRepository);
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());

    cleanup_terminal_plan_branch_local_artifacts_on_startup(
        plan_branch_repo,
        Arc::clone(&project_repo),
        None,
        Arc::new(HashSet::new()),
    )
    .await;
    cleanup_terminal_agent_workspace_local_artifacts_on_startup(
        workspace_repo,
        project_repo,
        None,
        Arc::new(HashSet::new()),
    )
    .await;
}

#[tokio::test]
async fn startup_terminal_plan_cleanup_continues_when_plan_branch_load_fails() {
    let project = cleanup_project();
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let plan_branch_repo: Arc<dyn PlanBranchRepository> = Arc::new(PlanBranchLoadErrorRepository);

    cleanup_terminal_plan_branch_local_artifacts_on_startup(
        plan_branch_repo,
        project_repo,
        None,
        Arc::new(HashSet::new()),
    )
    .await;
}

#[tokio::test]
async fn startup_terminal_workspace_cleanup_continues_when_workspace_load_fails() {
    let project = cleanup_project();
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(WorkspaceLoadErrorRepository);

    cleanup_terminal_agent_workspace_local_artifacts_on_startup(
        workspace_repo,
        project_repo,
        None,
        Arc::new(HashSet::new()),
    )
    .await;
}

#[tokio::test]
async fn startup_terminal_plan_cleanup_records_safety_skip_reports() {
    let project = cleanup_project();
    let project_repo: Arc<dyn ProjectRepository> = Arc::new(
        MemoryProjectRepository::with_projects(vec![project.clone()]),
    );
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    plan_branch_repo
        .create(merged_plan_branch(&project, "manual/not-owned"))
        .await
        .expect("create plan branch");
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_remote_result = Some(Err(AppError::GitOperation(
        "forced fetch failure".to_string(),
    )));

    cleanup_terminal_plan_branch_local_artifacts_on_startup(
        plan_branch_repo,
        project_repo,
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(HashSet::new()),
    )
    .await;

    assert_eq!(github.state().fetch_remote_calls, 1);
}

#[tokio::test]
async fn startup_terminal_workspace_cleanup_records_safety_skip_reports() {
    let project = cleanup_project();
    let project_repo: Arc<dyn ProjectRepository> = Arc::new(
        MemoryProjectRepository::with_projects(vec![project.clone()]),
    );
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(terminal_workspace(&project, Some("closed")))
        .await
        .expect("create workspace");
    workspace_repo
        .create_or_update(terminal_workspace(&project, Some("merged")))
        .await
        .expect("create merged workspace");
    workspace_repo
        .create_or_update(terminal_workspace(&project, None))
        .await
        .expect("create workspace without pr status");
    workspace_repo
        .create_or_update(terminal_workspace(&project, Some("open")))
        .await
        .expect("create open workspace");
    let workspaces = workspace_repo
        .get_by_project_id(&project.id)
        .await
        .expect("load workspaces");
    assert_eq!(workspaces.len(), 4);
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.publication_pr_status.as_deref() == Some("merged")));
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_remote_result = Some(Err(AppError::GitOperation(
        "forced fetch failure".to_string(),
    )));

    cleanup_terminal_agent_workspace_local_artifacts_on_startup(
        workspace_repo,
        project_repo,
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(HashSet::new()),
    )
    .await;

    assert_eq!(github.state().fetch_remote_calls, 1);
}

struct ProjectListErrorRepository;

#[async_trait]
impl ProjectRepository for ProjectListErrorRepository {
    async fn create(&self, _project: Project) -> AppResult<Project> {
        Err(repo_error())
    }

    async fn get_by_id(&self, _id: &ProjectId) -> AppResult<Option<Project>> {
        Err(repo_error())
    }

    async fn get_all(&self) -> AppResult<Vec<Project>> {
        Err(repo_error())
    }

    async fn update(&self, _project: &Project) -> AppResult<()> {
        Err(repo_error())
    }

    async fn delete(&self, _id: &ProjectId) -> AppResult<()> {
        Err(repo_error())
    }

    async fn get_by_working_directory(&self, _path: &str) -> AppResult<Option<Project>> {
        Err(repo_error())
    }

    async fn archive(&self, _id: &ProjectId) -> AppResult<Project> {
        Err(repo_error())
    }
}

struct PlanBranchLoadErrorRepository;

#[async_trait]
impl PlanBranchRepository for PlanBranchLoadErrorRepository {
    async fn create(&self, _branch: PlanBranch) -> AppResult<PlanBranch> {
        Err(repo_error())
    }

    async fn create_or_update(&self, _branch: PlanBranch) -> AppResult<PlanBranch> {
        Err(repo_error())
    }

    async fn get_by_id(&self, _id: &PlanBranchId) -> AppResult<Option<PlanBranch>> {
        Err(repo_error())
    }

    async fn get_by_plan_artifact_id(&self, _id: &ArtifactId) -> AppResult<Vec<PlanBranch>> {
        Err(repo_error())
    }

    async fn get_by_execution_plan_id(
        &self,
        _id: &ExecutionPlanId,
    ) -> AppResult<Option<PlanBranch>> {
        Err(repo_error())
    }

    async fn get_by_session_id(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Option<PlanBranch>> {
        Err(repo_error())
    }

    async fn get_by_merge_task_id(&self, _task_id: &TaskId) -> AppResult<Option<PlanBranch>> {
        Err(repo_error())
    }

    async fn get_by_project_id(&self, _project_id: &ProjectId) -> AppResult<Vec<PlanBranch>> {
        Err(repo_error())
    }

    async fn update_status(&self, _id: &PlanBranchId, _status: PlanBranchStatus) -> AppResult<()> {
        Err(repo_error())
    }

    async fn update_pr_eligible(&self, _id: &PlanBranchId, _enabled: bool) -> AppResult<()> {
        Err(repo_error())
    }

    async fn set_merge_task_id(&self, _id: &PlanBranchId, _task_id: &TaskId) -> AppResult<()> {
        Err(repo_error())
    }

    async fn clear_merge_task_id(&self, _id: &PlanBranchId) -> AppResult<()> {
        Err(repo_error())
    }

    async fn set_merged(&self, _id: &PlanBranchId) -> AppResult<()> {
        Err(repo_error())
    }

    async fn abandon_active_for_artifact(&self, _artifact_id: &ArtifactId) -> AppResult<u32> {
        Err(repo_error())
    }

    async fn delete(&self, _id: &PlanBranchId) -> AppResult<()> {
        Err(repo_error())
    }

    async fn update_pr_info(
        &self,
        _id: &PlanBranchId,
        _pr_number: i64,
        _pr_url: String,
        _pr_status: PlanPrStatus,
        _pr_draft: bool,
    ) -> AppResult<()> {
        Err(repo_error())
    }

    async fn clear_pr_info(&self, _id: &PlanBranchId) -> AppResult<()> {
        Err(repo_error())
    }

    async fn update_pr_status(&self, _id: &PlanBranchId, _status: PlanPrStatus) -> AppResult<()> {
        Err(repo_error())
    }

    async fn set_merge_commit_sha(&self, _id: &PlanBranchId, _sha: String) -> AppResult<()> {
        Err(repo_error())
    }

    async fn update_last_polled_at(
        &self,
        _id: &PlanBranchId,
        _polled_at: DateTime<Utc>,
    ) -> AppResult<()> {
        Err(repo_error())
    }

    async fn clear_polling_active_by_task(&self, _task_id: &TaskId) -> AppResult<()> {
        Err(repo_error())
    }

    async fn find_pr_polling_task_ids(&self) -> AppResult<Vec<TaskId>> {
        Err(repo_error())
    }

    async fn update_pr_push_status(
        &self,
        _id: &PlanBranchId,
        _status: PrPushStatus,
    ) -> AppResult<()> {
        Err(repo_error())
    }
}

struct WorkspaceLoadErrorRepository;

#[async_trait]
impl AgentConversationWorkspaceRepository for WorkspaceLoadErrorRepository {
    async fn create_or_update(
        &self,
        _workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        Err(repo_error())
    }

    async fn get_by_conversation_id(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Err(repo_error())
    }

    async fn get_by_project_id(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Err(repo_error())
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Err(repo_error())
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Err(repo_error())
    }

    async fn update_links(
        &self,
        _conversation_id: &ChatConversationId,
        _ideation_session_id: Option<&IdeationSessionId>,
        _plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()> {
        Err(repo_error())
    }

    async fn update_publication(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: Option<i64>,
        _pr_url: Option<&str>,
        _pr_status: Option<&str>,
        _push_status: Option<&str>,
    ) -> AppResult<()> {
        Err(repo_error())
    }

    async fn update_status(
        &self,
        _conversation_id: &ChatConversationId,
        _status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()> {
        Err(repo_error())
    }

    async fn save_pr_description(
        &self,
        _conversation_id: &ChatConversationId,
        _description: AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        Err(repo_error())
    }

    async fn get_pr_description(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>> {
        Err(repo_error())
    }

    async fn clear_pr_description(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Err(repo_error())
    }

    async fn append_publication_event(
        &self,
        _event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()> {
        Err(repo_error())
    }

    async fn list_publication_events(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>> {
        Err(repo_error())
    }

    async fn delete(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Err(repo_error())
    }
}
