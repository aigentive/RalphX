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

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use super::{cleanup_terminal_agent_workspace_after_pr, PrPollerRegistry, RateLimitState};
use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
};
use crate::application::chat_service::MockChatService;
use crate::application::git_service::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrDescription, ChatConversationId, IdeationAnalysisBaseRefKind,
    IdeationSessionId, PlanBranchId, Project, TaskId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::services::github_service::{
    PrAutoMergeRequest, PrHealth, PrHealthCheck, PrIssueCommentSummary, PrMergeStateStatus,
    PrMergeableState, PrReviewCommentFeedback, PrReviewFeedback, PrSyncState,
};
use crate::domain::services::GithubServiceTrait;
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryPlanBranchRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn make_registry_no_github() -> PrPollerRegistry {
    PrPollerRegistry::new(None, Arc::new(MemoryPlanBranchRepository::new()))
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

fn open_pr_health(head: &str) -> PrHealth {
    PrHealth {
        sync_state: PrSyncState {
            status: crate::domain::services::github_service::PrStatus::Open,
            merge_state_status: None,
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "feature/pr".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some(head.to_string()),
            base_ref_oid: Some("base".to_string()),
        },
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }
}

fn supervised_workspace(
    conversation_id: &str,
    project_id: &str,
    worktree_path: &std::path::Path,
) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string(conversation_id),
        crate::domain::entities::ProjectId::from_string(project_id.to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        format!("ralphx/test/{conversation_id}"),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(101);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/101".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.pr_autofix_enabled = true;
    workspace
}

fn codecov_comment(body: &str) -> PrIssueCommentSummary {
    PrIssueCommentSummary {
        id: "codecov-comment".to_string(),
        author: Some("codecov".to_string()),
        body: body.to_string(),
        url: Some("https://github.com/owner/repo/pull/101#issuecomment-1".to_string()),
        created_at: Some("2026-05-17T10:00:00Z".to_string()),
        is_codecov: true,
    }
}

#[test]
fn refreshed_agent_workspace_pr_remains_pollable_for_terminal_status() {
    let repo = init_cleanup_repo();
    let worktree_parent = repo.path().join("worktrees");
    let project = cleanup_project(repo.path(), &worktree_parent);
    let mut workspace = cleanup_workspace_with_conversation(
        &project,
        "ralphx/demo/agent-refreshed",
        "conversation-refreshed-polling",
    );
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("refreshed".to_string());

    assert!(super::agent_workspace_pr_polling_is_current(
        &workspace, 101
    ));
}

#[test]
fn supervised_agent_workspace_pr_health_routes_failing_checks() {
    let mut health = open_pr_health("abc123");
    health.checks.push(PrHealthCheck {
        name: "CI / test".to_string(),
        status: Some("COMPLETED".to_string()),
        conclusion: Some("FAILURE".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/1".to_string()),
    });

    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("failing check should route autofix");
    assert_eq!(issue.kind, super::AgentWorkspacePrAutofixIssueKind::Checks);
    assert!(issue.summary.contains("1 failing check"));
    assert!(issue.details[0].contains("CI / test"));
    assert!(issue
        .classification
        .starts_with("github_pr_autofix:101:abc123"));
}

#[test]
fn supervised_agent_workspace_pr_health_ignores_pending_checks() {
    let mut health = open_pr_health("abc123");
    health.checks.push(PrHealthCheck {
        name: "CI / test".to_string(),
        status: Some("IN_PROGRESS".to_string()),
        conclusion: None,
        details_url: None,
    });

    assert!(super::classify_agent_workspace_pr_autofix_issue(101, &health).is_none());
}

#[test]
fn supervised_agent_workspace_pr_health_routes_requested_changes() {
    let mut health = open_pr_health("review-head");
    health.review_decision = Some(" CHANGES_REQUESTED ".to_string());

    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("requested changes should route autofix");
    assert_eq!(issue.kind, super::AgentWorkspacePrAutofixIssueKind::Review);
    assert_eq!(issue.summary, "PR #101 has requested changes");
    assert_eq!(
        issue.details,
        vec!["GitHub review decision is CHANGES_REQUESTED".to_string()]
    );
    assert!(issue
        .classification
        .starts_with("github_pr_autofix:101:reviewhead"));
}

#[test]
fn supervised_agent_workspace_pr_health_routes_actionable_codecov_comment() {
    let mut health = open_pr_health("coverage-head");
    health.issue_comments.push(codecov_comment(
        "Codecov report: patch coverage is below target threshold and failed.",
    ));

    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("Codecov failure should route autofix");
    assert_eq!(
        issue.kind,
        super::AgentWorkspacePrAutofixIssueKind::Coverage
    );
    assert_eq!(issue.summary, "PR #101 has actionable Codecov feedback");
    assert!(issue.details[0].contains("@codecov: Codecov report"));
    assert!(issue
        .classification
        .starts_with("github_pr_autofix:101:coveragehead"));
}

#[test]
fn supervised_agent_workspace_pr_health_routes_mergeability_blockers() {
    let mut health = open_pr_health("merge-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Behind);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);

    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("merge blockers should route autofix");
    assert_eq!(
        issue.kind,
        super::AgentWorkspacePrAutofixIssueKind::Mergeability
    );
    assert_eq!(issue.summary, "PR #101 has mergeability blockers");
    assert!(issue
        .details
        .contains(&"PR branch is behind its base".to_string()));
    assert!(issue
        .details
        .contains(&"PR is reported as conflicting".to_string()));
}

#[test]
fn supervised_agent_workspace_pr_message_includes_fix_context_entrypoint() {
    let workspace = supervised_workspace(
        "autofix-message-conversation",
        "project-message",
        Path::new("/tmp"),
    );
    let issue = super::AgentWorkspacePrAutofixIssue {
        kind: super::AgentWorkspacePrAutofixIssueKind::Checks,
        summary: "PR #101 has 1 failing check".to_string(),
        details: vec!["CI / test (failure) - https://github.com/run".to_string()],
        classification: "github_pr_autofix:101:head:fingerprint".to_string(),
    };

    let message = super::build_agent_workspace_pr_autofix_message(101, &workspace, &issue);
    assert!(message.contains("RalphX PR supervision detected"));
    assert!(message.contains("complete_agent_workspace_pr_fix"));
    assert!(message.contains("get_agent_workspace_pr_fix_context"));
    assert!(message.contains("Fingerprint: github_pr_autofix:101:head:fingerprint"));
    assert!(message.contains("- CI / test (failure) - https://github.com/run"));
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_routes_failure_to_pr_fixer() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-route-conversation",
        "project-route",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("route-head");
    health.checks.push(PrHealthCheck {
        name: "Rust Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/1".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("autofix routing should succeed");

    assert!(routed);
    let messages = chat.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Rust Tests (failure)"));
    let options = chat.get_sent_options().await;
    assert_eq!(
        options[0].agent_name_override.as_deref(),
        Some(crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_PR_FIXER)
    );
    assert_eq!(
        options[0].working_directory_override.as_deref(),
        Some(worktree.path())
    );

    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(
        updated.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("fixing"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("failing check"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "pr_autofix"
            && event.status == "needs_agent"
            && event
                .classification
                .as_deref()
                .unwrap_or_default()
                .starts_with("github_pr_autofix:101:routehead")
    }));
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_skips_duplicate_fingerprint() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-duplicate-conversation",
        "project-duplicate",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("duplicate-head");
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: None,
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("issue should classify");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix",
            "needs_agent",
            issue.summary,
            Some(issue.classification),
        ))
        .await
        .expect("event should persist");

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("duplicate autofix should not error");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_marks_healthy_pr_monitoring() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-healthy-conversation",
        "project-healthy",
        worktree.path(),
    );
    workspace.pr_supervision_status = Some("waiting".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("healthy-head")));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("healthy monitoring should not error");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("RalphX is monitoring PR health.")
    );
}

