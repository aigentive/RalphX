use std::collections::HashSet;
use std::path::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::application::agent_conversation_workspace::{
    resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
};
use crate::application::agent_workspace_pr_supervision_recovery::{
    pr_supervision_recovery_schedule_skip_reason, recover_agent_workspace_pr_supervision,
    recover_recent_agent_workspace_pr_supervision_on_startup,
    schedule_agent_workspace_pr_supervision_recovery,
    schedule_agent_workspace_pr_supervision_recovery_with_lazy_deps,
    AgentWorkspacePrFixReviewPublishResumer, AgentWorkspacePrSupervisionRecoveryDeps,
    AgentWorkspacePrSupervisionRecoveryOutcome, AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::agent_workspace_review::resolve_review_target;
use crate::application::chat_service::MockChatService;
use crate::application::git_service::GitService;
use crate::application::services::PrPollerRegistry;
use crate::application::AppState;
use crate::domain::entities::plan_branch::{
    PrPushStatus as PlanPrPushStatus, PrStatus as PlanPrStatus,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, AgentRun,
    AgentRunActionKind, AgentRunStatus, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairOperationHoldReason,
    AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch,
    PlanBranchId, PlanBranchStatus, Project, ProjectId, TaskId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceRepairRepository,
    PlanBranchRepository, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use crate::domain::services::github_service::{
    PrHealth, PrHealthCheck, PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
};
use crate::domain::services::GithubServiceTrait;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryPlanBranchRepository, MemoryProjectRepository,
};
use crate::tests::mock_github_service::MockGithubService;

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

fn init_repo(repo_path: &Path) {
    std::fs::create_dir_all(repo_path).expect("create repo dir");
    run_git(repo_path, &["init"]);
    run_git(repo_path, &["config", "user.email", "test@example.com"]);
    run_git(repo_path, &["config", "user.name", "Test User"]);
    run_git(repo_path, &["checkout", "-b", "main"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
    run_git(repo_path, &["add", "."]);
    run_git(repo_path, &["commit", "-m", "initial"]);
}

fn recovery_project(temp_dir: &tempfile::TempDir, repo_path: &Path, name: &str) -> Project {
    let mut project = Project::new(name.to_string(), repo_path.to_string_lossy().to_string());
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project.worktree_parent_directory = Some(
        temp_dir
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );
    project
}

fn blocked_workspace(
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
    workspace.publication_pr_number = Some(257);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/257".to_string());
    workspace.publication_pr_status = Some("failed".to_string());
    workspace.publication_push_status = Some("failed".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_autofix_enabled = true;
    workspace
}

fn open_sync_state(branch_name: &str, head_sha: &str) -> PrSyncState {
    PrSyncState {
        status: PrStatus::Open,
        merge_state_status: Some(PrMergeStateStatus::Clean),
        mergeable: Some(PrMergeableState::Mergeable),
        is_draft: false,
        head_ref_name: branch_name.to_string(),
        base_ref_name: "main".to_string(),
        head_ref_oid: Some(head_sha.to_string()),
        base_ref_oid: None,
    }
}

fn healthy_pr_health(branch_name: &str, head_sha: &str) -> PrHealth {
    PrHealth {
        sync_state: open_sync_state(branch_name, head_sha),
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }
}

fn recovery_deps(
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    project_repo: Arc<MemoryProjectRepository>,
    github: Arc<MockGithubService>,
    agent_run_repo: Arc<MemoryAgentRunRepository>,
) -> AgentWorkspacePrSupervisionRecoveryDeps {
    AgentWorkspacePrSupervisionRecoveryDeps {
        workspace_repo: workspace_repo as Arc<dyn AgentConversationWorkspaceRepository>,
        project_repo,
        plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
            as Arc<dyn PlanBranchRepository>,
        github: github as Arc<dyn GithubServiceTrait>,
        pr_poller_registry: None,
        transition_service: None,
        chat_service: None,
        agent_run_repo,
        events: Arc::new(ralphx_events::NullEventSink),
        pr_fix_review_publish_resumer: None,
        durable_recovery_state: None,
    }
}

struct RecordingReviewPublishResumer {
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    calls: AtomicUsize,
}

impl RecordingReviewPublishResumer {
    fn new(workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>) -> Self {
        Self {
            workspace_repo,
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl AgentWorkspacePrFixReviewPublishResumer for RecordingReviewPublishResumer {
    async fn publish_pr_fix_after_workspace_review(
        &self,
        conversation_id: ChatConversationId,
    ) -> Result<Option<bool>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.workspace_repo
            .update_publication(
                &conversation_id,
                Some(681),
                Some("https://github.com/owner/repo/pull/681"),
                Some("open"),
                Some("pushed"),
            )
            .await
            .map_err(|error| error.to_string())?;
        self.workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id,
                "published",
                "succeeded",
                "Mock publish completed.",
                Some("published:681".to_string()),
            ))
            .await
            .map_err(|error| error.to_string())?;
        Ok(Some(true))
    }
}

async fn setup_recovery_workspace(
    name: &str,
) -> (
    tempfile::TempDir,
    Project,
    AgentConversationWorkspace,
    String,
) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    init_repo(&repo_path);
    let project = recovery_project(&temp_dir, &repo_path, name);
    let conversation_id = ChatConversationId::new();
    let branch_name = format!("ralphx/test/{name}");
    let workspace = blocked_workspace(&project, conversation_id, &branch_name);
    GitService::create_worktree(
        &repo_path,
        Path::new(&workspace.worktree_path),
        &branch_name,
        "main",
    )
    .await
    .expect("create workspace worktree");
    let head_sha = GitService::get_head_sha(Path::new(&workspace.worktree_path))
        .await
        .expect("read workspace head");
    (temp_dir, project, workspace, head_sha)
}

async fn setup_linked_plan_recovery_workspace(
    name: &str,
    pr_number: i64,
) -> (
    tempfile::TempDir,
    Project,
    AgentConversationWorkspace,
    PlanBranch,
    String,
) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    init_repo(&repo_path);
    let project = recovery_project(&temp_dir, &repo_path, name);
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::from_string(format!("session-{name}"));
    let plan_branch_id = PlanBranchId::from_string(format!("plan-branch-{name}"));
    let branch_name = format!("ralphx/test/plan-{name}");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string(format!("artifact-{name}")),
        session_id.clone(),
        project.id.clone(),
        branch_name.clone(),
        "main".to_string(),
    );
    plan_branch.id = plan_branch_id.clone();
    plan_branch.pr_eligible = true;
    plan_branch.merge_task_id = Some(TaskId::from_string(format!("merge-task-{name}")));
    plan_branch.pr_number = Some(pr_number);
    plan_branch.pr_url = Some(format!("https://github.com/owner/repo/pull/{pr_number}"));
    plan_branch.pr_status = Some(PlanPrStatus::Open);
    plan_branch.pr_push_status = PlanPrPushStatus::Failed;

    let plan_worktree = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("plan worktree path should resolve");
    GitService::create_worktree(&repo_path, &plan_worktree, &branch_name, "main")
        .await
        .expect("create linked plan worktree");
    let head_sha = GitService::get_head_sha(&plan_worktree)
        .await
        .expect("read plan worktree head");

    let mut workspace = blocked_workspace(&project, conversation_id, &branch_name);
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch_id);
    workspace.worktree_path = plan_worktree.to_string_lossy().to_string();
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.publication_push_status = None;
    workspace.pr_supervision_status = Some("blocked".to_string());

    (temp_dir, project, workspace, plan_branch, head_sha)
}

