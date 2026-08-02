use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::application::agent_conversation_workspace::{
    resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
};
use crate::application::chat_service::MockChatService;
use crate::application::git_service::GitService;
use crate::application::pr_startup_recovery::{
    cleanup_terminal_agent_workspace_local_artifacts_on_startup,
    cleanup_terminal_plan_branch_local_artifacts_on_startup, recover_agent_workspace_pr_pollers,
    recover_agent_workspace_pr_pollers_with_notifications, recover_one_agent_workspace_pr_poller,
};
use crate::application::services::PrPollerRegistry;
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as PlanPrStatus};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, AgentRun,
    AgentWorkspacePrDescription, AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
    AgentWorkspaceSourcePullRequest, ArtifactId, ChatConversationId, ExecutionPlanId,
    IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, PlanBranchId, PlanBranchStatus,
    Project, ProjectId, TaskId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceRepairRepository,
    PlanBranchRepository, ProjectRepository, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use crate::domain::services::{
    github_service::{
        GithubServiceTrait, PrHealth, PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
    },
    MemoryRunningAgentRegistry, RunningAgentRegistry,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryPlanBranchRepository, MemoryProjectRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn repo_error() -> AppError {
    AppError::Database("forced repository failure".to_string())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();
}

fn empty_running_agent_registry() -> Arc<dyn RunningAgentRegistry> {
    Arc::new(MemoryRunningAgentRegistry::new())
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

fn run_git(repo: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

fn published_workspace(
    project: &Project,
    conversation_id: ChatConversationId,
    branch_name: &str,
) -> AgentConversationWorkspace {
    let worktree_path = resolve_agent_conversation_workspace_path(project, &conversation_id)
        .expect("workspace path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        branch_name.to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(101);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/101".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace
}

fn review_pr_workspace(
    project: &Project,
    conversation_id: ChatConversationId,
    branch_name: &str,
) -> AgentConversationWorkspace {
    let worktree_path = resolve_agent_conversation_workspace_path(project, &conversation_id)
        .expect("workspace path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        branch_name.to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 101,
        url: Some("https://github.com/owner/repo/pull/101".to_string()),
        title: Some("Improve feature".to_string()),
        head_ref_name: "feature/pr".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("old-head".to_string()),
    });
    workspace
}

struct ReviewPrPollerRecoveryFixture {
    _temp_dir: tempfile::TempDir,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    registry: Arc<PrPollerRegistry>,
    github: Arc<MockGithubService>,
    conversation_id: ChatConversationId,
    workspace: AgentConversationWorkspace,
}

async fn setup_review_pr_poller_recovery_fixture(
    conversation_id: &str,
    branch_name: &str,
) -> ReviewPrPollerRecoveryFixture {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo_path).expect("create repo dir");
    run_git(&repo_path, &["init"]);
    run_git(&repo_path, &["config", "user.email", "test@example.com"]);
    run_git(&repo_path, &["config", "user.name", "Test User"]);
    run_git(&repo_path, &["checkout", "-b", "main"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "initial"]);

    let mut project = Project::new(
        "Startup Review PR Monitor".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project.worktree_parent_directory = Some(
        temp_dir
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );

    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = review_pr_workspace(&project, conversation_id.clone(), branch_name);
    GitService::create_worktree(
        &repo_path,
        Path::new(&workspace.worktree_path),
        branch_name,
        "main",
    )
    .await
    .expect("create workspace worktree");

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let github = Arc::new(MockGithubService::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&plan_branch_repo),
    ));

    ReviewPrPollerRecoveryFixture {
        _temp_dir: temp_dir,
        workspace_repo,
        project_repo,
        plan_branch_repo,
        registry,
        github,
        conversation_id,
        workspace,
    }
}

async fn recover_review_pr_poller_fixture(fixture: &ReviewPrPollerRecoveryFixture) {
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        fixture.workspace_repo.clone();
    recover_agent_workspace_pr_pollers(
        workspace_repo,
        Arc::clone(&fixture.project_repo),
        Arc::clone(&fixture.plan_branch_repo),
        Arc::clone(&fixture.registry),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MockChatService::new()),
        Arc::new(HashSet::new()),
    )
    .await;
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
    init_tracing();

    let project_repo: Arc<dyn ProjectRepository> = Arc::new(ProjectListErrorRepository);
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());

    cleanup_terminal_plan_branch_local_artifacts_on_startup(
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo),
        None,
        Arc::new(HashSet::new()),
        empty_running_agent_registry(),
    )
    .await;
    cleanup_terminal_agent_workspace_local_artifacts_on_startup(
        workspace_repo,
        plan_branch_repo,
        project_repo,
        None,
        Arc::new(HashSet::new()),
        empty_running_agent_registry(),
    )
    .await;
}

