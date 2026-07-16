use std::path::Path;
use std::sync::Arc;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::agent_workspace_review::resolve_review_target;
use crate::application::agent_workspace_review_auto_merge::{
    auto_merge_guard_blocks_enable, cancel_workspace_review_auto_merge_guard,
    handle_passing_workspace_review_auto_merge_guard, preview_manual_workspace_review_start,
    reconcile_workspace_review_auto_merge_guards, restore_guarded_auto_merge_after_publish,
    start_guarded_agent_workspace_review, WorkspaceReviewStartOrigin,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspaceReviewAutoMergeGuard,
    AgentWorkspaceReviewAutoMergeGuardStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, AgentWorkspaceSourcePullRequest, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::domain::services::github_service::{
    PrAutoMergeRequest, PrHealth, PrStatus, PrSyncState,
};
use crate::error::AppError;
use crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository;
use crate::tests::mock_github_service::MockGithubService;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn auto_merge_health(head_ref: &str, head_sha: &str) -> PrHealth {
    PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: None,
            mergeable: None,
            is_draft: false,
            head_ref_name: head_ref.to_string(),
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

fn no_auto_merge_health(head_ref: &str, head_sha: &str) -> PrHealth {
    PrHealth {
        auto_merge_request: None,
        ..auto_merge_health(head_ref, head_sha)
    }
}

fn init_repo(repo: &Path, branch: &str) -> String {
    std::fs::create_dir_all(repo).expect("repository directory should be created");
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("workspace file should be written");
    git(repo, &["add", "README.md"]);
    git(repo, &["commit", "-m", "base"]);
    if branch != "main" {
        git(repo, &["checkout", "-b", branch]);
        std::fs::write(repo.join("workspace.md"), "workspace review\n")
            .expect("workspace file should be written");
        git(repo, &["add", "workspace.md"]);
        git(repo, &["commit", "-m", "workspace"]);
    }
    git(repo, &["rev-parse", "HEAD"])
}

async fn awaiting_workspace_delta_restore_context(
    state: &AppState,
    conversation_id: ChatConversationId,
    project_id: ProjectId,
    worktree_path: &Path,
) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .expect("project lookup should succeed")
        .expect("project should exist");
    let target = resolve_review_target(&workspace, &project)
        .await
        .expect("workspace target should resolve")
        .expect("workspace delta should exist");

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.last_run_id = Some("review-run".to_string());
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: target.diff_fingerprint,
        head_sha: target.head_sha,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    workspace
}

async fn append_workspace_delta_review_deferred_event(
    state: &AppState,
    conversation_id: ChatConversationId,
) {
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    let guard = monitor.auto_merge_guard.expect("guard should exist");
    let run_id = monitor.last_run_id.expect("review run should exist");
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "workspace_review_auto_merge",
            "waiting",
            "Workspace Review passed; GitHub auto-merge will resume after these changes are published.",
            Some(format!(
                "workspace_review_auto_merge:restore_deferred:{}:{}:{run_id}",
                guard.pr_number, guard.diff_fingerprint
            )),
        ))
        .await
        .expect("deferred event should persist");
}

async fn append_successful_workspace_publish_event(
    state: &AppState,
    conversation_id: ChatConversationId,
) {
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "published",
            "succeeded",
            "Draft pull request is ready",
            Some("published:42".to_string()),
        ))
        .await
        .expect("publish event should persist");
}

async fn selected_source_workspace_context(
    root: &Path,
    conversation_name: &str,
) -> (
    AppState,
    Arc<MockGithubService>,
    AgentConversationWorkspace,
    String,
) {
    let repo = root.join("repository");
    std::fs::create_dir(&repo).expect("repository directory should be created");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["checkout", "-b", "feature/review"]);
    std::fs::write(repo.join("review.rs"), "pub fn review() {}\n")
        .expect("feature file should be written");
    git(&repo, &["add", "review.rs"]);
    git(&repo, &["commit", "-m", "feature"]);
    let feature_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "main"]);

    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));
    state.github_service = Some(github.clone());
    let mut project = Project::new(
        "Workspace Review".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string(conversation_name),
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::PullRequest,
        "feature/review".to_string(),
        Some("Review source".to_string()),
        None,
        "ralphx/test/workspace-review".to_string(),
        root.join("missing-worktree").to_string_lossy().to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 42,
        url: None,
        title: Some("Review source".to_string()),
        head_ref_name: "feature/review".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some(feature_head.clone()),
    });
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    (state, github, workspace, feature_head)
}