async fn wait_for_sync_state_calls(github: &MockGithubService, expected: u32) {
    for _ in 0..100 {
        if github.state().check_pr_sync_state_calls >= expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "expected at least {expected} PR sync-state lookups, got {}",
        github.state().check_pr_sync_state_calls
    );
}

#[test]
fn schedule_skip_reason_covers_recoverable_and_terminal_workspace_shapes() {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("schedule-skip-base"),
        ProjectId::from_string("project-schedule-skip".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/schedule-skip".to_string(),
        "/tmp/schedule-skip".to_string(),
    );
    workspace.publication_pr_number = Some(41);
    workspace.publication_push_status = Some("failed".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_autofix_enabled = true;
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&workspace),
        None
    );

    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&workspace),
        None
    );

    workspace.publication_push_status = Some("refreshed".to_string());
    workspace.pr_supervision_status = Some("reviewing".to_string());
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&workspace),
        None
    );

    workspace.publication_push_status = Some("pushed".to_string());
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&workspace),
        Some("workspace_push_not_recoverable")
    );

    let mut inactive = workspace.clone();
    inactive.status = AgentConversationWorkspaceStatus::Archived;
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&inactive),
        Some("workspace_not_active")
    );

    let mut chat_mode = workspace.clone();
    chat_mode.mode = AgentConversationWorkspaceMode::Chat;
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&chat_mode),
        Some("workspace_not_edit_or_ideation_mode")
    );

    let mut plan_owned = workspace.clone();
    plan_owned.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-owned"));
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&plan_owned),
        Some("workspace_linked_to_plan_branch")
    );

    let mut missing_pr = workspace.clone();
    missing_pr.publication_pr_number = None;
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&missing_pr),
        Some("missing_pr_number")
    );

    let mut terminal = workspace.clone();
    terminal.publication_pr_status = Some("merged".to_string());
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&terminal),
        Some("workspace_terminal")
    );

    let mut auto_publish_paused = workspace.clone();
    auto_publish_paused.auto_publish_enabled = false;
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&auto_publish_paused),
        Some("auto_publish_disabled")
    );

    let mut disabled = workspace;
    disabled.pr_autofix_enabled = false;
    disabled.pr_auto_merge_desired = false;
    assert_eq!(
        pr_supervision_recovery_schedule_skip_reason(&disabled),
        Some("pr_supervision_disabled")
    );
}

#[tokio::test]
async fn scheduled_recovery_claims_conversation_once_until_background_task_finishes() {
    let (_temp_dir, project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-scheduled").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
    github.state().fetch_pr_health_result =
        Some(Ok(healthy_pr_health(&workspace.branch_name, &head_sha)));
    let deps = recovery_deps(
        workspace_repo,
        project_repo,
        Arc::clone(&github),
        Arc::new(MemoryAgentRunRepository::new()),
    );

    let factory_calls = Arc::new(AtomicUsize::new(0));
    let first_factory_calls = Arc::clone(&factory_calls);
    let first_deps = deps.clone();
    schedule_agent_workspace_pr_supervision_recovery_with_lazy_deps(
        move || {
            first_factory_calls.fetch_add(1, Ordering::SeqCst);
            first_deps
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        true,
    );
    let duplicate_factory_calls = Arc::clone(&factory_calls);
    schedule_agent_workspace_pr_supervision_recovery_with_lazy_deps(
        move || {
            duplicate_factory_calls.fetch_add(1, Ordering::SeqCst);
            deps
        },
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        true,
    );

    wait_for_sync_state_calls(&github, 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(github.state().check_pr_sync_state_calls, 1);
    assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn recovers_blocked_pr_supervision_when_remote_head_matches_local_workspace() {
    let (_temp_dir, project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-recover").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));

    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo,
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github,
            pr_poller_registry: Some(Arc::clone(&registry)),
            transition_service: None,
            chat_service: Some(Arc::new(MockChatService::new())),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: None,
            durable_recovery_state: None,
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("recover supervision");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
            pr_number: 257,
            head_sha,
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.publication_pr_status.as_deref(), Some("open"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "pr_supervision_recovered");
    assert!(registry.is_agent_workspace_polling(&conversation_id));
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn matching_remote_head_with_failing_health_stays_blocked_and_never_reports_recovered() {
    let (_temp_dir, project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-unresolved-health").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    let sync_state = open_sync_state(&workspace.branch_name, &head_sha);
    github.will_return_sync_state(sync_state.clone());
    github.state().fetch_pr_health_result = Some(Ok(PrHealth {
        sync_state,
        review_decision: None,
        checks: vec![PrHealthCheck {
            name: "Rust Tests".to_string(),
            status: Some("completed".to_string()),
            conclusion: Some("failure".to_string()),
            details_url: None,
        }],
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }));
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));

    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo,
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github,
            pr_poller_registry: Some(Arc::clone(&registry)),
            transition_service: None,
            chat_service: Some(Arc::new(MockChatService::new())),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: None,
            durable_recovery_state: None,
        },
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("inspect unresolved PR health");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("pr_issue_unresolved")
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert!(events
        .iter()
        .all(|event| event.step != "pr_supervision_recovered"));
    assert!(registry.is_agent_workspace_polling(&conversation_id));
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn recovers_linked_plan_pr_supervision_without_workspace_publication_pr() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    init_repo(&repo_path);
    let project = recovery_project(&temp_dir, &repo_path, "plan-pr-supervision-recover");
    let conversation_id = ChatConversationId::from_string("conversation-plan-pr-recover");
    let session_id = IdeationSessionId::from_string("session-plan-pr-recover");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-pr-recover");
    let branch_name = "ralphx/test/plan-pr-recover";
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-pr-recover"),
        session_id.clone(),
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.id = plan_branch_id.clone();
    plan_branch.pr_eligible = true;
    plan_branch.merge_task_id = Some(TaskId::from_string(
        "merge-task-plan-pr-recover".to_string(),
    ));
    plan_branch.pr_number = Some(602);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/602".to_string());
    plan_branch.pr_status = Some(PlanPrStatus::Open);
    plan_branch.pr_push_status = PlanPrPushStatus::Failed;
    let plan_worktree = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("plan worktree path should resolve");
    GitService::create_worktree(&repo_path, &plan_worktree, branch_name, "main")
        .await
        .expect("create linked plan worktree");
    let head_sha = GitService::get_head_sha(&plan_worktree)
        .await
        .expect("read plan worktree head");

    let mut workspace = blocked_workspace(&project, conversation_id.clone(), branch_name);
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch_id.clone());
    workspace.worktree_path = plan_worktree.to_string_lossy().to_string();
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.publication_push_status = None;
    workspace.pr_supervision_status = Some("blocked".to_string());

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("seed plan branch");
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(branch_name, &head_sha));

    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo: Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            plan_branch_repo: Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
            github: github as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: None,
            transition_service: None,
            chat_service: None,
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: None,
            durable_recovery_state: None,
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("recover linked plan PR supervision");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
            pr_number: 602,
            head_sha,
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_push_status, None);
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    let updated_plan = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch should exist");
    assert_eq!(updated_plan.pr_status, Some(PlanPrStatus::Open));
    assert_eq!(updated_plan.pr_push_status, PlanPrPushStatus::Pushed);
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert!(events.iter().any(|event| {
        event.step == "pr_supervision_recovered"
            && event
                .classification
                .as_deref()
                .unwrap_or_default()
                .starts_with("github_pr_supervision_recovered:602:")
    }));
}

#[tokio::test]
async fn marks_terminal_linked_plan_pr_status_and_workspace_authority() {
    let cases = [
        (
            "plan-pr-supervision-terminal-merged",
            PrStatus::Merged {
                merge_commit_sha: Some("merge-sha".to_string()),
                merged_at: None,
            },
            PlanPrStatus::Merged,
            "merged",
            "pr_merged",
        ),
        (
            "plan-pr-supervision-terminal-closed",
            PrStatus::Closed,
            PlanPrStatus::Closed,
            "closed",
            "pr_closed",
        ),
    ];

    for (name, remote_status, expected_plan_status, expected_status, expected_step) in cases {
        let (_temp_dir, project, workspace, plan_branch, head_sha) =
            setup_linked_plan_recovery_workspace(name, 702).await;
        let conversation_id = workspace.conversation_id.clone();
        let plan_branch_id = plan_branch.id.clone();
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
        plan_branch_repo
            .create(plan_branch)
            .await
            .expect("seed plan branch");
        let github = Arc::new(MockGithubService::new());
        let mut sync_state = open_sync_state(&workspace.branch_name, &head_sha);
        sync_state.status = remote_status;
        github.will_return_sync_state(sync_state);

        let outcome = recover_agent_workspace_pr_supervision(
            AgentWorkspacePrSupervisionRecoveryDeps {
                workspace_repo: Arc::clone(&workspace_repo)
                    as Arc<dyn AgentConversationWorkspaceRepository>,
                project_repo: Arc::new(MemoryProjectRepository::with_projects(vec![project])),
                plan_branch_repo: Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
                github: github as Arc<dyn GithubServiceTrait>,
                pr_poller_registry: None,
                transition_service: None,
                chat_service: None,
                agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
                events: Arc::new(ralphx_events::NullEventSink),
                pr_fix_review_publish_resumer: None,
                durable_recovery_state: None,
            },
            conversation_id.clone(),
            AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        )
        .await
        .expect("terminal linked plan PR status should update plan branch");

        assert_eq!(
            outcome,
            AgentWorkspacePrSupervisionRecoveryOutcome::Terminal {
                pr_number: 702,
                pr_status: expected_status.to_string(),
            }
        );
        let updated = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should still exist");
        assert_eq!(updated.publication_pr_number, Some(702));
        assert_eq!(
            updated.publication_pr_status.as_deref(),
            Some(expected_status)
        );
        assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
        assert!(updated.pr_supervision_status.is_none());
        assert!(updated.pr_supervision_summary.is_none());
        let updated_plan = plan_branch_repo
            .get_by_id(&plan_branch_id)
            .await
            .unwrap()
            .expect("plan branch should exist");
        assert_eq!(updated_plan.pr_status, Some(expected_plan_status));
        assert_eq!(updated_plan.pr_push_status, PlanPrPushStatus::Pushed);
        let events = workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| event.step == expected_step));
    }
}

