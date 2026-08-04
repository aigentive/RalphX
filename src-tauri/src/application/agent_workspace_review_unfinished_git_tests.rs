use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::application::agent_workspace_review::{
    resolve_review_target, WORKSPACE_REVIEW_UNFINISHED_GIT_OPERATION_ERROR,
};
use crate::application::agent_workspace_review_auto_merge::{
    start_guarded_agent_workspace_review, WorkspaceReviewStartOrigin,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceReviewTargetScope,
    AgentWorkspaceSourcePullRequest, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::services::github_service::{
    PrAutoMergeRequest, PrHealth, PrStatus, PrSyncState,
};
use crate::error::AppError;
use crate::tests::mock_github_service::MockGithubService;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_fails(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        !output.status.success(),
        "git {args:?} unexpectedly succeeded"
    );
}

struct LinkedWorktreeFixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    worktree: PathBuf,
    base_sha: String,
    project: Project,
    workspace: AgentConversationWorkspace,
}

impl LinkedWorktreeFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let repo = root.path().join("repo");
        let worktree = root.path().join("worktree");
        std::fs::create_dir_all(&repo).expect("repo directory");
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("shared.txt"), "base\n").expect("base file");
        git(&repo, &["add", "shared.txt"]);
        git(&repo, &["commit", "-m", "base"]);
        let base_sha = git(&repo, &["rev-parse", "HEAD"]);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "ralphx/test/unfinished-review",
                worktree.to_str().expect("worktree path"),
                "main",
            ],
        );
        let mut project = Project::new(
            "Workspace Review".to_string(),
            repo.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        let workspace = AgentConversationWorkspace::new(
            ChatConversationId::new(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha.clone()),
            "ralphx/test/unfinished-review".to_string(),
            worktree.to_string_lossy().to_string(),
        );
        Self {
            _root: root,
            repo,
            worktree,
            base_sha,
            project,
            workspace,
        }
    }

    fn create_conflicted_merge(&self) {
        std::fs::write(self.worktree.join("shared.txt"), "feature\n").expect("feature file");
        git(&self.worktree, &["add", "shared.txt"]);
        git(&self.worktree, &["commit", "-m", "feature"]);
        std::fs::write(self.repo.join("shared.txt"), "main\n").expect("main file");
        git(&self.repo, &["add", "shared.txt"]);
        git(&self.repo, &["commit", "-m", "main"]);
        git_fails(&self.worktree, &["merge", "main"]);
    }

    fn create_conflicted_rebase(&self) {
        std::fs::write(self.worktree.join("shared.txt"), "feature\n").expect("feature file");
        git(&self.worktree, &["add", "shared.txt"]);
        git(&self.worktree, &["commit", "-m", "feature"]);
        std::fs::write(self.repo.join("shared.txt"), "main\n").expect("main file");
        git(&self.repo, &["add", "shared.txt"]);
        git(&self.repo, &["commit", "-m", "main"]);
        git_fails(&self.worktree, &["rebase", "main"]);
    }
}

fn assert_unfinished_operation(error: AppError) {
    assert!(matches!(
        error,
        AppError::WorkspaceReviewUnfinishedGitOperation
    ));
    assert_eq!(
        error.to_string(),
        WORKSPACE_REVIEW_UNFINISHED_GIT_OPERATION_ERROR
    );
}

fn auto_merge_health(head_sha: &str) -> PrHealth {
    PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: None,
            mergeable: None,
            is_draft: false,
            head_ref_name: "ralphx/test/unfinished-review".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some(head_sha.to_string()),
            base_ref_oid: None,
        },
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: Some(PrAutoMergeRequest {
            enabled_by: Some("reviewer".to_string()),
            merge_method: Some("squash".to_string()),
        }),
    }
}

#[tokio::test]
async fn workspace_review_unfinished_git_blocks_unmerged_linked_worktree_and_settled_retry() {
    let fixture = LinkedWorktreeFixture::new();
    fixture.create_conflicted_merge();

    let error = resolve_review_target(&fixture.workspace, &fixture.project)
        .await
        .expect_err("unmerged workspace delta must be rejected");
    assert_unfinished_operation(error);

    git(&fixture.worktree, &["merge", "--abort"]);
    std::fs::write(fixture.worktree.join("retry.txt"), "settled\n").expect("retry file");
    let target = resolve_review_target(&fixture.workspace, &fixture.project)
        .await
        .expect("settled retry should resolve")
        .expect("workspace delta should exist");
    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::WorkspaceDelta
    );
}

#[tokio::test]
async fn workspace_review_unfinished_git_blocks_resolved_but_unfinished_merge() {
    let fixture = LinkedWorktreeFixture::new();
    fixture.create_conflicted_merge();
    std::fs::write(fixture.worktree.join("shared.txt"), "resolved\n").expect("resolved file");
    git(&fixture.worktree, &["add", "shared.txt"]);

    let error = resolve_review_target(&fixture.workspace, &fixture.project)
        .await
        .expect_err("staged merge must remain blocked until completed or aborted");

    assert_unfinished_operation(error);
}

#[tokio::test]
async fn workspace_review_unfinished_git_blocks_resolved_but_unfinished_rebase() {
    let fixture = LinkedWorktreeFixture::new();
    fixture.create_conflicted_rebase();
    std::fs::write(fixture.worktree.join("shared.txt"), "resolved\n").expect("resolved file");
    git(&fixture.worktree, &["add", "shared.txt"]);

    let error = resolve_review_target(&fixture.workspace, &fixture.project)
        .await
        .expect_err("staged rebase must remain blocked until completed or aborted");

    assert_unfinished_operation(error);
}