#[tokio::test]
async fn manual_preview_captures_the_target_bound_auto_merge_effect() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repository");
    std::fs::create_dir(&repo).expect("repository directory should be created");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["checkout", "-b", "feature/review"]);
    std::fs::write(repo.join("review.rs"), "pub fn review() {}\n")
        .expect("feature file should be written");
    git(&repo, &["add", "review.rs"]);
    git(&repo, &["commit", "-m", "feature"]);
    let feature_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "main"]);

    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));
    state.github_service = Some(github.clone());
    let mut project = Project::new(
        "Workspace Review".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let conversation_id = ChatConversationId::from_string("workspace-review-preview");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::PullRequest,
        "feature/review".to_string(),
        Some("Review source".to_string()),
        None,
        "ralphx/test/workspace-review".to_string(),
        temp.path()
            .join("missing-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.pr_auto_merge_method = "merge".to_string();
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 42,
        url: None,
        title: Some("Review source".to_string()),
        head_ref_name: "feature/review".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some(feature_head.clone()),
    });

    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("preview should resolve");

    let target = preview
        .target
        .expect("selected source target should resolve");
    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::SelectedSource
    );
    assert_eq!(target.head_sha.as_deref(), Some(feature_head.as_str()));
    let auto_merge = preview
        .auto_merge
        .expect("enabled GitHub auto-merge should be previewed");
    assert_eq!(auto_merge.pr_number, 42);
    assert_eq!(auto_merge.merge_method, "squash");
    assert!(!auto_merge.restore_after_publish);
    assert_eq!(preview.confirmation.pr_number, Some(42));
    assert!(preview.confirmation.will_disable_auto_merge);
    assert_eq!(preview.confirmation.merge_method.as_deref(), Some("squash"));
    assert!(!preview.confirmation.restore_after_publish);
    assert_eq!(github.state().fetch_pr_health_calls, 1);
}

#[tokio::test]
async fn manual_preview_fails_closed_for_a_pr_target_without_a_github_service() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (mut state, _github, workspace, _feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-preview-no-github").await;
    state.github_service = None;

    let error = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect_err("PR-backed preview should require the GitHub integration");

    assert!(error
        .to_string()
        .contains("GitHub integration is unavailable"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn automated_review_fails_closed_for_a_pr_target_without_a_github_service() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (mut state, _github, workspace, _feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-start-no-github").await;
    state.github_service = None;
    let state = Arc::new(state);

    let error = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Automated,
        None,
    )
    .await
    .expect_err("PR-backed automated start should require the GitHub integration");

    assert!(error
        .to_string()
        .contains("GitHub integration is unavailable"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn manual_review_without_a_receipt_never_mutates_github_or_starts_a_reviewer() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repository");
    std::fs::create_dir(&repo).expect("repository directory should be created");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["checkout", "-b", "feature/review"]);
    std::fs::write(repo.join("review.rs"), "pub fn review() {}\n")
        .expect("feature file should be written");
    git(&repo, &["add", "review.rs"]);
    git(&repo, &["commit", "-m", "feature"]);
    let feature_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "main"]);

    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));
    state.github_service = Some(github.clone());
    let mut project = Project::new(
        "Workspace Review".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-confirmation");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::PullRequest,
        "feature/review".to_string(),
        Some("Review source".to_string()),
        None,
        "ralphx/test/workspace-review".to_string(),
        temp.path()
            .join("missing-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 42,
        url: None,
        title: Some("Review source".to_string()),
        head_ref_name: "feature/review".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some(feature_head),
    });
    let state = Arc::new(state);

    let error = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Manual,
        None,
    )
    .await
    .expect_err("manual starts require a confirmation receipt");

    assert!(error.to_string().contains("requires a fresh confirmation"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn manual_review_with_stale_confirmation_never_claims_or_disables_auto_merge() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-stale-confirmation").await;
    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("preview should resolve");
    let mut stale_confirmation = preview.confirmation;
    stale_confirmation.merge_method = Some("merge".to_string());
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));
    let state = Arc::new(state);

    let error = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Manual,
        Some(&stale_confirmation),
    )
    .await
    .expect_err("stale confirmation should be rejected");

    assert!(error.to_string().contains("state changed"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn automated_review_rolls_back_guard_when_github_disable_fails() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, _feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-disable-failure").await;
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("previous-diff".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("existing monitor target should persist");
    github.state().disable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "GitHub refused to disable auto-merge".to_string(),
    )));
    let state = Arc::new(state);

    let error = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Automated,
        None,
    )
    .await
    .expect_err("disable failure should stop review start");

    assert!(error.to_string().contains("could not disable"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    assert_eq!(
        monitor.current_target_scope,
        Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
    );
    assert_eq!(
        monitor.current_diff_fingerprint.as_deref(),
        Some("previous-diff")
    );
}

#[tokio::test]
async fn manual_review_rejects_a_conflicting_existing_guard_before_github_mutation() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-conflicting-guard").await;
    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("preview should resolve");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 777,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        diff_fingerprint: "different-source".to_string(),
        head_sha: Some("other-head".to_string()),
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));
    let state = Arc::new(state);

    let error = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Manual,
        Some(&preview.confirmation),
    )
    .await
    .expect_err("conflicting guard should be rejected");

    assert!(error.to_string().contains("another workspace Review"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard
            .expect("existing guard should remain")
            .pr_number,
        777
    );
}

