use std::path::Path;
use std::sync::Arc;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::agent_workspace_review_auto_merge::{
    cancel_workspace_review_auto_merge_guard, restore_guarded_auto_merge_after_publish,
    start_guarded_agent_workspace_review, WorkspaceReviewStartOrigin,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspaceReviewAutoMergeGuard,
    AgentWorkspaceReviewAutoMergeGuardStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope, AgentWorkspaceSourcePullRequest,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::domain::services::github_service::{
    PrAutoMergeRequest, PrHealth, PrStatus, PrSyncState,
};
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

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("workspace-delta".to_string());
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_diff_fingerprint = Some("workspace-delta".to_string());
    monitor.last_run_id = Some("review-run".to_string());
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
    workspace
}

async fn append_workspace_delta_review_deferred_event(
    state: &AppState,
    conversation_id: ChatConversationId,
) {
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "workspace_review_auto_merge",
            "waiting",
            "Workspace Review passed; GitHub auto-merge will resume after these changes are published.",
            Some(
                "workspace_review_auto_merge:restore_deferred:42:workspace-delta:review-run"
                    .to_string(),
            ),
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
    std::fs::create_dir_all(&worktree_path).expect("workspace directory should be created");
    git(
        &worktree_path,
        &["init", "-b", "ralphx/test/workspace-review"],
    );
    git(
        &worktree_path,
        &["config", "user.email", "test@example.com"],
    );
    git(&worktree_path, &["config", "user.name", "Test User"]);
    std::fs::write(worktree_path.join("README.md"), "workspace review\n")
        .expect("workspace file should be written");
    git(&worktree_path, &["add", "README.md"]);
    git(&worktree_path, &["commit", "-m", "workspace"]);
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
    for _ in 0..20 {
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
    std::fs::create_dir_all(&worktree_path).expect("workspace directory should be created");
    git(
        &worktree_path,
        &["init", "-b", "ralphx/test/workspace-review"],
    );
    git(
        &worktree_path,
        &["config", "user.email", "test@example.com"],
    );
    git(&worktree_path, &["config", "user.name", "Test User"]);
    std::fs::write(worktree_path.join("README.md"), "workspace review\n")
        .expect("workspace file should be written");
    git(&worktree_path, &["add", "README.md"]);
    git(&worktree_path, &["commit", "-m", "workspace"]);
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
    for _ in 0..20 {
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