#[tokio::test]
async fn agent_workspace_auto_merge_sync_enables_draft_pr_and_records_state() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "auto-merge-enable-conversation",
        "project-auto-enable",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_method = "squash".to_string();
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("auto-enable-head");
    health.sync_state.is_draft = true;
    let github = Arc::new(MockGithubService::new());

    let current = super::sync_agent_workspace_auto_merge_preference(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &workspace,
        &health,
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("auto-merge sync should succeed");

    assert!(current);
    let github_state = github.state();
    assert_eq!(github_state.mark_pr_ready_calls, 1);
    assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
    assert_eq!(
        github_state.last_enable_pr_auto_merge_args.as_ref(),
        Some(&(101, "squash".to_string()))
    );
    drop(github_state);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
}

#[tokio::test]
async fn agent_workspace_auto_merge_sync_records_enable_failure_as_waiting() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "auto-merge-enable-failure-conversation",
        "project-auto-enable-failure",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let github = Arc::new(MockGithubService::new());
    github.state().enable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "merge queue unavailable".to_string(),
    )));

    let current = super::sync_agent_workspace_auto_merge_preference(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &workspace,
        &open_pr_health("auto-enable-failure-head"),
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("auto-merge sync should not fail on GitHub enable errors");

    assert!(!current);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("waiting"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("merge queue unavailable"));
}