#[tokio::test]
async fn automated_review_settles_current_passing_selected_source_after_pausing_auto_merge() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-current-pass-start").await;
    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("preview should resolve");
    let target = preview.target.expect("target should resolve");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact"));
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.reviewed_head_sha = Some(feature_head.clone());
    monitor.selected_source_head_sha = Some(feature_head.clone());
    monitor.selected_source_pull_request_number = Some(42);
    monitor.last_run_id = Some("review-run".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));
    let state = Arc::new(state);

    let start = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Automated,
        None,
    )
    .await
    .expect("current passing review should settle guarded start");

    assert!(!start.started);
    assert_eq!(start.skipped_reason.as_deref(), Some("current"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard
            .expect("unconfirmed restore should remain retryable")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    );
}

#[tokio::test]
async fn passing_selected_source_review_clears_guard_without_restore_when_source_pr_is_terminal() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-terminal-source-pr").await;
    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("preview should resolve");
    let target = preview.target.expect("target should resolve");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        diff_fingerprint: target.diff_fingerprint.clone(),
        head_sha: Some(feature_head.clone()),
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact"));
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint);
    monitor.reviewed_head_sha = Some(feature_head.clone());
    monitor.selected_source_head_sha = Some(feature_head);
    monitor.selected_source_pull_request_number = Some(42);
    monitor.last_run_id = Some("review-run".to_string());
    monitor.auto_merge_guard = Some(guard);
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("monitor should persist");
    github.state().check_pr_status_result = Some(Ok(PrStatus::Closed));

    handle_passing_workspace_review_auto_merge_guard(&state, &workspace, &monitor)
        .await
        .expect("terminal source PR should clear the guard");

    assert_eq!(github.state().check_pr_status_calls, 1);
    assert_eq!(github.state().last_check_pr_status_number, Some(42));
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn automated_review_restores_a_new_guard_when_the_start_is_already_reviewing() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, feature_head) = selected_source_workspace_context(
        temp.path(),
        "workspace-review-already-reviewing-new-guard",
    )
    .await;
    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("preview should resolve");
    let target = preview.target.expect("target should resolve");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint);
    monitor.last_run_id = Some("existing-review-run".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));
    let state = Arc::new(state);

    let start = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Automated,
        None,
    )
    .await
    .expect("already-reviewing skip should settle the guard created by this attempt");

    assert!(!start.started);
    assert_eq!(start.skipped_reason.as_deref(), Some("already_reviewing"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard
            .expect("unconfirmed restore should remain retryable")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    );
}