#[tokio::test]
async fn startup_terminal_plan_cleanup_continues_when_plan_branch_load_fails() {
    init_tracing();

    let project = cleanup_project();
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let plan_branch_repo: Arc<dyn PlanBranchRepository> = Arc::new(PlanBranchLoadErrorRepository);

    cleanup_terminal_plan_branch_local_artifacts_on_startup(
        plan_branch_repo,
        project_repo,
        None,
        Arc::new(HashSet::new()),
        empty_running_agent_registry(),
    )
    .await;
}

#[tokio::test]
async fn startup_terminal_workspace_cleanup_continues_when_workspace_load_fails() {
    init_tracing();

    let project = cleanup_project();
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(WorkspaceLoadErrorRepository);

    cleanup_terminal_agent_workspace_local_artifacts_on_startup(
        workspace_repo,
        Arc::new(MemoryPlanBranchRepository::new()),
        project_repo,
        None,
        Arc::new(HashSet::new()),
        empty_running_agent_registry(),
    )
    .await;
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_restarts_active_published_poller() {
    init_tracing();

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo_path).expect("create repo dir");
    run_git(&repo_path, &["init"]);
    run_git(&repo_path, &["config", "user.email", "test@example.com"]);
    run_git(&repo_path, &["config", "user.name", "Test User"]);
    run_git(&repo_path, &["checkout", "-b", "main"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "initial"]);

    let mut project = Project::new(
        "Startup Poller".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project.worktree_parent_directory = Some(
        temp_dir
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );

    let conversation_id = ChatConversationId::from_string("abababab-1111-2222-3333-cdcdcdcdcdcd");
    let branch_name = "ralphx/test/startup-agent-workspace-poller";
    let workspace = published_workspace(&project, conversation_id.clone(), branch_name);
    GitService::create_worktree(
        &repo_path,
        Path::new(&workspace.worktree_path),
        branch_name,
        "main",
    )
    .await
    .expect("create workspace worktree");

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&plan_branch_repo),
    ));

    recover_agent_workspace_pr_pollers(
        workspace_repo,
        project_repo,
        plan_branch_repo,
        Arc::clone(&registry),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MockChatService::new()),
        Arc::new(HashSet::new()),
    )
    .await;

    assert!(registry.is_agent_workspace_polling(&conversation_id));
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_does_not_poll_edit_workspace_source_pr() {
    init_tracing();

    let project = cleanup_project();
    let conversation_id = ChatConversationId::from_string("abababab-1212-2323-3434-cdcdcdcdcdcd");
    let mut workspace = published_workspace(
        &project,
        conversation_id.clone(),
        "ralphx/test/startup-edit-source-pr",
    );
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.publication_push_status = None;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 941,
        url: Some("https://github.com/owner/repo/pull/941".to_string()),
        title: Some("Base pull request".to_string()),
        head_ref_name: "feature/base-pr".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("base-head".to_string()),
    });
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let github = Arc::new(MockGithubService::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&plan_branch_repo),
    ));

    recover_one_agent_workspace_pr_poller(
        workspace,
        workspace_repo,
        project_repo,
        plan_branch_repo,
        Arc::clone(&registry),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MockChatService::new()),
        None,
        None,
        Arc::new(HashSet::new()),
    )
    .await;

    assert!(!registry.is_agent_workspace_polling(&conversation_id));
    assert_eq!(github.state().check_pr_status_calls, 0);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_with_autofix_disabled_skips_review_dispatch() {
    init_tracing();

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo_path).expect("create repo dir");
    run_git(&repo_path, &["init"]);
    run_git(&repo_path, &["config", "user.email", "test@example.com"]);
    run_git(&repo_path, &["config", "user.name", "Test User"]);
    run_git(&repo_path, &["checkout", "-b", "main"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "initial"]);

    let mut project = Project::new(
        "Startup Disabled Autofix Poller".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project.worktree_parent_directory = Some(
        temp_dir
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );

    let conversation_id = ChatConversationId::from_string("abababab-7777-8888-9999-cdcdcdcdcdcd");
    let branch_name = "ralphx/test/startup-disabled-autofix-poller";
    let mut workspace = published_workspace(&project, conversation_id.clone(), branch_name);
    workspace.pr_autofix_enabled = false;
    GitService::create_worktree(
        &repo_path,
        Path::new(&workspace.worktree_path),
        branch_name,
        "main",
    )
    .await
    .expect("create workspace worktree");

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(crate::domain::services::github_service::PrReviewFeedback {
        review_id: "startup-disabled-review".to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-07-20T12:00:00Z".to_string()),
        body: Some("Please address this review.".to_string()),
        comments: Vec::new(),
    });
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&plan_branch_repo),
    ));
    let chat = Arc::new(MockChatService::new());

    recover_agent_workspace_pr_pollers(
        workspace_repo,
        project_repo,
        plan_branch_repo,
        Arc::clone(&registry),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone(),
        Arc::new(HashSet::new()),
    )
    .await;

    assert!(chat.get_sent_messages().await.is_empty());
    assert!(registry.is_agent_workspace_polling(&conversation_id));
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_preserves_active_durable_repair_authority() {
    init_tracing();

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo_path).expect("create repo dir");
    run_git(&repo_path, &["init"]);
    run_git(&repo_path, &["config", "user.email", "test@example.com"]);
    run_git(&repo_path, &["config", "user.name", "Test User"]);
    run_git(&repo_path, &["checkout", "-b", "main"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "initial"]);

    let mut project = Project::new(
        "Startup Durable Repair Poller".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project.worktree_parent_directory = Some(
        temp_dir
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );

    let conversation_id = ChatConversationId::from_string("abababab-7777-8888-9999-dddddddddddd");
    let branch_name = "ralphx/test/startup-durable-repair-poller";
    let workspace = published_workspace(&project, conversation_id.clone(), branch_name);
    GitService::create_worktree(
        &repo_path,
        Path::new(&workspace.worktree_path),
        branch_name,
        "main",
    )
    .await
    .expect("create workspace worktree");

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let active_run = agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed active repair run");
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "main",
        false,
        true,
        false,
        None,
        Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Repairing;
    attempt.reserved_agent_run_id = Some(active_run.id.clone());
    let started = repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "existing durable repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed durable repair");
    let durable_attempt = match started {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected a new durable repair, got {outcome:?}"),
    };

    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(crate::domain::services::github_service::PrReviewFeedback {
        review_id: "startup-durable-review".to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-07-20T12:00:00Z".to_string()),
        body: Some("Please address this review.".to_string()),
        comments: Vec::new(),
    });
    github.state().fetch_pr_health_result = Some(Ok(PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: Some(PrMergeStateStatus::Clean),
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: branch_name.to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some("durable-repair-head".to_string()),
            base_ref_oid: None,
        },
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }));
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&plan_branch_repo),
    ));
    let chat = Arc::new(MockChatService::new());
    recover_agent_workspace_pr_pollers_with_notifications(
        workspace_repo.clone(),
        Arc::clone(&project_repo),
        Arc::clone(&plan_branch_repo),
        Arc::clone(&registry),
        agent_run_repo.clone(),
        chat.clone(),
        None,
        Some(Arc::clone(&repair_repo)),
        Arc::new(HashSet::new()),
    )
    .await;
    recover_agent_workspace_pr_pollers_with_notifications(
        workspace_repo.clone(),
        project_repo,
        plan_branch_repo,
        Arc::clone(&registry),
        agent_run_repo,
        chat.clone(),
        None,
        Some(Arc::clone(&repair_repo)),
        Arc::new(HashSet::new()),
    )
    .await;

    assert!(
        !registry.is_agent_workspace_polling(&conversation_id),
        "an active durable repair must prevent duplicate poller registration"
    );
    assert!(workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .expect("read monitor")
        .is_none());
    assert!(chat.get_sent_messages().await.is_empty());
    assert_eq!(github.state().check_pr_review_feedback_calls, 0);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert_eq!(github.state().push_branch_calls, 0);
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read publication events")
        .is_empty());
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read durable repair")
        .expect("durable repair should remain current");
    assert_eq!(current.id, durable_attempt.id);
    assert_eq!(current.generation, durable_attempt.generation);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Repairing);
    assert_eq!(current.reserved_agent_run_id, Some(active_run.id));
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_restarts_review_pr_monitor_poller() {
    init_tracing();

    let fixture = setup_review_pr_poller_recovery_fixture(
        "abababab-7777-8888-9999-cdcdcdcdcdcd",
        "ralphx/test/startup-review-pr-monitor",
    )
    .await;
    assert!(!fixture.workspace.pr_autofix_enabled);
    assert!(!fixture.workspace.pr_auto_merge_desired);
    assert!(fixture.workspace.auto_publish_enabled);
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        fixture.conversation_id.clone(),
        fixture.workspace.project_id.clone(),
        101,
        Some("old-head".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor.first_review_completed = true;
    monitor.last_reviewed_head_sha = Some("old-head".to_string());
    fixture
        .workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    recover_review_pr_poller_fixture(&fixture).await;

    assert!(fixture
        .registry
        .is_agent_workspace_polling(&fixture.conversation_id));
    fixture
        .registry
        .stop_agent_workspace_polling(&fixture.conversation_id);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_creates_missing_review_pr_monitor() {
    init_tracing();

    let fixture = setup_review_pr_poller_recovery_fixture(
        "abababab-1313-1414-1515-cdcdcdcdcdcd",
        "ralphx/test/startup-review-pr-missing-monitor",
    )
    .await;

    recover_review_pr_poller_fixture(&fixture).await;

    let monitor = fixture
        .workspace_repo
        .get_pr_review_monitor(&fixture.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("legacy Review PR workspace should be armed");
    assert!(monitor.monitor_enabled);
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
    assert_eq!(monitor.pr_number, 101);
    assert_eq!(monitor.last_seen_head_sha.as_deref(), Some("old-head"));
    assert!(fixture
        .registry
        .is_agent_workspace_polling(&fixture.conversation_id));
    fixture
        .registry
        .stop_agent_workspace_polling(&fixture.conversation_id);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_rearms_legacy_terminal_review_pr_monitor() {
    init_tracing();

    let fixture = setup_review_pr_poller_recovery_fixture(
        "abababab-1616-1717-1818-cdcdcdcdcdcd",
        "ralphx/test/startup-review-pr-terminal-monitor",
    )
    .await;
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        fixture.conversation_id.clone(),
        fixture.workspace.project_id.clone(),
        101,
        Some("old-head".to_string()),
    );
    monitor.monitor_enabled = false;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
    fixture
        .workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("legacy terminal monitor should persist");

    recover_review_pr_poller_fixture(&fixture).await;

    let monitor = fixture
        .workspace_repo
        .get_pr_review_monitor(&fixture.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("legacy terminal monitor should remain present");
    assert!(monitor.monitor_enabled);
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
    assert!(fixture.github.state().check_pr_status_calls >= 1);
    assert!(fixture
        .registry
        .is_agent_workspace_polling(&fixture.conversation_id));
    fixture
        .registry
        .stop_agent_workspace_polling(&fixture.conversation_id);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_settles_terminal_monitor_from_remote_authority() {
    init_tracing();

    let fixture = setup_review_pr_poller_recovery_fixture(
        "abababab-1818-1919-2020-cdcdcdcdcdcd",
        "ralphx/test/startup-review-pr-terminal-monitor-merged",
    )
    .await;
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        fixture.conversation_id.clone(),
        fixture.workspace.project_id.clone(),
        101,
        Some("old-head".to_string()),
    );
    monitor.monitor_enabled = false;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
    fixture
        .workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .unwrap();
    fixture
        .github
        .will_return_status(crate::domain::services::github_service::PrStatus::Merged {
            merge_commit_sha: Some("merge-sha".to_string()),
            merged_at: Some("2026-07-21T00:00:00Z".to_string()),
        });

    recover_review_pr_poller_fixture(&fixture).await;

    let workspace = fixture
        .workspace_repo
        .get_by_conversation_id(&fixture.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("merged"));
    assert_eq!(fixture.github.state().check_pr_status_calls, 1);
    assert!(!fixture
        .registry
        .is_agent_workspace_polling(&fixture.conversation_id));
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_preserves_paused_review_pr_monitor() {
    init_tracing();

    let fixture = setup_review_pr_poller_recovery_fixture(
        "abababab-1919-2020-2121-cdcdcdcdcdcd",
        "ralphx/test/startup-review-pr-paused-monitor",
    )
    .await;
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        fixture.conversation_id.clone(),
        fixture.workspace.project_id.clone(),
        101,
        Some("old-head".to_string()),
    );
    monitor.monitor_enabled = false;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Paused;
    fixture
        .workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("paused monitor should persist");

    recover_review_pr_poller_fixture(&fixture).await;

    let monitor = fixture
        .workspace_repo
        .get_pr_review_monitor(&fixture.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("paused monitor should remain present");
    assert!(!monitor.monitor_enabled);
    assert_eq!(monitor.status, AgentWorkspacePrReviewMonitorStatus::Paused);
    assert!(fixture
        .registry
        .is_agent_workspace_polling(&fixture.conversation_id));
    fixture
        .registry
        .stop_agent_workspace_polling(&fixture.conversation_id);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_converges_persisted_terminal_workspace() {
    init_tracing();

    let fixture = setup_review_pr_poller_recovery_fixture(
        "abababab-2222-2323-2424-cdcdcdcdcdcd",
        "ralphx/test/startup-review-pr-persisted-terminal",
    )
    .await;
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        fixture.conversation_id.clone(),
        fixture.workspace.project_id.clone(),
        101,
        Some("old-head".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    fixture
        .workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .unwrap();
    let action = fixture
        .workspace_repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            fixture.conversation_id.clone(),
            101,
            "old-head".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "Approve".to_string(),
            "Looks good".to_string(),
            None,
            Some("run-terminal".to_string()),
        ))
        .await
        .unwrap();
    let mut terminal_workspace = fixture.workspace.clone();
    terminal_workspace.publication_pr_status = Some("merged".to_string());
    fixture
        .workspace_repo
        .create_or_update(terminal_workspace)
        .await
        .unwrap();

    recover_review_pr_poller_fixture(&fixture).await;

    assert_eq!(
        fixture
            .workspace_repo
            .get_pr_review_monitor(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkspacePrReviewMonitorStatus::Terminal
    );
    assert_eq!(
        fixture
            .workspace_repo
            .get_pr_review_action(&action.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkspacePrReviewActionStatus::Superseded
    );
    assert!(!fixture
        .registry
        .is_agent_workspace_polling(&fixture.conversation_id));
    assert_eq!(fixture.github.state().check_pr_status_calls, 0);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_skips_orphaned_review_pr_monitor() {
    init_tracing();

    let project = cleanup_project();
    let conversation_id = ChatConversationId::from_string("abababab-1010-1111-1212-cdcdcdcdcdcd");
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project.id.clone(),
        101,
        Some("old-head".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("orphaned monitor should persist");

    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let github = Arc::new(MockGithubService::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&plan_branch_repo),
    ));

    recover_agent_workspace_pr_pollers(
        workspace_repo,
        project_repo,
        plan_branch_repo,
        Arc::clone(&registry),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MockChatService::new()),
        Arc::new(HashSet::new()),
    )
    .await;

    assert!(!registry.is_agent_workspace_polling(&conversation_id));
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_tolerates_workspace_and_monitor_listing_errors() {
    init_tracing();

    let project = cleanup_project();
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(WorkspaceLoadErrorRepository);
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let github = Arc::new(MockGithubService::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&plan_branch_repo),
    ));

    recover_agent_workspace_pr_pollers(
        workspace_repo,
        project_repo,
        plan_branch_repo,
        Arc::clone(&registry),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MockChatService::new()),
        Arc::new(HashSet::new()),
    )
    .await;

    assert_eq!(github.state().fetch_pr_health_calls, 0);
}

#[tokio::test]
async fn startup_agent_workspace_pr_recovery_restarts_supervised_ideation_poller() {
    init_tracing();

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo_path).expect("create repo dir");
    run_git(&repo_path, &["init"]);
    run_git(&repo_path, &["config", "user.email", "test@example.com"]);
    run_git(&repo_path, &["config", "user.name", "Test User"]);
    run_git(&repo_path, &["checkout", "-b", "main"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "initial"]);

    let mut project = Project::new(
        "Startup Ideation Poller".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project.worktree_parent_directory = Some(
        temp_dir
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );

    let conversation_id = ChatConversationId::from_string("abababab-4444-5555-6666-cdcdcdcdcdcd");
    let plan_branch_name = "feature/startup-ideation-poller-plan";
    run_git(&repo_path, &["checkout", "-b", plan_branch_name]);
    std::fs::write(repo_path.join("plan.txt"), "plan branch\n").expect("write plan fixture");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "plan branch"]);
    run_git(&repo_path, &["checkout", "main"]);

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-startup-ideation-poller"),
        IdeationSessionId::from_string("session-startup-ideation-poller"),
        project.id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.id = PlanBranchId::from_string("plan-branch-ideation");
    plan_branch.pr_number = Some(101);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/101".to_string());
    plan_branch.pr_status = Some(PlanPrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let expected_plan_worktree =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("plan worktree path should resolve");

    let mut workspace = published_workspace(
        &project,
        conversation_id.clone(),
        "ralphx/test/startup-ideation-poller",
    );
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-ideation"));
    workspace.pr_autofix_enabled = true;

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should persist");
    let github = Arc::new(MockGithubService::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&plan_branch_repo),
    ));

    recover_agent_workspace_pr_pollers(
        workspace_repo,
        project_repo,
        plan_branch_repo,
        Arc::clone(&registry),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MockChatService::new()),
        Arc::new(HashSet::new()),
    )
    .await;

    assert!(registry.is_agent_workspace_polling(&conversation_id));
    assert_eq!(
        GitService::get_current_branch(&repo_path)
            .await
            .expect("root branch should be readable"),
        "main"
    );
    assert_eq!(
        GitService::get_current_branch(&expected_plan_worktree)
            .await
            .expect("plan worktree branch should be readable"),
        plan_branch_name
    );
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn startup_terminal_plan_cleanup_records_safety_skip_reports() {
    init_tracing();

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

    cleanup_terminal_plan_branch_local_artifacts_on_startup(
        plan_branch_repo,
        project_repo,
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(HashSet::new()),
        empty_running_agent_registry(),
    )
    .await;

    assert_eq!(github.state().fetch_remote_calls, 0);
}

#[tokio::test]
async fn startup_terminal_workspace_cleanup_records_safety_skip_reports() {
    init_tracing();

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

    cleanup_terminal_agent_workspace_local_artifacts_on_startup(
        workspace_repo,
        Arc::new(MemoryPlanBranchRepository::new()),
        project_repo,
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(HashSet::new()),
        empty_running_agent_registry(),
    )
    .await;

    assert_eq!(github.state().fetch_remote_calls, 0);
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
    async fn set_last_blocked_pr_health_fingerprint(
        &self,
        _conversation_id: &ChatConversationId,
        _fingerprint: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn set_review_automation_override(
        &self,
        _conversation_id: &ChatConversationId,
        _value: Option<bool>,
    ) -> AppResult<()> {
        Err(repo_error())
    }
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

    async fn list_active_pr_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
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

    async fn update_pr_supervision_preferences(
        &self,
        _conversation_id: &ChatConversationId,
        _autofix_enabled: bool,
        _auto_merge_desired: bool,
        _auto_merge_method: &str,
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

    async fn set_pr_review_auto_approve_enabled(
        &self,
        _conversation_id: &ChatConversationId,
        _enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        Err(repo_error())
    }

    async fn mark_pr_review_first_action_resolved(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        Err(repo_error())
    }

    async fn claim_pending_pr_review_action(&self, _action_id: &str) -> AppResult<bool> {
        Ok(false)
    }

    async fn delete(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Err(repo_error())
    }
}

#[tokio::test]
async fn workspace_repository_default_linked_ideation_lookup_returns_none() {
    let repo = WorkspaceLoadErrorRepository;
    let loaded = repo
        .get_by_linked_ideation_session_id(&IdeationSessionId::from_string("session-1"))
        .await
        .unwrap();

    assert!(loaded.is_none());
}
