use std::fs;
use std::process::Command;

use super::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
};
use super::agent_workspace_local_commit::{
    commit_agent_workspace_locally, AgentWorkspaceLocalCommitOutcome,
    AgentWorkspaceLocalCommitRequest,
};
use super::agent_workspace_review::{
    apply_review_artifact_to_monitor, load_agent_workspace_review_context,
};
use super::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentConversationWorkspacePublicationEvent,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    ArtifactId, ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::review::ReviewSettings;

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

async fn setup_workspace() -> (tempfile::TempDir, AppState, ChatConversationId, String) {
    let temp = tempfile::tempdir().expect("temporary repository");
    let repo = temp.path().join("repo");
    let worktrees = temp.path().join("worktrees");
    fs::create_dir_all(&repo).expect("repository root");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "RalphX Test"]);
    fs::write(repo.join("tracked.txt"), "base\n").expect("base file");
    fs::write(repo.join("delete.txt"), "delete me\n").expect("delete file");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    let state = AppState::new_test();
    let review_settings = ReviewSettings {
        require_workspace_review: false,
        ..Default::default()
    };
    state
        .review_settings_repo
        .update_settings(&review_settings)
        .await
        .expect("review settings");
    let mut project = Project::new(
        "Local commit".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.expect("project");
    let conversation_id = ChatConversationId::new();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id.clone();
    conversation.title = Some("Local commit test".to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation");
    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace");
    let head = git(
        std::path::Path::new(&workspace.worktree_path),
        &["rev-parse", "HEAD"],
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");
    (temp, state, conversation_id, head)
}

fn request(expected_head_sha: String, attempt_token: &str) -> AgentWorkspaceLocalCommitRequest {
    AgentWorkspaceLocalCommitRequest {
        expected_head_sha,
        review_artifact_id: None,
        review_artifact_version: None,
        reviewed_head_sha: None,
        reviewed_diff_fingerprint: None,
        attempt_token: attempt_token.to_string(),
        before_staging: None,
    }
}

async fn current_passing_review_request(
    state: &AppState,
    conversation_id: &ChatConversationId,
    expected_head_sha: String,
    attempt_token: &str,
) -> AgentWorkspaceLocalCommitRequest {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let context = load_agent_workspace_review_context(state, &workspace)
        .await
        .expect("review context");
    let target = context.target.expect("review target");
    let mut monitor = context.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("run-passed".to_string()),
        ArtifactId::from_string("artifact-passed"),
        1,
        chrono::Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("review monitor");
    AgentWorkspaceLocalCommitRequest {
        expected_head_sha,
        review_artifact_id: monitor.review_artifact_id.map(|id| id.as_str().to_string()),
        review_artifact_version: monitor.review_artifact_version,
        reviewed_head_sha: monitor.reviewed_head_sha,
        reviewed_diff_fingerprint: monitor.reviewed_diff_fingerprint,
        attempt_token: attempt_token.to_string(),
        before_staging: None,
    }
}

#[tokio::test]
async fn local_commit_commits_added_modified_and_deleted_files_without_publication_side_effects() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::Path::new(&workspace.worktree_path);
    fs::write(worktree.join("tracked.txt"), "modified\n").expect("modify file");
    fs::write(worktree.join("added.txt"), "added\n").expect("add file");
    fs::remove_file(worktree.join("delete.txt")).expect("delete file");

    let result =
        commit_agent_workspace_locally(&state, conversation_id.clone(), request(head, "a1"))
            .await
            .expect("local commit");

    assert_eq!(
        result.outcome,
        AgentWorkspaceLocalCommitOutcome::CommittedLocal
    );
    assert!(result.had_changes);
    assert_eq!(result.attempt_token, "a1");
    assert!(git(worktree, &["show", "--format=", "--name-only", "HEAD"]).contains("added.txt"));
    assert!(git(worktree, &["show", "--format=", "--name-only", "HEAD"]).contains("tracked.txt"));
    assert!(git(worktree, &["show", "--format=", "--name-only", "HEAD"]).contains("delete.txt"));
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    assert!(stored.publication_pr_number.is_none());
    assert!(stored.publication_push_status.is_none());
    assert!(stored.publication_pr_status.is_none());
    assert!(state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("publication events")
        .is_empty());
}

#[tokio::test]
async fn local_commit_retry_with_a_clean_newer_head_is_already_committed() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    fs::write(
        std::path::Path::new(&workspace.worktree_path).join("added.txt"),
        "added\n",
    )
    .expect("add file");
    let committed = commit_agent_workspace_locally(
        &state,
        conversation_id.clone(),
        request(head.clone(), "a1"),
    )
    .await
    .expect("first local commit");
    let retry = commit_agent_workspace_locally(&state, conversation_id, request(head, "a2"))
        .await
        .expect("idempotent retry");

    assert_eq!(
        committed.outcome,
        AgentWorkspaceLocalCommitOutcome::CommittedLocal
    );
    assert_eq!(
        retry.outcome,
        AgentWorkspaceLocalCommitOutcome::AlreadyCommitted
    );
    assert_eq!(retry.commit_sha, committed.commit_sha);
    assert_eq!(retry.attempt_token, "a2");
}

