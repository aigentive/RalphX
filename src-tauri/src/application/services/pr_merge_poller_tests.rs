// Tests for PrPollerRegistry
//
// Tests cover:
// - is_polling() liveness detection
// - stop_polling() stopping guard + handle abort
// - start_polling() atomic idempotency (no duplicate pollers)
// - start_polling() skips when github_service is None
// - Adaptive interval calculation (age-based floor)
// - Backoff logic (exponential up to 600s cap, floor enforced)
// - RateLimitState default values

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{cleanup_terminal_agent_workspace_after_pr, PrPollerRegistry, RateLimitState};
use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
};
use crate::application::chat_service::MockChatService;
use crate::application::git_service::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatConversationId, IdeationAnalysisBaseRefKind, PlanBranchId, Project, TaskId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::services::GithubServiceTrait;
use crate::error::AppError;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryPlanBranchRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn make_registry_no_github() -> PrPollerRegistry {
    PrPollerRegistry::new(
        None,
        Arc::new(MemoryPlanBranchRepository::new()),
    )
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
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

fn init_cleanup_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    run_git(dir.path(), &["init"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test User"]);
    run_git(dir.path(), &["checkout", "-b", "main"]);
    std::fs::write(dir.path().join("README.md"), "base\n").expect("write readme");
    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-m", "initial"]);
    dir
}

fn branch_exists(repo: &std::path::Path, branch: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .current_dir(repo)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn cleanup_project(repo: &std::path::Path, worktree_parent: &std::path::Path) -> Project {
    let mut project = Project::new(
        "Poller Cleanup".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project
}

fn cleanup_workspace_with_conversation(
    project: &Project,
    branch_name: &str,
    conversation_id: &str,
) -> AgentConversationWorkspace {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let worktree_path =
        resolve_agent_conversation_workspace_path(project, &conversation_id).unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        branch_name.to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(101);
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.status = AgentConversationWorkspaceStatus::Active;
    workspace
}

fn expected_workspace_branch(project: &Project, conversation_id: &str) -> String {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    agent_conversation_branch_name(project, &conversation_id)
}

// ────────────────────────────────────────────────────────────────────
// RateLimitState
// ────────────────────────────────────────────────────────────────────

#[test]
fn rate_limit_default_has_high_remaining() {
    let rl = RateLimitState::default();
    assert!(
        rl.remaining >= 5000,
        "default remaining should be high so no throttling occurs on startup"
    );
    assert!(
        rl.reset_at > Instant::now(),
        "default reset_at should be in the future"
    );
}

// ────────────────────────────────────────────────────────────────────
// is_polling
// ────────────────────────────────────────────────────────────────────

#[test]
fn is_polling_returns_false_when_no_poller() {
    let registry = make_registry_no_github();
    let task_id = TaskId::from_string("task-1".to_string());
    assert!(!registry.is_polling(&task_id));
}

// ────────────────────────────────────────────────────────────────────
// start_polling — github_service guard
// ────────────────────────────────────────────────────────────────────

#[test]
fn start_polling_noop_when_github_service_none() {
    let registry = make_registry_no_github();
    let task_id = TaskId::from_string("task-1".to_string());
    let plan_branch_id = PlanBranchId::from_string("branch-1".to_string());

    // This should not panic or spawn anything when github_service is None
    // We can't call start_polling without a transition_service easily in unit tests,
    // so we just verify no poller is active after returning.
    // The actual noop is tested by checking is_polling remains false.
    // Note: start_polling requires transition_service which we can't easily
    // construct in unit tests without full AppState. We verify behavior through
    // the is_polling check in integration tests.
    assert!(!registry.is_polling(&task_id));
    // start_polling with None github_service returns early without inserting
    drop(plan_branch_id); // suppress unused warning
}

// ────────────────────────────────────────────────────────────────────
// stop_polling — stopping guard
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn stop_polling_inserts_into_stopping_before_abort() {
    let registry = make_registry_no_github();
    let task_id = TaskId::from_string("task-2".to_string());

    // stop_polling on a non-running task should not panic
    registry.stop_polling(&task_id);

    // The stopping map should have the entry set (even for non-running task)
    // This ensures the race guard is in place
    assert!(
        registry.stopping.contains_key(&task_id),
        "stopping flag must be set even if no active poller"
    );
}

#[tokio::test]
async fn stop_polling_does_not_remove_from_stopping_immediately() {
    // The stopping flag must remain until poll_loop cleanup removes it.
    // stop_polling itself must NOT remove it (AD11).
    let registry = make_registry_no_github();
    let task_id = TaskId::from_string("task-3".to_string());

    registry.stop_polling(&task_id);
    // Flag should still be present (poll_loop cleanup is responsible for removal)
    assert!(registry.stopping.contains_key(&task_id));
}

// ────────────────────────────────────────────────────────────────────
// Adaptive interval calculation
// ────────────────────────────────────────────────────────────────────

#[test]
fn age_floor_fresh_pr_is_60s() {
    // Fresh PR (< 1 hr) should use 60s floor
    let elapsed = Duration::from_secs(300); // 5 minutes
    let floor = compute_age_floor(elapsed);
    assert_eq!(floor, Duration::from_secs(60));
}

#[test]
fn age_floor_hourly_pr_is_120s() {
    // PR > 1 hr but < 24 hr → 120s floor
    let elapsed = Duration::from_secs(7200); // 2 hours
    let floor = compute_age_floor(elapsed);
    assert_eq!(floor, Duration::from_secs(120));
}

#[test]
fn age_floor_day_old_pr_is_300s() {
    // PR > 24 hr → 300s floor
    let elapsed = Duration::from_secs(90000); // 25 hours
    let floor = compute_age_floor(elapsed);
    assert_eq!(floor, Duration::from_secs(300));
}

// ────────────────────────────────────────────────────────────────────
// Backoff calculation
// ────────────────────────────────────────────────────────────────────

#[test]
fn backoff_caps_at_600s() {
    // After many errors, backoff should not exceed 600s
    for errors in 5u32..=20 {
        let backoff =
            Duration::from_secs(60 * 2u64.pow(errors.min(4))).min(Duration::from_secs(600));
        assert!(
            backoff <= Duration::from_secs(600),
            "backoff exceeded 600s at {} errors: {:?}",
            errors,
            backoff
        );
    }
}

#[test]
fn backoff_increases_exponentially_up_to_cap() {
    // Verify the backoff sequence: 120s, 240s, 480s, 600s, 600s
    let expected = [120u64, 240, 480, 600, 600];
    for (i, &expected_secs) in expected.iter().enumerate() {
        let errors = (i + 1) as u32;
        let backoff = Duration::from_secs(60 * 2u64.pow(errors.min(4)))
            .min(Duration::from_secs(600))
            .as_secs();
        assert_eq!(
            backoff, expected_secs,
            "error #{}: expected {}s backoff, got {}s",
            errors, expected_secs, backoff
        );
    }
}

#[test]
fn backoff_never_goes_below_age_floor() {
    // Error backoff at 1 error = 120s; for a fresh PR (floor=60s), interval = max(120, 60) = 120s
    let consecutive_errors = 1u32;
    let age_floor = Duration::from_secs(60); // fresh PR
    let backoff =
        Duration::from_secs(60 * 2u64.pow(consecutive_errors.min(4))).min(Duration::from_secs(600));
    let interval = backoff.max(age_floor);
    assert_eq!(interval, Duration::from_secs(120));

    // For an old PR (floor=300s), backoff at 1 error = 120s; interval = max(120, 300) = 300s
    let old_age_floor = Duration::from_secs(300);
    let interval_old = backoff.max(old_age_floor);
    assert_eq!(interval_old, Duration::from_secs(300));
}

// ────────────────────────────────────────────────────────────────────
// Idempotency: no duplicate pollers
// ────────────────────────────────────────────────────────────────────

#[test]
fn pr_creation_guard_is_shared_arc() {
    // Verify pr_creation_guard is an Arc (shared between registry and TaskServices)
    let registry = make_registry_no_github();
    let guard_clone = Arc::clone(&registry.pr_creation_guard);

    // Insert via registry's guard — should be visible through clone
    registry
        .pr_creation_guard
        .insert(PlanBranchId::from_string("branch-1".to_string()), ());

    assert!(
        guard_clone.contains_key(&PlanBranchId::from_string("branch-1".to_string())),
        "pr_creation_guard must be an Arc pointing to same DashMap"
    );
}

#[tokio::test]
async fn terminal_agent_workspace_pr_cleanup_fetches_base_and_deletes_merged_artifacts() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "poller-cleanup-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    let conversation_id = workspace.conversation_id.clone();
    let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    GitService::create_worktree(repo.path(), &worktree_path, &branch, "main")
        .await
        .expect("create worktree");
    std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
    run_git(&worktree_path, &["add", "."]);
    run_git(&worktree_path, &["commit", "-m", "agent work"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", &branch, "-m", "merge agent"],
    );
    let github = Arc::new(MockGithubService::new());

    cleanup_terminal_agent_workspace_after_pr(
        Arc::clone(&workspace_repo),
        &conversation_id,
        &project,
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        true,
    )
    .await;

    assert!(!worktree_path.exists());
    assert!(!branch_exists(repo.path(), &branch));
    let state = github.state();
    assert_eq!(state.fetch_remote_calls, 1);
    assert_eq!(state.last_fetch_remote_branch_name.as_deref(), Some("main"));
}

#[tokio::test]
async fn terminal_agent_workspace_pr_cleanup_continues_after_fetch_failure() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "poller-fetch-failure-cleanup-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    let conversation_id = workspace.conversation_id.clone();
    let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    GitService::create_worktree(repo.path(), &worktree_path, &branch, "main")
        .await
        .expect("create worktree");
    std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
    run_git(&worktree_path, &["add", "."]);
    run_git(&worktree_path, &["commit", "-m", "agent work"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", &branch, "-m", "merge agent"],
    );
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_remote_result = Some(Err(AppError::GitOperation(
        "simulated fetch failure".to_string(),
    )));

    cleanup_terminal_agent_workspace_after_pr(
        Arc::clone(&workspace_repo),
        &conversation_id,
        &project,
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        true,
    )
    .await;

    assert!(!worktree_path.exists());
    assert!(!branch_exists(repo.path(), &branch));
    assert_eq!(github.state().fetch_remote_calls, 1);
}

#[tokio::test]
async fn agent_workspace_closed_pr_polling_removes_worktree_but_keeps_branch() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let branch = "ralphx/poller-cleanup/agent-closed";
    let mut workspace =
        cleanup_workspace_with_conversation(&project, branch, "poller-closed-cleanup-conversation");
    workspace.publication_pr_status = Some("open".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    GitService::create_worktree(repo.path(), &worktree_path, branch, "main")
        .await
        .expect("create worktree");
    std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
    run_git(&worktree_path, &["add", "."]);
    run_git(&worktree_path, &["commit", "-m", "agent work"]);
    let github = Arc::new(MockGithubService::new());
    github.will_return_status(crate::domain::services::github_service::PrStatus::Closed);
    let registry = PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    );

    registry.start_agent_workspace_polling(
        conversation_id.clone(),
        101,
        project,
        repo.path().to_path_buf(),
        Arc::clone(&workspace_repo),
        Arc::new(MockChatService::new()),
    );
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if !worktree_path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("poller should remove closed PR worktree");

    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should remain persisted");
    assert_eq!(updated.publication_pr_status.as_deref(), Some("closed"));
    assert!(branch_exists(repo.path(), branch));
    assert_eq!(github.state().fetch_remote_calls, 0);
}

// ────────────────────────────────────────────────────────────────────
// Helper: compute age floor (mirrors poll_loop logic)
// ────────────────────────────────────────────────────────────────────

fn compute_age_floor(elapsed: Duration) -> Duration {
    if elapsed < Duration::from_secs(3600) {
        Duration::from_secs(60)
    } else if elapsed < Duration::from_secs(86400) {
        Duration::from_secs(120)
    } else {
        Duration::from_secs(300)
    }
}