#[tokio::test]
async fn skips_linked_plan_pr_supervision_when_plan_branch_is_not_current() {
    let cases = [
        ("missing-plan-row", "linked_plan_branch_missing"),
        ("inactive-plan", "linked_plan_branch_not_current"),
        ("closed-plan-pr", "linked_plan_branch_not_current"),
        ("session-mismatch", "linked_plan_branch_not_current"),
        ("branch-mismatch", "linked_plan_branch_not_current"),
        ("missing-pr-number", "missing_pr_number"),
    ];

    for (name, expected_reason) in cases {
        let (_temp_dir, project, mut workspace, mut plan_branch, _head_sha) =
            setup_linked_plan_recovery_workspace(name, 703).await;
        match name {
            "inactive-plan" => plan_branch.status = PlanBranchStatus::Abandoned,
            "closed-plan-pr" => plan_branch.pr_status = Some(PlanPrStatus::Closed),
            "session-mismatch" => {
                workspace.linked_ideation_session_id =
                    Some(IdeationSessionId::from_string("other-session"));
            }
            "branch-mismatch" => {
                workspace.branch_name = "ralphx/test/different-plan-branch".to_string();
            }
            "missing-pr-number" => plan_branch.pr_number = None,
            _ => {}
        }
        let conversation_id = workspace.conversation_id.clone();
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
        if name != "missing-plan-row" {
            plan_branch_repo
                .create(plan_branch)
                .await
                .expect("seed plan branch");
        }
        let github = Arc::new(MockGithubService::new());

        let outcome = recover_agent_workspace_pr_supervision(
            AgentWorkspacePrSupervisionRecoveryDeps {
                workspace_repo: workspace_repo as Arc<dyn AgentConversationWorkspaceRepository>,
                project_repo: Arc::new(MemoryProjectRepository::with_projects(vec![project])),
                plan_branch_repo: plan_branch_repo as Arc<dyn PlanBranchRepository>,
                github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
                pr_poller_registry: None,
                transition_service: None,
                chat_service: None,
                agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
                events: Arc::new(ralphx_events::NullEventSink),
                pr_fix_review_publish_resumer: None,
                durable_recovery_state: None,
            },
            conversation_id,
            AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        )
        .await
        .expect("linked plan recovery should skip stale linkage");

        assert_eq!(
            outcome,
            AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(expected_reason)
        );
        assert_eq!(github.state().check_pr_sync_state_calls, 0);
    }
}

#[tokio::test]
async fn recovers_stale_needs_agent_repair_without_rearming_pr_supervision() {
    let (_temp_dir, project, mut workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-needs-agent").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(head_sha.clone());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let repair_run = agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed repair run");
    agent_run_repo
        .fail(&repair_run.id, "repair agent exited")
        .await
        .expect("mark repair run failed");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));

    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            project_repo,
            github,
            agent_run_repo,
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("recover stale needs-agent supervision");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("stale_repair_recovered")
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.publication_pr_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.step == "stale_repair_recovered"));
    assert!(!events
        .iter()
        .any(|event| event.step == "pr_supervision_recovered"));
    assert!(!events.iter().any(|event| {
        matches!(
            event.step.as_str(),
            "pr_autofix_completed" | "pr_autofix_published"
        )
    }));
}