#[tokio::test]
async fn local_commit_returns_no_changes_for_the_current_clean_head() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;

    let result =
        commit_agent_workspace_locally(&state, conversation_id, request(head.clone(), "a1"))
            .await
            .expect("clean workspace is a successful no-op");

    assert_eq!(result.outcome, AgentWorkspaceLocalCommitOutcome::NoChanges);
    assert!(!result.had_changes);
    assert_eq!(result.previous_head_sha, head);
    assert_eq!(result.commit_sha, head);
}

#[tokio::test]
async fn local_commit_rejects_a_stale_head_when_changes_remain() {
    let (_temp, state, conversation_id, _head) = setup_workspace().await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::Path::new(&workspace.worktree_path);
    fs::write(worktree.join("added.txt"), "added\n").expect("add file");

    let error = commit_agent_workspace_locally(
        &state,
        conversation_id,
        request("stale-head".to_string(), "a1"),
    )
    .await
    .expect_err("stale branch state must not commit remaining changes");

    assert!(error.contains("Workspace branch changed since this commit attempt started"));
    assert_eq!(git(worktree, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(git(worktree, &["status", "--short"]), "?? added.txt");
}

#[tokio::test]
async fn local_commit_checks_identity_before_staging_changes() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::Path::new(&workspace.worktree_path);
    fs::write(worktree.join("added.txt"), "added\n").expect("add file");
    git(worktree, &["config", "user.name", ""]);
    git(worktree, &["config", "user.email", ""]);

    let error = commit_agent_workspace_locally(&state, conversation_id, request(head, "a1"))
        .await
        .expect_err("missing Git identity must reject before staging");

    assert!(error.contains("Git commit identity is not configured"));
    assert_eq!(git(worktree, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(git(worktree, &["status", "--short"]), "?? added.txt");
}

#[tokio::test]
async fn local_commit_rejects_an_active_persisted_publication_without_git_or_publication_mutation()
{
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::PathBuf::from(&workspace.worktree_path);
    fs::write(worktree.join("active-publication.txt"), "pending\n").expect("add file");
    workspace.publication_push_status = Some("checking".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist active publication status");
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "checking",
            "started",
            "Existing publication",
            None,
        ))
        .await
        .expect("persist publication event");
    let events_before = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("publication events");

    let error = commit_agent_workspace_locally(
        &state,
        conversation_id.clone(),
        request(head.clone(), "a1"),
    )
    .await
    .expect_err("active publication must reject local commit");

    assert!(error.contains("Commit & Publish is running"));
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&worktree, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(
        git(&worktree, &["status", "--short"]),
        "?? active-publication.txt"
    );
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    assert_eq!(stored.publication_push_status.as_deref(), Some("checking"));
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("publication events"),
        events_before
    );
}

#[tokio::test]
async fn local_commit_allows_a_non_active_needs_agent_publication_status() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::PathBuf::from(&workspace.worktree_path);
    fs::write(worktree.join("repair-follow-up.txt"), "pending\n").expect("add file");
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist unavailable publication state");

    let result =
        commit_agent_workspace_locally(&state, conversation_id.clone(), request(head, "a1"))
            .await
            .expect("non-active publication status must allow the local commit action");

    assert_eq!(
        result.outcome,
        AgentWorkspaceLocalCommitOutcome::CommittedLocal
    );
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    assert_eq!(
        stored.publication_push_status.as_deref(),
        Some("needs_agent")
    );
}