#[tokio::test]
async fn automated_review_keeps_an_existing_guard_when_the_start_is_already_reviewing() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, feature_head) = selected_source_workspace_context(
        temp.path(),
        "workspace-review-already-reviewing-existing-guard",
    )
    .await;
    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("preview should resolve");
    let effect = preview
        .auto_merge
        .expect("auto-merge effect should resolve");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: effect.pr_number,
        merge_method: effect.merge_method,
        target_scope: effect.target.scope,
        diff_fingerprint: effect.target.diff_fingerprint.clone(),
        head_sha: effect.target.head_sha,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some(effect.target.diff_fingerprint);
    monitor.last_run_id = Some("existing-review-run".to_string());
    monitor.auto_merge_guard = Some(guard.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    github.state().fetch_pr_health_result =
        Some(Ok(no_auto_merge_health("feature/review", &feature_head)));
    let state = Arc::new(state);

    let start = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Automated,
        None,
    )
    .await
    .expect("already-reviewing skip should retain the pre-existing guard");

    assert!(!start.started);
    assert_eq!(start.skipped_reason.as_deref(), Some("already_reviewing"));
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard,
        Some(guard)
    );
}

#[tokio::test]
async fn automated_review_restores_guard_when_reviewer_launch_fails_after_pausing() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, _feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-launch-failure").await;
    let state = Arc::new(state);

    let error = start_guarded_agent_workspace_review(
        Arc::clone(&state),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Automated,
        None,
    )
    .await
    .expect_err("missing parent conversation should fail reviewer launch");

    assert!(error.to_string().contains("Conversation not found"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard
            .expect("failed restore should be retryable")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    );
}

#[tokio::test]
async fn passing_selected_source_review_restores_auto_merge_immediately() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repository");
    std::fs::create_dir(&repo).expect("repository directory should be created");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["checkout", "-b", "feature/review"]);
    std::fs::write(repo.join("review.rs"), "pub fn review() {}\n")
        .expect("feature file should be written");
    git(&repo, &["add", "review.rs"]);
    git(&repo, &["commit", "-m", "feature"]);
    let feature_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "main"]);

    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));
    state.github_service = Some(github.clone());
    let mut project = Project::new(
        "Workspace Review".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let conversation_id = ChatConversationId::from_string("workspace-review-selected-source-pass");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::PullRequest,
        "feature/review".to_string(),
        Some("Review source".to_string()),
        None,
        "ralphx/test/workspace-review".to_string(),
        temp.path()
            .join("missing-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(7);
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 42,
        url: None,
        title: Some("Review source".to_string()),
        head_ref_name: "feature/review".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some(feature_head.clone()),
    });
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("selected-source target should preview");
    let target = preview.target.expect("target should resolve");
    assert_eq!(
        preview
            .auto_merge
            .as_ref()
            .expect("the live selected-source PR should retain authority")
            .pr_number,
        42
    );
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("selected-source-review-artifact"));
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.reviewed_head_sha = Some(feature_head.clone());
    monitor.selected_source_head_sha = Some(feature_head.clone());
    monitor.selected_source_pull_request_number = Some(42);
    monitor.last_run_id = Some("review-run".to_string());
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        diff_fingerprint: target.diff_fingerprint,
        head_sha: Some(feature_head),
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("monitor should persist");

    let restored = handle_passing_workspace_review_auto_merge_guard(&state, &workspace, &monitor)
        .await
        .expect("passing selected-source review should restore auto-merge");

    assert!(restored.auto_merge_guard.is_none());
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(true)
    );
}

#[tokio::test]
async fn passing_workspace_delta_review_defers_restore_until_publish_proof() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-delta-pass");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let diff_fingerprint = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("workspace-delta target should preview")
        .target
        .expect("target should resolve")
        .diff_fingerprint;
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("workspace-delta-review-artifact"));
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some(diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(diff_fingerprint.clone());
    monitor.last_run_id = Some("review-run".to_string());
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: diff_fingerprint.clone(),
        head_sha: None,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("monitor should persist");

    let deferred = handle_passing_workspace_review_auto_merge_guard(&state, &workspace, &monitor)
        .await
        .expect("passing workspace-delta review should defer restoration");

    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert_eq!(
        deferred
            .auto_merge_guard
            .expect("guard should remain")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should load");
    let expected_classification =
        format!("workspace_review_auto_merge:restore_deferred:42:{diff_fingerprint}:review-run");
    assert!(events.iter().any(|event| {
        event.classification.as_deref() == Some(expected_classification.as_str())
    }));
}

#[tokio::test]
async fn passing_review_with_stale_monitor_does_not_advance_guard() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-stale-pass");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let workspace = awaiting_workspace_delta_restore_context(
        &state,
        conversation_id.clone(),
        project.id,
        &worktree_path,
    )
    .await;
    let mut stale_monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    stale_monitor.last_run_id = Some("stale-review-run".to_string());

    let unchanged =
        handle_passing_workspace_review_auto_merge_guard(&state, &workspace, &stale_monitor)
            .await
            .expect("stale pass should be ignored");

    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert_eq!(
        unchanged
            .auto_merge_guard
            .expect("guard should remain awaiting publication")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish
    );
}