#[tokio::test]
async fn durable_base_update_authority_runs_before_legacy_pr_supervision_gates() {
    let (_temp_dir, project, mut workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-durable-authority").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(head_sha);
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.auto_publish_enabled = false;
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let active_run = agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed active repair run");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Repairing;
    attempt.reserved_agent_run_id = Some(active_run.id.clone());
    let durable_attempt = match repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "existing durable repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed durable repair")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected a new durable repair, got {outcome:?}"),
    };

    let mut durable_state = AppState::new_test();
    durable_state.agent_conversation_workspace_repo = workspace_repo.clone();
    durable_state.agent_workspace_repair_repo = repair_repo.clone();
    durable_state.agent_run_repo = agent_run_repo.clone();
    let github = Arc::new(MockGithubService::new());
    let mut deps = recovery_deps(
        Arc::clone(&workspace_repo),
        Arc::new(MemoryProjectRepository::with_projects(vec![project])),
        Arc::clone(&github),
        Arc::clone(&agent_run_repo),
    );
    deps.durable_recovery_state = Some(Arc::new(durable_state));

    let outcome = recover_agent_workspace_pr_supervision(
        deps,
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("durable authority should recover without legacy replay");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("durable_repair_active")
    );
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read events")
        .is_empty());
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read durable attempt")
        .expect("durable attempt remains current");
    assert_eq!(current.id, durable_attempt.id);
    assert_eq!(current.generation, durable_attempt.generation);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Repairing);
    assert_eq!(current.reserved_agent_run_id, Some(active_run.id));
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
}

#[tokio::test]
async fn settled_durable_repair_boundaries_recover_pr_supervision() {
    for (name, phase) in [
        (
            "pr-supervision-durable-ready",
            AgentWorkspaceRepairPhase::Ready,
        ),
        (
            "pr-supervision-durable-blocked",
            AgentWorkspaceRepairPhase::Blocked,
        ),
    ] {
        let (_temp_dir, project, workspace, head_sha) = setup_recovery_workspace(name).await;
        let conversation_id = workspace.conversation_id.clone();
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
        let mut attempt = AgentWorkspaceRepairAttempt::new(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::BaseUpdate,
            AgentWorkspaceRepairContinuation::UpdateOnly,
            "main",
            false,
            true,
            false,
            None,
            chrono::Utc::now(),
        );
        attempt.phase = phase;
        let durable_attempt = match repair_repo
            .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
                attempt,
                reason: "settled durable repair boundary".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("seed durable repair")
        {
            StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
            outcome => panic!("expected a new durable repair, got {outcome:?}"),
        };

        let mut durable_state = AppState::new_test();
        durable_state.agent_conversation_workspace_repo = workspace_repo.clone();
        durable_state.agent_workspace_repair_repo = repair_repo.clone();
        durable_state.agent_run_repo = agent_run_repo.clone();
        let github = Arc::new(MockGithubService::new());
        github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
        github.state().fetch_pr_health_result =
            Some(Ok(healthy_pr_health(&workspace.branch_name, &head_sha)));
        let mut deps = recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            Arc::clone(&github),
            Arc::clone(&agent_run_repo),
        );
        deps.durable_recovery_state = Some(Arc::new(durable_state));

        let outcome = recover_agent_workspace_pr_supervision(
            deps,
            conversation_id.clone(),
            AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
        )
        .await
        .expect("settled durable repair should allow PR supervision recovery");

        assert_eq!(
            outcome,
            AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
                pr_number: 257,
                head_sha: head_sha.clone(),
            }
        );
        let current = repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("read durable attempt")
            .expect("durable attempt remains current");
        assert_eq!(current.id, durable_attempt.id);
        assert_eq!(current.phase, phase);
        assert_eq!(current.reserved_agent_run_id, None);
        assert_eq!(current.dispatch_count, 0);
        assert_eq!(github.state().check_pr_sync_state_calls, 1);
        assert_eq!(github.state().fetch_pr_health_calls, 1);
    }
}

#[tokio::test]
async fn startup_recovery_preserves_held_pr_autofix_attempt() {
    let (_temp_dir, project, mut workspace, _head_sha) =
        setup_recovery_workspace("pr-supervision-durable-held").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.pr_supervision_status = Some("held".to_string());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Ready;
    attempt.ci_rerun_count = 1;
    attempt.ci_rerun_fingerprint = Some("ci-rerun:257".to_string());
    let durable_attempt = match repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "CI rerun reserved".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed held repair")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected a held repair, got {outcome:?}"),
    };

    let mut durable_state = AppState::new_test();
    durable_state.agent_conversation_workspace_repo = workspace_repo.clone();
    durable_state.agent_workspace_repair_repo = repair_repo.clone();
    durable_state.agent_run_repo = agent_run_repo.clone();
    let github = Arc::new(MockGithubService::new());
    let mut deps = recovery_deps(
        Arc::clone(&workspace_repo),
        Arc::new(MemoryProjectRepository::with_projects(vec![project])),
        Arc::clone(&github),
        Arc::clone(&agent_run_repo),
    );
    deps.durable_recovery_state = Some(Arc::new(durable_state));

    let outcome = recover_agent_workspace_pr_supervision(
        deps,
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("held repair recovery should be a no-op");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("durable_repair_held")
    );
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read held attempt")
        .expect("held attempt remains current");
    assert_eq!(current.id, durable_attempt.id);
    assert!(current.is_unsettled());
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(
        current.operation_snapshot().hold_reason,
        Some(AgentWorkspaceRepairOperationHoldReason::CiRerunPending)
    );
    let current_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("read workspace")
        .expect("workspace remains current");
    assert_eq!(
        current_workspace.pr_supervision_status.as_deref(),
        Some("held")
    );
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read publication events")
        .is_empty());
    assert!(agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("read agent runs")
        .is_empty());
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
}

#[tokio::test]
async fn dispatching_durable_repair_authority_vetoes_pr_supervision_recovery() {
    let (_temp_dir, project, workspace, _head_sha) =
        setup_recovery_workspace("pr-supervision-durable-dispatching").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let active_run = agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed active dispatch run");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Dispatching;
    attempt.reserved_agent_run_id = Some(active_run.id.clone());
    let durable_attempt = match repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "in-flight durable repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed durable repair")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected a new durable repair, got {outcome:?}"),
    };

    let mut durable_state = AppState::new_test();
    durable_state.agent_conversation_workspace_repo = workspace_repo.clone();
    durable_state.agent_workspace_repair_repo = repair_repo.clone();
    durable_state.agent_run_repo = agent_run_repo.clone();
    let github = Arc::new(MockGithubService::new());
    let mut deps = recovery_deps(
        Arc::clone(&workspace_repo),
        Arc::new(MemoryProjectRepository::with_projects(vec![project])),
        Arc::clone(&github),
        Arc::clone(&agent_run_repo),
    );
    deps.durable_recovery_state = Some(Arc::new(durable_state));

    let outcome = recover_agent_workspace_pr_supervision(
        deps,
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("in-flight durable repair should veto PR supervision recovery");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("durable_repair_active")
    );
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read durable attempt")
        .expect("durable attempt remains current");
    assert_eq!(current.id, durable_attempt.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Dispatching);
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
}

#[tokio::test]
async fn durable_recovery_without_repair_attempt_recovers_pr_supervision() {
    let (_temp_dir, project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-durable-no-attempt").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut durable_state = AppState::new_test();
    durable_state.agent_conversation_workspace_repo = workspace_repo.clone();
    durable_state.agent_workspace_repair_repo = workspace_repo.clone();
    durable_state.agent_run_repo = agent_run_repo.clone();
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
    github.state().fetch_pr_health_result =
        Some(Ok(healthy_pr_health(&workspace.branch_name, &head_sha)));
    let mut deps = recovery_deps(
        Arc::clone(&workspace_repo),
        Arc::new(MemoryProjectRepository::with_projects(vec![project])),
        Arc::clone(&github),
        agent_run_repo,
    );
    deps.durable_recovery_state = Some(Arc::new(durable_state));

    let outcome = recover_agent_workspace_pr_supervision(
        deps,
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("missing durable repair should allow PR supervision recovery");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
            pr_number: 257,
            head_sha,
        }
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 1);
    assert_eq!(github.state().fetch_pr_health_calls, 1);
}

