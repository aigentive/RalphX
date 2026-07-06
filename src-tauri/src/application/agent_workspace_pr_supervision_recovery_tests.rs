use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::application::agent_conversation_workspace::{
    resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
};
use crate::application::agent_workspace_pr_supervision_recovery::{
    pr_supervision_recovery_schedule_skip_reason, recover_agent_workspace_pr_supervision,
    recover_recent_agent_workspace_pr_supervision_on_startup,
    schedule_agent_workspace_pr_supervision_recovery, AgentWorkspacePrSupervisionRecoveryDeps,
    AgentWorkspacePrSupervisionRecoveryOutcome, AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::chat_service::MockChatService;
use crate::application::git_service::GitService;
use crate::application::services::PrPollerRegistry;
use crate::domain::entities::plan_branch::{
    PrPushStatus as PlanPrPushStatus, PrStatus as PlanPrStatus,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRun, ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId,
    PlanBranch, PlanBranchId, PlanBranchStatus, Project, ProjectId, TaskId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, PlanBranchRepository,
};
use crate::domain::services::github_service::{
    PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
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
        app_handle: None,
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
    let branch_name = format!("ralphx/test/{name}");
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
    let deps = recovery_deps(
        workspace_repo,
        project_repo,
        Arc::clone(&github),
        Arc::new(MemoryAgentRunRepository::new()),
    );

    schedule_agent_workspace_pr_supervision_recovery(
        deps.clone(),
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        true,
    );
    schedule_agent_workspace_pr_supervision_recovery(
        deps,
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        false,
    );

    wait_for_sync_state_calls(&github, 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(github.state().check_pr_sync_state_calls, 1);
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
            app_handle: None,
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
            app_handle: None,
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
async fn marks_terminal_linked_plan_pr_status_without_workspace_publication_pr() {
    let cases = [
        (
            "plan-pr-supervision-terminal-merged",
            PrStatus::Merged {
                merge_commit_sha: Some("merge-sha".to_string()),
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
                app_handle: None,
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
        assert_eq!(updated.publication_pr_number, None);
        assert_eq!(updated.publication_push_status, None);
        assert!(updated
            .pr_supervision_summary
            .as_deref()
            .unwrap_or_default()
            .contains("Pull request"));
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
                app_handle: None,
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
async fn recovers_stale_needs_agent_repair_before_rearming_pr_supervision() {
    let (_temp_dir, project, mut workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-needs-agent").await;
    let conversation_id = workspace.conversation_id.clone();
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
    assert!(events
        .iter()
        .any(|event| event.step == "stale_repair_recovered"));
    assert!(events
        .iter()
        .any(|event| event.step == "pr_supervision_recovered"));
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
            app_handle: None,
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
async fn skips_recovery_before_git_when_workspace_or_project_state_blocks_it() {
    let (_temp_dir, mut project, workspace, _head_sha) =
        setup_recovery_workspace("pr-supervision-project-skips").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let github = Arc::new(MockGithubService::new());

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
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("github_pr_disabled")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
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