#[tokio::test]
async fn passing_review_without_a_guard_is_a_noop() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-pass-no-guard");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(42);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let target = resolve_review_target(&workspace, &project)
        .await
        .expect("target should resolve")
        .expect("workspace delta should exist");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project.id);
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint);
    monitor.last_run_id = Some("review-run".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("monitor should persist");

    let loaded = handle_passing_workspace_review_auto_merge_guard(&state, &workspace, &monitor)
        .await
        .expect("missing guard should be a no-op");

    assert!(loaded.auto_merge_guard.is_none());
}

#[tokio::test]
async fn passing_review_cancels_guard_when_target_disappears() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-pass-missing-target");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        temp.path()
            .join("missing-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(42);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("missing-target-review-artifact"));
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("missing-delta".to_string());
    monitor.reviewed_diff_fingerprint = Some("missing-delta".to_string());
    monitor.last_run_id = Some("review-run".to_string());
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "missing-delta".to_string(),
        head_sha: None,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("monitor should persist");

    let loaded = handle_passing_workspace_review_auto_merge_guard(&state, &workspace, &monitor)
        .await
        .expect("missing target should cancel guard without restoring");

    assert!(loaded.auto_merge_guard.is_none());
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(false)
    );
}

#[tokio::test]
async fn deferred_restore_marker_failure_rolls_guard_back_to_paused() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    state.agent_conversation_workspace_repo = workspace_repo.clone();
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-marker-failure");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let target = resolve_review_target(&workspace, &project)
        .await
        .expect("workspace target should resolve")
        .expect("workspace delta should exist");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: target.diff_fingerprint.clone(),
        head_sha: target.head_sha,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("deferred-review-artifact"));
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint);
    monitor.last_run_id = Some("review-run".to_string());
    monitor.auto_merge_guard = Some(guard.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("monitor should persist");
    workspace_repo.fail_next_publication_event("publication store unavailable");

    let error = handle_passing_workspace_review_auto_merge_guard(&state, &workspace, &monitor)
        .await
        .expect_err("required publish marker failure should fail the transition");

    assert!(error.to_string().contains("publication store unavailable"));
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard,
        Some(guard)
    );
    assert!(state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should load")
        .is_empty());
}

#[tokio::test]
async fn reconciliation_disables_auto_merge_for_an_interrupted_pausing_guard() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(auto_merge_health("workspace", "head")));
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-pausing-reconcile");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let diff_fingerprint = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("workspace-delta target should preview")
        .target
        .expect("target should resolve")
        .diff_fingerprint;
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some(diff_fingerprint.clone());
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Pausing,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint,
        head_sha: None,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    github.state().fetch_pr_health_result = Some(Ok(auto_merge_health("workspace", "head")));

    assert_eq!(
        reconcile_workspace_review_auto_merge_guards(&state)
            .await
            .expect("reconciliation should pause GitHub"),
        1
    );

    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard
            .expect("guard should remain paused")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview
    );
}