#[tokio::test]
async fn retry_eligible_stale_pr_autofix_settlement_restarts_pr_polling() {
    let (_temp_dir, project, mut workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-retry-eligible").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(head_sha.clone());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    let fingerprint = "github_pr_autofix:257:head:retry-eligible";

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut failed_autofix = AgentRun::new(conversation_id.clone());
    failed_autofix.action_kind = Some(AgentRunActionKind::PrAutofix);
    failed_autofix.action_context_id = Some("257".to_string());
    failed_autofix.action_target_id = Some(fingerprint.to_string());
    let failed_autofix = agent_run_repo
        .create(failed_autofix)
        .await
        .expect("seed failed autofix");
    agent_run_repo
        .fail(&failed_autofix.id, "autofix failed")
        .await
        .expect("fail autofix");

    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
    github.state().fetch_pr_health_result =
        Some(Ok(healthy_pr_health(&workspace.branch_name, &head_sha)));
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));

    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo: Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github,
            pr_poller_registry: Some(Arc::clone(&registry)),
            transition_service: None,
            chat_service: Some(Arc::new(MockChatService::new())),
            agent_run_repo,
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: None,
            durable_recovery_state: None,
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("retry-eligible recovery should rearm supervision");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
            pr_number: 257,
            head_sha,
        }
    );
    assert!(registry.is_agent_workspace_polling(&conversation_id));
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace exists");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn refreshed_fixing_workspace_restores_current_exact_pr_autofix_claim() {
    let (_temp_dir, project, mut workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-refreshed-fixing").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(head_sha);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("refreshed".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_summary = Some("PR fixer refreshed from base.".to_string());

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed stranded workspace");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut active_autofix = AgentRun::new(conversation_id.clone());
    active_autofix.action_kind = Some(AgentRunActionKind::PrAutofix);
    active_autofix.action_context_id = Some("257".to_string());
    active_autofix.action_target_id =
        Some("github_pr_autofix:257:head:refreshed-fixing".to_string());
    agent_run_repo
        .create(active_autofix)
        .await
        .expect("seed active exact autofix");
    let github = Arc::new(MockGithubService::new());

    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            Arc::clone(&github),
            agent_run_repo,
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("stranded exact PR autofix should recover");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("active_pr_autofix_replacement")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    let recovered = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace exists");
    assert_eq!(
        recovered.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(recovered.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(
        recovered.pr_supervision_summary.as_deref(),
        Some("PR fixer refreshed from base.")
    );
    assert!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("events should load")
            .is_empty(),
        "claim restoration must not fabricate completion or publish events"
    );
}

#[tokio::test]
async fn refreshed_fixing_workspace_without_exact_pr_autofix_run_stays_unclaimed() {
    let (_temp_dir, project, mut workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-refreshed-fixing-unbound").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(head_sha);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("refreshed".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed unbound stranded workspace");
    let github = Arc::new(MockGithubService::new());

    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("unbound stranded workspace should fail closed");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("workspace_push_not_failed")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    let unchanged = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace exists");
    assert_eq!(
        unchanged.publication_push_status,
        workspace.publication_push_status
    );
    assert_eq!(
        unchanged.pr_supervision_status,
        workspace.pr_supervision_status
    );
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should load")
        .is_empty());
}

#[tokio::test]
async fn exhausted_stale_pr_autofix_stays_blocked_without_pr_polling() {
    let (_temp_dir, project, mut workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-retry-exhausted").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(head_sha);
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    let fingerprint = "github_pr_autofix:257:head:retry-exhausted";

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    for _ in 0..2 {
        let mut failed_autofix = AgentRun::new(conversation_id.clone());
        failed_autofix.action_kind = Some(AgentRunActionKind::PrAutofix);
        failed_autofix.action_context_id = Some("257".to_string());
        failed_autofix.action_target_id = Some(fingerprint.to_string());
        let failed_autofix = agent_run_repo
            .create(failed_autofix)
            .await
            .expect("seed failed autofix");
        agent_run_repo
            .fail(&failed_autofix.id, "autofix failed")
            .await
            .expect("fail autofix");
    }

    let github = Arc::new(MockGithubService::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));
    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo: Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: Some(Arc::clone(&registry)),
            transition_service: None,
            chat_service: Some(Arc::new(MockChatService::new())),
            agent_run_repo,
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: None,
            durable_recovery_state: None,
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("exhausted recovery should settle blocked");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("stale_repair_manual")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    assert!(!registry.is_agent_workspace_polling(&conversation_id));
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace exists");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("retry budget is exhausted"));
}

#[tokio::test]
async fn recovers_blocked_pr_supervision_as_draft_when_remote_pr_is_draft() {
    let (_temp_dir, project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-draft").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    let mut sync_state = open_sync_state(&workspace.branch_name, &head_sha);
    sync_state.is_draft = true;
    github.will_return_sync_state(sync_state);

    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            project_repo,
            github,
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
    )
    .await
    .expect("recover draft supervision");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
            pr_number: 257,
            head_sha,
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_pr_status.as_deref(), Some("draft"));
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
}

#[tokio::test]
async fn marks_terminal_pr_status_during_blocked_pr_supervision_recovery() {
    let cases = [
        (
            "pr-supervision-terminal-merged",
            PrStatus::Merged {
                merge_commit_sha: Some("merge-sha".to_string()),
                merged_at: None,
            },
            "merged",
            "pr_merged",
        ),
        (
            "pr-supervision-terminal-closed",
            PrStatus::Closed,
            "closed",
            "pr_closed",
        ),
    ];

    for (name, remote_status, expected_status, expected_step) in cases {
        let (_temp_dir, project, workspace, head_sha) = setup_recovery_workspace(name).await;
        let conversation_id = workspace.conversation_id.clone();
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
        let github = Arc::new(MockGithubService::new());
        let mut sync_state = open_sync_state(&workspace.branch_name, &head_sha);
        sync_state.status = remote_status;
        github.will_return_sync_state(sync_state);

        let outcome = recover_agent_workspace_pr_supervision(
            recovery_deps(
                Arc::clone(&workspace_repo),
                project_repo,
                github,
                Arc::new(MemoryAgentRunRepository::new()),
            ),
            conversation_id.clone(),
            AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        )
        .await
        .expect("terminal PR status should update workspace");

        assert_eq!(
            outcome,
            AgentWorkspacePrSupervisionRecoveryOutcome::Terminal {
                pr_number: 257,
                pr_status: expected_status.to_string(),
            }
        );
        let updated = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should still exist");
        assert_eq!(
            updated.publication_pr_status.as_deref(),
            Some(expected_status)
        );
        assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
        assert!(updated.pr_supervision_status.is_none());
        let events = workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| event.step == expected_step));
    }
}