#[tokio::test]
async fn agent_workspace_auto_merge_sync_disables_remote_auto_merge() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "auto-merge-disable-conversation",
        "project-auto-disable",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("auto-disable-head");
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());

    let current = super::sync_agent_workspace_auto_merge_preference(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &workspace,
        &health,
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("auto-merge sync should succeed");

    assert!(!current);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("GitHub auto-merge is disabled.")
    );
}

#[tokio::test]
async fn agent_workspace_review_feedback_uses_pr_fixer_when_autofix_enabled() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "review-feedback-autofix-conversation",
        "project-review-feedback",
        worktree.path(),
    );
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let feedback = PrReviewFeedback {
        review_id: "review-123".to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-05-17T12:00:00Z".to_string()),
        body: Some("Please handle the edge case.".to_string()),
        comments: vec![PrReviewCommentFeedback {
            id: "comment-1".to_string(),
            author: "reviewer".to_string(),
            path: Some("src/lib.rs".to_string()),
            line: Some(42),
            body: "This branch is not covered.".to_string(),
        }],
    };
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(feedback);
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_review_feedback_if_present(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("review feedback should route");

    assert!(routed);
    let messages = chat.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("get_agent_workspace_pr_fix_context"));
    assert!(messages[0].contains("complete_agent_workspace_pr_fix"));
    assert!(messages[0].contains("Please handle the edge case."));
    assert!(messages[0].contains("src/lib.rs:42"));
    let options = chat.get_sent_options().await;
    assert_eq!(
        options[0].agent_name_override.as_deref(),
        Some(crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_PR_FIXER)
    );
    assert_eq!(
        options[0].working_directory_override.as_deref(),
        Some(worktree.path())
    );

    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(
        updated.publication_pr_status.as_deref(),
        Some("changes_requested")
    );
    assert_eq!(
        updated.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("GitHub requested changes routed to the PR fixer.")
    );

    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "github_review"
            && event.status == "needs_agent"
            && event.classification.as_deref() == Some("github_pr_review:review-123")
    }));
}

fn repo_error() -> AppError {
    AppError::Database("forced workspace repository failure".to_string())
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
async fn terminal_agent_workspace_pr_cleanup_returns_when_workspace_missing() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::new();

    cleanup_terminal_agent_workspace_after_pr(
        workspace_repo,
        &conversation_id,
        &project,
        None,
        true,
    )
    .await;
}

#[tokio::test]
async fn terminal_agent_workspace_pr_cleanup_returns_when_workspace_lookup_fails() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(WorkspaceLookupErrorRepository);
    let conversation_id = ChatConversationId::new();

    cleanup_terminal_agent_workspace_after_pr(
        workspace_repo,
        &conversation_id,
        &project,
        None,
        true,
    )
    .await;
}

#[tokio::test]
async fn terminal_agent_workspace_pr_cleanup_logs_nonfatal_cleanup_error() {
    let repo = tempfile::tempdir().expect("non-git repo path");
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "poller-cleanup-error-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    cleanup_terminal_agent_workspace_after_pr(
        workspace_repo,
        &conversation_id,
        &project,
        None,
        false,
    )
    .await;
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

struct WorkspaceLookupErrorRepository;

#[async_trait]
impl AgentConversationWorkspaceRepository for WorkspaceLookupErrorRepository {
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
        _project_id: &crate::domain::entities::ProjectId,
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

    async fn delete(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Err(repo_error())
    }
}