#[tokio::test]
async fn reconciliation_does_not_restore_from_a_stale_passing_review() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let (state, github, workspace, feature_head) =
        selected_source_workspace_context(temp.path(), "workspace-review-stale-pass-reconcile")
            .await;
    let preview = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("preview should resolve");
    let effect = preview
        .auto_merge
        .expect("auto-merge effect should resolve");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: effect.pr_number,
        merge_method: effect.merge_method,
        target_scope: effect.target.scope,
        diff_fingerprint: effect.target.diff_fingerprint.clone(),
        head_sha: effect.target.head_sha,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("stale-review-artifact"));
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some(effect.target.diff_fingerprint);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.reviewed_diff_fingerprint = Some("stale-review-target".to_string());
    monitor.reviewed_head_sha = Some("stale-review-head".to_string());
    monitor.selected_source_head_sha = Some(feature_head.clone());
    monitor.selected_source_pull_request_number = Some(42);
    monitor.last_run_id = Some("stale-review-run".to_string());
    monitor.auto_merge_guard = Some(guard.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    github.state().fetch_pr_health_result =
        Some(Ok(auto_merge_health("feature/review", &feature_head)));

    assert_eq!(
        reconcile_workspace_review_auto_merge_guards(&state)
            .await
            .expect("reconciliation should keep the guard fail-closed"),
        1
    );

    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard,
        Some(guard)
    );
}

#[tokio::test]
async fn reconciliation_marks_an_interrupted_restore_unconfirmed_by_github_as_retryable() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(no_auto_merge_health("workspace", "head")));
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-interrupted-restore");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let diff_fingerprint = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("workspace-delta target should preview")
        .target
        .expect("target should resolve")
        .diff_fingerprint;
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some(diff_fingerprint.clone());
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint,
        head_sha: None,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert_eq!(
        reconcile_workspace_review_auto_merge_guards(&state)
            .await
            .expect("reconciliation should inspect interrupted restore"),
        1
    );

    let guard = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .expect("guard should remain retryable");
    assert_eq!(
        guard.status,
        AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    );
    assert_eq!(
        guard.last_error.as_deref(),
        Some("GitHub did not report auto-merge as enabled after an interrupted restoration")
    );
}

#[tokio::test]
async fn reconciliation_cancels_guard_when_supervision_is_no_longer_desired() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("workspace-review-supervision-off");
    let project_id = ProjectId::from_string("workspace-review-project".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        "/tmp/workspace-review-supervision-off".to_string(),
    );
    workspace.pr_auto_merge_desired = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id);
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert_eq!(
        reconcile_workspace_review_auto_merge_guards(&state)
            .await
            .expect("reconciliation should cancel guard"),
        1
    );
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn reconciliation_cancels_guard_when_guarded_publication_pr_is_terminal() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("workspace-review-terminal-pr");
    let project_id = ProjectId::from_string("workspace-review-project".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        "/tmp/workspace-review-terminal-pr".to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("merged".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id);
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert_eq!(
        reconcile_workspace_review_auto_merge_guards(&state)
            .await
            .expect("reconciliation should cancel terminal PR guard"),
        1
    );
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn reconciliation_accepts_refreshed_workspace_delta_publication_proof() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(auto_merge_health("workspace", "head")));
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-reconcile-restore");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let mut workspace = awaiting_workspace_delta_restore_context(
        &state,
        conversation_id.clone(),
        project.id,
        &worktree_path,
    )
    .await;
    workspace.publication_push_status = Some("refreshed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("refreshed publication status should persist");
    append_workspace_delta_review_deferred_event(&state, conversation_id.clone()).await;
    append_successful_workspace_publish_event(&state, conversation_id.clone()).await;

    assert_eq!(
        reconcile_workspace_review_auto_merge_guards(&state)
            .await
            .expect("reconciliation should restore proven guard"),
        1
    );
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn cancelling_supervision_guard_keeps_auto_merge_disabled() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("workspace-review-guard-cancel");
    let project_id = ProjectId::from_string("workspace-review-project".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        "/tmp/workspace-review-guard-cancel".to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id);
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: Some("head-sha".to_string()),
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert!(cancel_workspace_review_auto_merge_guard(&state, &workspace)
        .await
        .expect("guard cancellation should succeed"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor should load")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace should load")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(false)
    );
}