#[tokio::test]
async fn workspace_review_unfinished_git_does_not_block_selected_source_only_target() {
    let fixture = LinkedWorktreeFixture::new();
    git(&fixture.repo, &["checkout", "-b", "selected"]);
    std::fs::write(fixture.repo.join("selected.txt"), "selected\n").expect("selected file");
    git(&fixture.repo, &["add", "selected.txt"]);
    git(&fixture.repo, &["commit", "-m", "selected"]);
    let selected_head = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let git_dir = fixture.repo.join(".git");
    std::fs::create_dir(git_dir.join("rebase-merge")).expect("unrelated rebase metadata");
    let mut workspace = fixture.workspace.clone();
    workspace.worktree_path = fixture
        ._root
        .path()
        .join("missing-worktree")
        .to_string_lossy()
        .to_string();
    workspace.base_ref_kind = IdeationAnalysisBaseRefKind::PullRequest;
    workspace.base_ref = "selected".to_string();
    workspace.base_commit = None;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 42,
        url: None,
        title: Some("Selected source".to_string()),
        head_ref_name: "selected".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some(selected_head),
    });

    let target = resolve_review_target(&workspace, &fixture.project)
        .await
        .expect("selected source should ignore unrelated workspace metadata")
        .expect("selected source target");

    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::SelectedSource
    );
    assert_eq!(target.base_sha.as_deref(), Some(fixture.base_sha.as_str()));
}

async fn guarded_start_race_context(
    restore_should_fail: bool,
) -> (LinkedWorktreeFixture, Arc<AppState>, Arc<MockGithubService>) {
    let mut fixture = LinkedWorktreeFixture::new();
    std::fs::write(fixture.worktree.join("review.txt"), "review\n").expect("review file");
    std::fs::write(fixture.worktree.join("shared.txt"), "feature\n").expect("feature file");
    git(&fixture.worktree, &["add", "review.txt", "shared.txt"]);
    git(&fixture.worktree, &["commit", "-m", "review delta"]);
    let head_sha = git(&fixture.worktree, &["rev-parse", "HEAD"]);
    std::fs::write(fixture.repo.join("shared.txt"), "main\n").expect("main file");
    git(&fixture.repo, &["add", "shared.txt"]);
    git(&fixture.repo, &["commit", "-m", "main conflict"]);
    fixture.workspace.pr_auto_merge_desired = true;
    fixture.workspace.publication_pr_number = Some(42);
    fixture.workspace.publication_pr_status = Some("open".to_string());

    let mut state = AppState::new_test();
    state
        .project_repo
        .create(fixture.project.clone())
        .await
        .expect("project should persist");
    state
        .agent_conversation_workspace_repo
        .create_or_update(fixture.workspace.clone())
        .await
        .expect("workspace should persist");
    let github = Arc::new(MockGithubService::new());
    {
        let mut github_state = github.state();
        github_state.fetch_pr_health_result = Some(Ok(auto_merge_health(&head_sha)));
        github_state.disable_pr_auto_merge_delay_ms = 250;
        github_state.disable_pr_auto_merge_followup_health_result =
            Some(Ok(auto_merge_health(&head_sha)));
        github_state.check_pr_status_result = Some(Ok(PrStatus::Open));
        if restore_should_fail {
            github_state.enable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
                "restore unavailable".to_string(),
            )));
        }
    }
    state.github_service = Some(github.clone());
    (fixture, Arc::new(state), github)
}

async fn start_guarded_review_after_beginning_merge(
    fixture: &LinkedWorktreeFixture,
    state: &Arc<AppState>,
    github: &Arc<MockGithubService>,
) -> AppError {
    let workspace = fixture.workspace.clone();
    let start_state = Arc::clone(state);
    let start = tokio::spawn(async move {
        start_guarded_agent_workspace_review(
            start_state,
            &workspace,
            false,
            WorkspaceReviewStartOrigin::Automated,
            None,
        )
        .await
    });
    let pause_started = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if github.state().disable_pr_auto_merge_calls == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        pause_started.is_ok(),
        "workspace review did not pause auto-merge before the bounded synchronization deadline"
    );
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    git_fails(&fixture.worktree, &["merge", "main"]);
    start
        .await
        .expect("guarded start task should join")
        .expect_err("second target read should reject the unfinished operation")
}

#[tokio::test]
async fn workspace_review_unfinished_git_rolls_back_attempt_guard_without_reresolving_target() {
    let (fixture, state, github) = guarded_start_race_context(false).await;

    let error = start_guarded_review_after_beginning_merge(&fixture, &state, &github).await;

    assert_unfinished_operation(error);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().fetch_pr_health_calls, 1);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&fixture.workspace.conversation_id)
        .await
        .expect("monitor lookup")
        .expect("monitor should exist");
    assert!(monitor.auto_merge_guard.is_none());
    assert!(monitor.last_run_id.is_none());
    assert!(monitor.review_conversation_id.is_none());
    assert!(state
        .agent_run_repo
        .get_active_for_conversation(&fixture.workspace.conversation_id)
        .await
        .expect("agent run lookup")
        .is_none());
}

#[tokio::test]
async fn workspace_review_unfinished_git_records_restore_failed_without_launching_reviewer() {
    let (fixture, state, github) = guarded_start_race_context(true).await;

    let error = start_guarded_review_after_beginning_merge(&fixture, &state, &github).await;

    assert_unfinished_operation(error);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&fixture.workspace.conversation_id)
        .await
        .expect("monitor lookup")
        .expect("monitor should exist");
    let guard = monitor
        .auto_merge_guard
        .expect("failed restore remains durable");
    assert_eq!(
        guard.status,
        crate::domain::entities::AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    );
    assert!(guard
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("restore unavailable")));
    assert!(monitor.last_run_id.is_none());
    assert!(monitor.review_conversation_id.is_none());
}