#[tokio::test]
async fn skips_recovery_when_workspace_path_validation_fails_before_github_sync() {
    let (_temp_dir, project, mut workspace, _head_sha) =
        setup_recovery_workspace("pr-supervision-branch-validation").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.branch_name = "ralphx/test/other-branch".to_string();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let github = Arc::new(MockGithubService::new());

    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            workspace_repo,
            Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
    )
    .await
    .expect("invalid path recovery should be skipped");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("workspace_path_invalid")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
}

#[tokio::test]
async fn skips_blocked_pr_supervision_recovery_when_worktree_is_dirty() {
    let (_temp_dir, project, workspace, _head_sha) =
        setup_recovery_workspace("pr-supervision-dirty").await;
    let conversation_id = workspace.conversation_id.clone();
    std::fs::write(
        Path::new(&workspace.worktree_path).join("dirty.txt"),
        "uncommitted\n",
    )
    .expect("write dirty file");
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());

    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo,
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: None,
            transition_service: None,
            chat_service: None,
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: None,
            durable_recovery_state: None,
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
    )
    .await
    .expect("skip dirty recovery");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("worktree_dirty")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
}

#[tokio::test]
async fn recovery_noops_when_workspace_is_missing_or_startup_has_no_candidates() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let github = Arc::new(MockGithubService::new());

    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::clone(&project_repo),
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        ChatConversationId::new(),
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
    )
    .await
    .expect("missing workspace should be a skip");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("workspace_missing")
    );

    recover_recent_agent_workspace_pr_supervision_on_startup(
        recovery_deps(
            workspace_repo,
            project_repo,
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        Arc::new(HashSet::new()),
    )
    .await;

    assert_eq!(github.state().check_pr_sync_state_calls, 0);
}

#[tokio::test]
async fn skips_recovery_when_workspace_or_project_state_blocks_it() {
    let (_temp_dir, mut project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-project-skips").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));

    let active_run_repo = Arc::new(MemoryAgentRunRepository::new());
    active_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed active run");
    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::new(MemoryProjectRepository::with_projects(
                vec![project.clone()],
            )),
            Arc::clone(&github),
            active_run_repo,
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("active run skip");
    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("active_agent_run")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 1);

    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::new(MemoryProjectRepository::new()),
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("missing project skip");
    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("project_missing")
    );

    project.archived_at = Some(chrono::Utc::now());
    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::new(MemoryProjectRepository::with_projects(
                vec![project.clone()],
            )),
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("archived project skip");
    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("project_archived")
    );

    project.archived_at = None;
    project.github_pr_enabled = false;
    let mut unlinked_workspace = workspace;
    unlinked_workspace.publication_pr_number = None;
    unlinked_workspace.publication_pr_url = None;
    unlinked_workspace.publication_pr_status = None;
    workspace_repo
        .create_or_update(unlinked_workspace)
        .await
        .expect("future-PR-disabled workspace should persist");
    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            workspace_repo,
            Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("disabled PR skip");
    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("missing_pr_number")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 1);
}

#[tokio::test]
async fn supervision_recovers_existing_pr_without_origin_when_future_pr_preference_is_disabled() {
    let (_temp_dir, mut project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-no-origin-disabled").await;
    project.github_pr_enabled = false;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(
        "ralphx/test/pr-supervision-no-origin-disabled",
        &head_sha,
    ));

    let outcome = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("persisted PR should remain recoverable without an origin remote");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
            pr_number: 257,
            head_sha,
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, Some(257));
    assert_eq!(github.state().check_pr_sync_state_calls, 2);
}

#[tokio::test]
async fn active_run_does_not_hide_terminal_pr_during_supervision_recovery() {
    let (_temp_dir, project, workspace, _head_sha) =
        setup_recovery_workspace("pr-supervision-active-terminal").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let github = Arc::new(MockGithubService::new());
    let mut sync_state = open_sync_state(&workspace.branch_name, "remote-head");
    sync_state.status = PrStatus::Merged {
        merged_at: None,
        merge_commit_sha: None,
    };
    github.will_return_sync_state(sync_state);

    let active_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let run = active_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed active run");
    let chat = Arc::new(MockChatService::new());
    let mut deps = recovery_deps(
        Arc::clone(&workspace_repo),
        Arc::new(MemoryProjectRepository::with_projects(
            vec![project.clone()],
        )),
        Arc::clone(&github),
        Arc::clone(&active_run_repo),
    );
    deps.chat_service =
        Some(Arc::clone(&chat) as Arc<dyn crate::application::chat_service::ChatService>);

    let outcome = recover_agent_workspace_pr_supervision(
        deps,
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("terminal PR should recover");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Terminal {
            pr_number: 257,
            pr_status: "merged".to_string(),
        }
    );
    assert_eq!(
        chat.get_stop_agent_calls().await,
        vec![(
            crate::domain::entities::ChatContextType::Project,
            conversation_id.as_str()
        )]
    );
    let updated_run = active_run_repo
        .get_by_id(&run.id)
        .await
        .expect("run lookup should succeed")
        .expect("run should exist");
    assert_eq!(updated_run.status, AgentRunStatus::Failed);
    assert_eq!(
        updated_run.error_message.as_deref(),
        Some("Agent stopped because the workspace pull request was merged")
    );
    let updated_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(
        updated_workspace.publication_pr_status.as_deref(),
        Some("merged")
    );
}

#[tokio::test]
async fn terminal_supervision_recovery_reports_missing_chat_runtime() {
    let (_temp_dir, project, workspace, _head_sha) =
        setup_recovery_workspace("pr-supervision-terminal-runtime-missing").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let github = Arc::new(MockGithubService::new());
    let mut sync_state = open_sync_state(&workspace.branch_name, "remote-head");
    sync_state.status = PrStatus::Merged {
        merged_at: None,
        merge_commit_sha: None,
    };
    github.will_return_sync_state(sync_state);
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let run = agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed active run");

    let error = recover_agent_workspace_pr_supervision(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            github,
            Arc::clone(&agent_run_repo),
        ),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect_err("missing chat runtime must block terminal recovery success");

    assert!(error.to_string().contains("no chat runtime was available"));
    assert!(agent_run_repo
        .get_by_id(&run.id)
        .await
        .expect("run lookup")
        .expect("run retained")
        .is_active());
    let persisted = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace retained");
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("merged"));
    assert!(workspace_repo
        .get_local_cleanup_status(&conversation_id)
        .await
        .expect("cleanup status lookup")
        .is_none());
}