#[tokio::test]
async fn local_commit_required_review_accepts_exact_receipt_and_preserves_equivalent_authority() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let review_settings = ReviewSettings {
        require_workspace_review: true,
        ..Default::default()
    };
    state
        .review_settings_repo
        .update_settings(&review_settings)
        .await
        .expect("review settings");
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::Path::new(&workspace.worktree_path);
    fs::write(worktree.join("reviewed.txt"), "reviewed content\n").expect("reviewed change");
    let request = current_passing_review_request(&state, &conversation_id, head, "review-1").await;

    let result = commit_agent_workspace_locally(&state, conversation_id.clone(), request)
        .await
        .expect("current passing review should authorize local commit");

    assert_eq!(
        result.outcome,
        AgentWorkspaceLocalCommitOutcome::CommittedLocal
    );
    assert_eq!(git(worktree, &["diff", "--cached", "--name-only"]), "");
    let after_commit = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("equivalent committed review context");
    assert!(after_commit.is_current);
    assert_eq!(
        after_commit.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
}

#[tokio::test]
async fn local_commit_allows_an_optional_review_receipt() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::Path::new(&workspace.worktree_path);
    fs::write(worktree.join("optional-reviewed.txt"), "optional review\n")
        .expect("optional reviewed change");
    let request =
        current_passing_review_request(&state, &conversation_id, head, "optional-1").await;

    let result = commit_agent_workspace_locally(&state, conversation_id, request)
        .await
        .expect("optional review receipt must not block a local commit");

    assert_eq!(
        result.outcome,
        AgentWorkspaceLocalCommitOutcome::CommittedLocal
    );
}