#[tokio::test]
async fn cancelling_a_restoring_guard_disables_remote_before_clearing_it() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(auto_merge_health(
        "ralphx/test/workspace-review",
        "head",
    )));
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-restoring-cancel");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.pr_auto_merge_desired = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.auto_merge_guard = Some(guard.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert!(cancel_workspace_review_auto_merge_guard(&state, &workspace)
        .await
        .expect("guard cancellation should succeed"));

    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor should load")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn cancelling_a_restoring_guard_retains_it_when_remote_disable_fails() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    {
        let mut github_state = github.state();
        github_state.fetch_pr_health_result = Some(Ok(auto_merge_health(
            "ralphx/test/workspace-review",
            "head",
        )));
        github_state.disable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
            "remote disable failed".to_string(),
        )));
    }
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-cancel-disable-fails");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.pr_auto_merge_desired = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.auto_merge_guard = Some(guard.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    let error = cancel_workspace_review_auto_merge_guard(&state, &workspace)
        .await
        .expect_err("failed remote disable must fail closed");

    assert!(error.to_string().contains("remote disable failed"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor should load")
            .expect("monitor should exist")
            .auto_merge_guard,
        Some(guard)
    );
}

#[tokio::test]
async fn workspace_delta_restore_ignores_a_push_that_predates_the_passing_review() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-stale-push");
    init_repo(temp.path(), "ralphx/test/workspace-review");
    let workspace = awaiting_workspace_delta_restore_context(
        &state,
        conversation_id.clone(),
        project.id,
        temp.path(),
    )
    .await;

    append_successful_workspace_publish_event(&state, conversation_id).await;

    restore_guarded_auto_merge_after_publish(&state, &workspace)
        .await
        .expect("stale publish should be ignored");

    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard
            .expect("guard should remain active")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish
    );
}

#[tokio::test]
async fn restore_after_publish_ignores_missing_or_non_publish_guards() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("workspace-review-restore-ignored");
    let project_id = ProjectId::from_string("workspace-review-project".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        "/tmp/workspace-review-restore-ignored".to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.publication_pr_number = Some(42);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    restore_guarded_auto_merge_after_publish(&state, &workspace)
        .await
        .expect("missing guard should be ignored");

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id);
    assert!(!auto_merge_guard_blocks_enable(Some(&monitor)));
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("monitor should persist");

    restore_guarded_auto_merge_after_publish(&state, &workspace)
        .await
        .expect("paused guard should wait for passing review");

    assert!(auto_merge_guard_blocks_enable(Some(&monitor)));
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard
            .expect("paused guard should remain")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview
    );
}

#[tokio::test]
async fn workspace_delta_restore_accepts_refreshed_workspace_delta_after_publish() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(auto_merge_health("workspace", "head")));
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-new-local-content");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let workspace = awaiting_workspace_delta_restore_context(
        &state,
        conversation_id.clone(),
        project.id,
        &worktree_path,
    )
    .await;
    let original_fingerprint = preview_manual_workspace_review_start(&state, &workspace)
        .await
        .expect("review target should resolve")
        .target
        .expect("workspace delta should exist")
        .diff_fingerprint;
    let mut monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    monitor.current_diff_fingerprint = Some(original_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(original_fingerprint.clone());
    monitor
        .auto_merge_guard
        .as_mut()
        .expect("guard should exist")
        .diff_fingerprint = original_fingerprint;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    append_workspace_delta_review_deferred_event(&state, conversation_id.clone()).await;
    append_successful_workspace_publish_event(&state, conversation_id).await;
    std::fs::write(
        worktree_path.join("refreshed.rs"),
        "pub fn refreshed() {}\n",
    )
    .expect("refreshed workspace content should be written");

    restore_guarded_auto_merge_after_publish(&state, &workspace)
        .await
        .expect("refreshed publication proof should restore");

    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn workspace_delta_restore_repauses_auto_merge_when_supervision_turns_off_during_enable() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    {
        let mut github_state = github.state();
        github_state.enable_pr_auto_merge_delay_ms = 50;
        github_state.fetch_pr_health_result = Some(Ok(auto_merge_health("workspace", "head")));
    }
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-restore-race");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let workspace = awaiting_workspace_delta_restore_context(
        &state,
        conversation_id.clone(),
        project.id,
        &worktree_path,
    )
    .await;
    append_workspace_delta_review_deferred_event(&state, conversation_id.clone()).await;
    append_successful_workspace_publish_event(&state, conversation_id).await;

    let state = Arc::new(state);
    let restore_state = Arc::clone(&state);
    let restore_workspace = workspace.clone();
    let restore = tokio::spawn(async move {
        restore_guarded_auto_merge_after_publish(restore_state.as_ref(), &restore_workspace).await
    });
    for _ in 0..1_000 {
        if github.state().enable_pr_auto_merge_calls == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);

    let mut disabled_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    disabled_workspace.pr_auto_merge_desired = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(disabled_workspace)
        .await
        .expect("supervision preference should persist");

    restore
        .await
        .expect("restore task should join")
        .expect("restore should settle safely");

    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(false)
    );
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn workspace_delta_restore_repauses_auto_merge_when_the_review_target_changes_during_enable()
{
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    {
        let mut github_state = github.state();
        github_state.enable_pr_auto_merge_delay_ms = 50;
        github_state.fetch_pr_health_result = Some(Ok(auto_merge_health("workspace", "head")));
    }
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-target-race");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let workspace = awaiting_workspace_delta_restore_context(
        &state,
        conversation_id.clone(),
        project.id,
        &worktree_path,
    )
    .await;
    append_workspace_delta_review_deferred_event(&state, conversation_id.clone()).await;
    append_successful_workspace_publish_event(&state, conversation_id).await;

    let state = Arc::new(state);
    let restore_state = Arc::clone(&state);
    let restore_workspace = workspace.clone();
    let restore = tokio::spawn(async move {
        restore_guarded_auto_merge_after_publish(restore_state.as_ref(), &restore_workspace).await
    });
    for _ in 0..1_000 {
        if github.state().enable_pr_auto_merge_calls == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);

    let mut changed_monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    changed_monitor.current_diff_fingerprint = Some("new-workspace-delta".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(changed_monitor)
        .await
        .expect("new review target should persist");

    restore
        .await
        .expect("restore task should join")
        .expect("restore should settle safely");

    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(false)
    );
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn reconciliation_cancels_a_paused_guard_when_the_review_target_is_unavailable() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-missing-target");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/test/workspace-review".to_string(),
        temp.path()
            .join("missing-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.pr_auto_merge_desired = true;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project.id);
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert_eq!(
        reconcile_workspace_review_auto_merge_guards(&state)
            .await
            .expect("reconciliation should succeed"),
        1
    );
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(false)
    );
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
}