#[tokio::test]
async fn active_run_does_not_hide_terminal_linked_plan_pr_during_supervision_recovery() {
    let (_temp_dir, project, workspace, plan_branch, _head_sha) =
        setup_linked_plan_recovery_workspace("pr-supervision-active-plan-terminal", 704).await;
    let conversation_id = workspace.conversation_id.clone();
    let plan_worktree_path = workspace.worktree_path.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let plan_branch_id = plan_branch.id.clone();
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("seed plan branch");
    let github = Arc::new(MockGithubService::new());
    let mut sync_state = open_sync_state(&workspace.branch_name, "remote-head");
    sync_state.status = PrStatus::Merged {
        merged_at: None,
        merge_commit_sha: None,
    };
    github.will_return_sync_state(sync_state);

    let active_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let run = active_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed active run");
    let chat = Arc::new(MockChatService::new());
    let deps = AgentWorkspacePrSupervisionRecoveryDeps {
        workspace_repo: Arc::clone(&workspace_repo)
            as Arc<dyn AgentConversationWorkspaceRepository>,
        project_repo: Arc::new(MemoryProjectRepository::with_projects(
            vec![project.clone()],
        )),
        plan_branch_repo: Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        pr_poller_registry: None,
        transition_service: None,
        chat_service: Some(
            Arc::clone(&chat) as Arc<dyn crate::application::chat_service::ChatService>
        ),
        agent_run_repo: Arc::clone(&active_run_repo) as Arc<dyn AgentRunRepository>,
        events: Arc::new(ralphx_events::NullEventSink),
        pr_fix_review_publish_resumer: None,
        durable_recovery_state: None,
    };

    let outcome = recover_agent_workspace_pr_supervision(
        deps,
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
    )
    .await
    .expect("terminal linked plan PR should recover");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Terminal {
            pr_number: 704,
            pr_status: "merged".to_string(),
        }
    );
    assert_eq!(
        chat.get_stop_agent_calls().await,
        vec![(
            crate::domain::entities::ChatContextType::Project,
            conversation_id.as_str()
        )]
    );
    let updated_run = active_run_repo
        .get_by_id(&run.id)
        .await
        .expect("run lookup should succeed")
        .expect("run should exist");
    assert_eq!(updated_run.status, AgentRunStatus::Failed);
    assert_eq!(
        updated_run.error_message.as_deref(),
        Some("Agent stopped because the workspace pull request was merged")
    );
    let updated_plan = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    assert_eq!(updated_plan.pr_status, Some(PlanPrStatus::Merged));
    let updated_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace should exist");
    assert_eq!(
        updated_workspace.publication_pr_status.as_deref(),
        Some("merged"),
        "workspace authority must match the terminal linked plan PR"
    );
    assert_eq!(
        workspace_repo
            .get_local_cleanup_status(&conversation_id)
            .await
            .expect("cleanup marker lookup")
            .as_deref(),
        Some("cleaned")
    );
    assert!(
        !Path::new(&plan_worktree_path).exists(),
        "terminal linked plan worktree should be removed in-process"
    );
}

#[tokio::test]
async fn skips_recovery_when_remote_pr_sync_state_no_longer_matches_workspace() {
    let cases = [
        (
            "pr-supervision-branch-mismatch",
            open_sync_state("ralphx/test/different-branch", "unused"),
            "pr_head_branch_mismatch",
        ),
        (
            "pr-supervision-missing-head",
            {
                let mut sync = open_sync_state("ralphx/test/pr-supervision-missing-head", "unused");
                sync.head_ref_oid = None;
                sync
            },
            "pr_head_sha_missing",
        ),
        (
            "pr-supervision-sha-mismatch",
            open_sync_state("ralphx/test/pr-supervision-sha-mismatch", "remote-sha"),
            "pr_head_sha_mismatch",
        ),
    ];

    for (name, mut sync_state, expected_reason) in cases {
        let (_temp_dir, project, workspace, head_sha) = setup_recovery_workspace(name).await;
        if sync_state.head_ref_name == format!("ralphx/test/{name}") {
            sync_state.head_ref_oid = sync_state.head_ref_oid.map(|_| {
                if expected_reason == "pr_head_sha_mismatch" {
                    "remote-sha".to_string()
                } else {
                    head_sha.clone()
                }
            });
        }
        let conversation_id = workspace.conversation_id.clone();
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        let github = Arc::new(MockGithubService::new());
        github.will_return_sync_state(sync_state);

        let outcome = recover_agent_workspace_pr_supervision(
            recovery_deps(
                workspace_repo,
                Arc::new(MemoryProjectRepository::with_projects(vec![project])),
                github,
                Arc::new(MemoryAgentRunRepository::new()),
            ),
            conversation_id,
            AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        )
        .await
        .expect("sync mismatch skip");

        assert_eq!(
            outcome,
            AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(expected_reason)
        );
    }
}

#[tokio::test]
async fn startup_recovery_processes_candidates_and_skips_blocked_projects() {
    let (_temp_dir, project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-startup").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(
        vec![project.clone()],
    ));
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
    github.state().fetch_pr_health_result =
        Some(Ok(healthy_pr_health(&workspace.branch_name, &head_sha)));

    recover_recent_agent_workspace_pr_supervision_on_startup(
        recovery_deps(
            Arc::clone(&workspace_repo),
            Arc::clone(&project_repo),
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        Arc::new(HashSet::new()),
    )
    .await;

    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    assert_eq!(github.state().check_pr_sync_state_calls, 1);

    let (_blocked_temp, blocked_project, blocked_workspace, _blocked_head) =
        setup_recovery_workspace("pr-supervision-startup-blocked").await;
    let blocked_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    blocked_workspace_repo
        .create_or_update(blocked_workspace)
        .await
        .expect("seed blocked workspace");
    let blocked_github = Arc::new(MockGithubService::new());
    let blocked_ids = Arc::new(HashSet::from([blocked_project.id.clone()]));

    recover_recent_agent_workspace_pr_supervision_on_startup(
        recovery_deps(
            blocked_workspace_repo,
            Arc::new(MemoryProjectRepository::with_projects(vec![
                blocked_project,
            ])),
            Arc::clone(&blocked_github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        blocked_ids,
    )
    .await;

    assert_eq!(blocked_github.state().check_pr_sync_state_calls, 0);
}

#[tokio::test]
async fn startup_recovery_resumes_passed_pr_fix_workspace_review_handoff() {
    let (_temp_dir, project, mut workspace, base_sha) =
        setup_recovery_workspace("pr-supervision-startup-review-handoff").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(base_sha);
    std::fs::write(
        Path::new(&workspace.worktree_path).join("fix.txt"),
        "reviewed fix\n",
    )
    .expect("write workspace review target");
    let review_target = resolve_review_target(&workspace, &project)
        .await
        .expect("resolve review target")
        .expect("workspace review target should exist");
    workspace.publication_pr_number = Some(681);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/681".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("refreshed".to_string());
    workspace.auto_publish_enabled = true;
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("reviewing".to_string());
    workspace.pr_supervision_summary = Some(
        "PR fix verified; Workspace Review must finish before publishing resumes.".to_string(),
    );

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review handoff");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-current"));
    monitor.review_artifact_version = Some(1);
    monitor.review_requested_changes_artifact_id = Some(ArtifactId::from_string(
        "requested-changes-artifact-current",
    ));
    monitor.review_requested_changes_artifact_version = Some(1);
    monitor.reviewed_target_scope = Some(review_target.scope);
    monitor.reviewed_head_sha = review_target.head_sha.clone();
    monitor.reviewed_diff_fingerprint = Some(review_target.diff_fingerprint.clone());
    monitor.current_target_scope = Some(review_target.scope);
    monitor.current_diff_fingerprint = Some(review_target.diff_fingerprint);
    monitor.workspace_head_sha = review_target.head_sha;
    workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed passed review monitor");

    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut terminal_repair_run = AgentRun::new(conversation_id.clone());
    terminal_repair_run.complete();
    agent_run_repo
        .create(terminal_repair_run)
        .await
        .expect("terminal repair run should persist");
    let publish_resumer = Arc::new(RecordingReviewPublishResumer::new(Arc::clone(
        &workspace_repo,
    )));

    recover_recent_agent_workspace_pr_supervision_on_startup(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo,
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: None,
            transition_service: None,
            chat_service: None,
            agent_run_repo,
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: Some(
                Arc::clone(&publish_resumer) as Arc<dyn AgentWorkspacePrFixReviewPublishResumer>
            ),
            durable_recovery_state: None,
        },
        Arc::new(HashSet::new()),
    )
    .await;

    assert_eq!(publish_resumer.calls(), 1);
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events");
    assert!(events.iter().any(|event| {
        event.step == "pr_autofix_workspace_review_passed"
            && event.status == "publishing"
            && event.classification.as_deref() == Some("workspace_review_passed")
    }));
    assert!(events
        .iter()
        .any(|event| event.step == "published" && event.status == "succeeded"));
}