#[tokio::test]
async fn local_commit_required_review_rejects_mismatched_receipt_before_staging() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let review_settings = ReviewSettings {
        require_workspace_review: true,
        ..Default::default()
    };
    state
        .review_settings_repo
        .update_settings(&review_settings)
        .await
        .expect("review settings");
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::Path::new(&workspace.worktree_path);
    fs::write(worktree.join("reviewed.txt"), "reviewed content\n").expect("reviewed change");
    let mut request =
        current_passing_review_request(&state, &conversation_id, head, "review-2").await;
    request.reviewed_diff_fingerprint = Some("stale-fingerprint".to_string());

    let error = commit_agent_workspace_locally(&state, conversation_id, request)
        .await
        .expect_err("stale review receipt must reject before staging");

    assert!(error.contains("receipt changed"));
    assert_eq!(git(worktree, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(git(worktree, &["status", "--short"]), "?? reviewed.txt");
}

fn add_unreviewed_change_after_receipt_validation(worktree: &std::path::Path) {
    fs::write(worktree.join("unreviewed.txt"), "unreviewed content\n")
        .expect("inject unreviewed change");
}

fn advance_head_after_staging(worktree: &std::path::Path) {
    git(
        worktree,
        &[
            "commit",
            "--allow-empty",
            "--only",
            "--no-verify",
            "-m",
            "concurrent commit",
        ],
    );
}

#[tokio::test]
async fn local_commit_rejects_a_head_advance_after_staging_and_restores_the_index() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::Path::new(&workspace.worktree_path);
    fs::write(worktree.join("stale-after-stage.txt"), "pending\n").expect("add file");
    let mut request = request(head.clone(), "cas-1");
    request.before_staging = Some(advance_head_after_staging);

    let error = commit_agent_workspace_locally(&state, conversation_id, request)
        .await
        .expect_err("a changed HEAD immediately before commit must reject the stale request");

    assert!(error.contains("Workspace branch changed since this commit attempt started"));
    assert_ne!(git(worktree, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(worktree, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(
        git(worktree, &["status", "--short"]),
        "?? stale-after-stage.txt"
    );
    assert_eq!(
        git(worktree, &["show", "--format=", "--name-only", "HEAD"]),
        ""
    );
}

#[tokio::test]
async fn local_commit_required_review_rejects_snapshot_drift_and_restores_the_index() {
    let (_temp, state, conversation_id, head) = setup_workspace().await;
    let review_settings = ReviewSettings {
        require_workspace_review: true,
        ..Default::default()
    };
    state
        .review_settings_repo
        .update_settings(&review_settings)
        .await
        .expect("review settings");
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let worktree = std::path::Path::new(&workspace.worktree_path);
    fs::write(worktree.join("reviewed.txt"), "reviewed content\n").expect("reviewed change");
    let mut request =
        current_passing_review_request(&state, &conversation_id, head.clone(), "review-3").await;
    request.before_staging = Some(add_unreviewed_change_after_receipt_validation);

    let error = commit_agent_workspace_locally(&state, conversation_id, request)
        .await
        .expect_err("snapshot drift must reject before committing");

    assert!(error.contains("Workspace Review is required"));
    assert_eq!(git(worktree, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(worktree, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(
        git(worktree, &["status", "--short"]),
        "?? reviewed.txt\n?? unreviewed.txt"
    );
}

#[tokio::test]
async fn local_commit_required_review_rejects_running_and_blocking_states_before_staging() {
    for (suffix, gate_status, expected_error) in [
        (
            "running",
            AgentWorkspaceReviewGateStatus::Reviewing,
            "still running",
        ),
        (
            "blocking",
            AgentWorkspaceReviewGateStatus::Blocking,
            "blocking changes",
        ),
    ] {
        let (_temp, state, conversation_id, head) = setup_workspace().await;
        let review_settings = ReviewSettings {
            require_workspace_review: true,
            ..Default::default()
        };
        state
            .review_settings_repo
            .update_settings(&review_settings)
            .await
            .expect("review settings");
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup")
            .expect("workspace");
        let worktree = std::path::Path::new(&workspace.worktree_path);
        fs::write(worktree.join("reviewed.txt"), "reviewed content\n").expect("reviewed change");
        let context = load_agent_workspace_review_context(&state, &workspace)
            .await
            .expect("review context");
        let target = context.target.expect("review target");
        let mut monitor = context.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some(format!("run-{suffix}")),
            ArtifactId::from_string(format!("artifact-{suffix}")),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.status = if gate_status == AgentWorkspaceReviewGateStatus::Reviewing {
            AgentWorkspaceReviewMonitorStatus::Reviewing
        } else {
            AgentWorkspaceReviewMonitorStatus::Ready
        };
        monitor.review_outcome = if gate_status == AgentWorkspaceReviewGateStatus::Blocking {
            AgentWorkspaceReviewOutcome::Blocking
        } else {
            AgentWorkspaceReviewOutcome::None
        };
        if gate_status == AgentWorkspaceReviewGateStatus::Blocking {
            monitor.review_blocking_summary =
                Some("Workspace Review found blocking changes".to_string());
        }
        monitor.review_gate_status = gate_status;
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("review monitor");

        let error = commit_agent_workspace_locally(&state, conversation_id, request(head, suffix))
            .await
            .expect_err("non-passing review states must reject before staging");

        assert!(error.contains(expected_error));
        assert_eq!(git(worktree, &["diff", "--cached", "--name-only"]), "");
        assert_eq!(git(worktree, &["status", "--short"]), "?? reviewed.txt");
    }
}