#[tokio::test]
async fn post_publish_handoff_retries_a_failed_workspace_delta_restore() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().enable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "GitHub is temporarily unavailable".to_string(),
    )));
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-retry-restore");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let workspace = awaiting_workspace_delta_restore_context(
        &state,
        conversation_id.clone(),
        project.id,
        &worktree_path,
    )
    .await;
    append_workspace_delta_review_deferred_event(&state, conversation_id.clone()).await;
    append_successful_workspace_publish_event(&state, conversation_id).await;

    restore_guarded_auto_merge_after_publish(&state, &workspace)
        .await
        .expect("failed restore should be recorded");
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard
            .expect("guard should remain retryable")
            .status,
        AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    );

    github.state().fetch_pr_health_result = Some(Ok(auto_merge_health("workspace", "head")));
    restore_guarded_auto_merge_after_publish(&state, &workspace)
        .await
        .expect("post-publish handoff should retry restoration");

    assert_eq!(github.state().enable_pr_auto_merge_calls, 2);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .is_none());
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(true)
    );
}

#[tokio::test]
async fn post_publish_restore_records_retryable_failure_when_github_does_not_confirm() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(no_auto_merge_health("workspace", "head")));
    state.github_service = Some(github.clone());
    let project = Project::new(
        "Workspace Review".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("workspace-review-restore-unconfirmed");
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path should resolve");
    init_repo(&worktree_path, "ralphx/test/workspace-review");
    let workspace = awaiting_workspace_delta_restore_context(
        &state,
        conversation_id.clone(),
        project.id,
        &worktree_path,
    )
    .await;
    append_workspace_delta_review_deferred_event(&state, conversation_id.clone()).await;
    append_successful_workspace_publish_event(&state, conversation_id).await;

    restore_guarded_auto_merge_after_publish(&state, &workspace)
        .await
        .expect("unconfirmed restore should be recorded");

    let guard = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist")
        .auto_merge_guard
        .expect("guard should remain retryable");
    assert_eq!(
        guard.status,
        AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed
    );
    assert_eq!(
        guard.last_error.as_deref(),
        Some("GitHub did not report auto-merge as enabled after restoration")
    );
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
}