#[tokio::test]
async fn startup_recovery_does_not_publish_pr_fix_from_stale_review_fingerprint() {
    let (_temp_dir, project, mut workspace, base_sha) =
        setup_recovery_workspace("pr-supervision-startup-stale-review-handoff").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(base_sha);
    let reviewed_path = Path::new(&workspace.worktree_path).join("fix.txt");
    std::fs::write(&reviewed_path, "reviewed fix\n").expect("write reviewed workspace change");
    let reviewed_target = resolve_review_target(&workspace, &project)
        .await
        .expect("resolve reviewed target")
        .expect("reviewed target should exist");
    std::fs::write(&reviewed_path, "changed after review\n").expect("write stale workspace change");

    workspace.publication_pr_number = Some(681);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/681".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("refreshed".to_string());
    workspace.auto_publish_enabled = true;
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("reviewing".to_string());
    workspace.pr_supervision_summary = Some(
        "PR fix verified; Workspace Review must finish before publishing resumes.".to_string(),
    );

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review handoff");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-stale"));
    monitor.reviewed_target_scope = Some(reviewed_target.scope);
    monitor.reviewed_head_sha = reviewed_target.head_sha.clone();
    monitor.reviewed_diff_fingerprint = Some(reviewed_target.diff_fingerprint.clone());
    monitor.current_target_scope = Some(reviewed_target.scope);
    monitor.current_diff_fingerprint = Some(reviewed_target.diff_fingerprint);
    monitor.workspace_head_sha = reviewed_target.head_sha;
    workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed stale passed review monitor");

    let github = Arc::new(MockGithubService::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut terminal_repair_run = AgentRun::new(conversation_id.clone());
    terminal_repair_run.complete();
    agent_run_repo
        .create(terminal_repair_run)
        .await
        .expect("terminal repair run should persist");
    let publish_resumer = Arc::new(RecordingReviewPublishResumer::new(Arc::clone(
        &workspace_repo,
    )));
    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo: Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: None,
            transition_service: None,
            chat_service: None,
            agent_run_repo,
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: Some(
                Arc::clone(&publish_resumer) as Arc<dyn AgentWorkspacePrFixReviewPublishResumer>
            ),
            durable_recovery_state: None,
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("recover supervision");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("stale_repair_manual")
    );
    assert_eq!(
        publish_resumer.calls(),
        0,
        "stale passed review fingerprint must not authorize PR fix publish"
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events");
    assert!(
        !events
            .iter()
            .any(|event| event.step == "pr_autofix_workspace_review_passed"),
        "stale review handoff must not authorize publication"
    );
    assert!(events
        .iter()
        .any(|event| event.step == "pr_autofix_workspace_review_aborted"));
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("blocked"));
}

#[tokio::test]
async fn startup_recovery_processes_linked_plan_pr_supervision_candidates() {
    let (_temp_dir, project, workspace, plan_branch, head_sha) =
        setup_linked_plan_recovery_workspace("pr-supervision-startup-linked", 704).await;
    let conversation_id = workspace.conversation_id.clone();
    let plan_branch_id = plan_branch.id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("seed plan branch");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
    github.state().fetch_pr_health_result =
        Some(Ok(healthy_pr_health(&workspace.branch_name, &head_sha)));

    recover_recent_agent_workspace_pr_supervision_on_startup(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo,
            plan_branch_repo: Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
            github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: None,
            transition_service: None,
            chat_service: None,
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: None,
            durable_recovery_state: None,
        },
        Arc::new(HashSet::new()),
    )
    .await;

    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_push_status, None);
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    let updated_plan = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch should exist");
    assert_eq!(updated_plan.pr_push_status, PlanPrPushStatus::Pushed);
    assert_eq!(github.state().check_pr_sync_state_calls, 1);
}

#[tokio::test]
async fn pending_review_handoff_without_monitor_aborts_fail_closed_without_publishing() {
    // Falsification coverage for the crash window between the PR-fix completion
    // CAS (which persists `refreshed`/`reviewing` plus the pending
    // `pr_autofix_workspace_review` event atomically) and the Workspace Review
    // start that would create the review monitor. Recovery must not publish and
    // must not invoke the review-publish resumer; it aborts the orphaned
    // handoff to `failed`/`blocked` so a fresh repair attempt stays possible.
    let (_temp_dir, project, mut workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-review-pending-no-monitor").await;
    let conversation_id = workspace.conversation_id.clone();
    workspace.base_commit = Some(head_sha.clone());
    workspace.publication_push_status = Some("refreshed".to_string());
    workspace.pr_supervision_status = Some("reviewing".to_string());
    workspace.pr_supervision_summary = Some(
        "PR fix verified; Workspace Review must finish before publishing resumes.".to_string(),
    );

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_workspace_review",
            "pending",
            "PR fix verified; Workspace Review handoff is pending.",
            Some("workspace_review_pending".to_string()),
        ))
        .await
        .expect("seed pending review handoff event");

    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    let resumer = Arc::new(RecordingReviewPublishResumer::new(Arc::clone(
        &workspace_repo,
    )));

    let mut deps = recovery_deps(
        Arc::clone(&workspace_repo),
        project_repo,
        Arc::clone(&github),
        agent_run_repo,
    );
    deps.pr_fix_review_publish_resumer =
        Some(Arc::clone(&resumer) as Arc<dyn AgentWorkspacePrFixReviewPublishResumer>);

    let outcome = recover_agent_workspace_pr_supervision(
        deps,
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("recovery should settle without error");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("stale_repair_manual")
    );
    assert_eq!(resumer.calls(), 0);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.step == "pr_autofix_workspace_review_aborted"));
    assert!(!events.iter().any(|event| {
        matches!(
            event.step.as_str(),
            "published" | "pr_autofix_workspace_review_passed" | "pr_supervision_recovered"
        )
    }));
}

#[tokio::test]
async fn schedule_wrapper_delegates_to_lazy_deps_variant() {
    let (_temp_dir, project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-schedule-wrapper").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
    github.state().fetch_pr_health_result =
        Some(Ok(healthy_pr_health(&workspace.branch_name, &head_sha)));

    schedule_agent_workspace_pr_supervision_recovery(
        recovery_deps(
            workspace_repo,
            project_repo,
            Arc::clone(&github),
            Arc::new(MemoryAgentRunRepository::new()),
        ),
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        true,
    );

    wait_for_sync_state_calls(&github, 1).await;
}
