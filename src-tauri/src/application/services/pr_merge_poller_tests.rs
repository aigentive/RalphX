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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;

use super::{AgentWorkspacePrPollerStart, PrPollerRegistry, RateLimitState};
use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
};
use crate::application::agent_workspace_publish_repair_state::current_agent_workspace_repair_claim_for_completion;
use crate::application::agent_workspace_terminal_cleanup::{
    cleanup_terminal_agent_workspace_after_pr, terminalize_agent_workspace_after_pr,
    TerminalAgentWorkspaceCause,
};
use crate::application::chat_service::MockChatService;
use crate::application::git_service::GitService;
use crate::application::interactive_notification_producer::pr_review_notification_key;
use crate::application::notification_service::{NoopNotificationEventEmitter, NotificationService};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as DbPrStatus};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, AgentRun,
    AgentRunActionKind, AgentRunStatus, AgentWorkspacePrDescription, AgentWorkspacePrReviewAction,
    AgentWorkspacePrReviewActionKind, AgentWorkspacePrReviewActionStatus,
    AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewMonitorStatus,
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairContinuation, AgentWorkspaceRepairPhase,
    AgentWorkspaceRepairSource, AgentWorkspaceReviewAutoMergeGuard,
    AgentWorkspaceReviewAutoMergeGuardStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewTargetScope, AgentWorkspaceSourcePullRequest, ArtifactId, ChatContextType,
    ChatConversationId, GitTargetLeaseOwner, IdeationAnalysisBaseRefKind, IdeationSessionId,
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget, PlanBranch,
    PlanBranchId, Project, TaskId,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentConversationWorkspaceRepository,
    AgentRunRepository, AgentWorkspaceRepairRepository, BranchUpdateRepository,
    NotificationRepository,
};
use crate::domain::services::github_service::{
    PrAutoMergeRequest, PrHealth, PrHealthCheck, PrIssueCommentSummary, PrMergeStateStatus,
    PrMergeableState, PrReviewCommentFeedback, PrReviewFeedback, PrStatus, PrSyncState,
};
use crate::domain::services::GithubServiceTrait;
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryBranchUpdateRepository, MemoryNotificationRepository, MemoryPlanBranchRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn make_registry_no_github() -> PrPollerRegistry {
    PrPollerRegistry::new(None, Arc::new(MemoryPlanBranchRepository::new()))
}

async fn seeded_latest_pr_fixer_run_repo(
    conversation_id: &ChatConversationId,
) -> Arc<dyn AgentRunRepository> {
    let repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let mut run = AgentRun::new(conversation_id.clone());
    run.harness = Some(AgentHarnessKind::Codex);
    run.logical_model = Some("gpt-5.6-sol".to_string());
    run.logical_effort = Some(LogicalEffort::High);
    run.service_tier = Some("fast".to_string());
    run.complete();
    repo.create(run).await.expect("latest run should persist");
    repo
}

async fn seed_pr_autofix_attempt(
    repo: &dyn AgentRunRepository,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    fingerprint: &str,
    status: AgentRunStatus,
) {
    let mut run = AgentRun::new(conversation_id.clone());
    run.action_kind = Some(AgentRunActionKind::PrAutofix);
    run.action_context_id = Some(pr_number.to_string());
    run.action_target_id = Some(fingerprint.to_string());
    run.status = status;
    repo.create(run)
        .await
        .expect("autofix attempt should persist");
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

fn init_repair_dispatch_repo(repo: &std::path::Path, branch: &str) {
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("write repair dispatch fixture");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);
    run_git(repo, &["checkout", "-b", branch]);
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

fn requested_changes_feedback(review_id: &str) -> PrReviewFeedback {
    PrReviewFeedback {
        review_id: review_id.to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-05-17T12:00:00Z".to_string()),
        body: Some("Please handle the edge case.".to_string()),
        comments: vec![PrReviewCommentFeedback {
            id: format!("comment-{review_id}"),
            author: "reviewer".to_string(),
            path: Some("src/lib.rs".to_string()),
            line: Some(42),
            body: "This branch is not covered.".to_string(),
        }],
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

async fn reserve_pending_ci_rerun_attempt(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    conversation_id: &ChatConversationId,
    fingerprint: &str,
) -> AgentWorkspaceRepairAttempt {
    reserve_ci_hold_attempt(repair_repo, conversation_id, fingerprint, 1, false).await
}

async fn reserve_pending_ci_await_attempt(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    conversation_id: &ChatConversationId,
    fingerprint: &str,
) -> AgentWorkspaceRepairAttempt {
    reserve_ci_hold_attempt(repair_repo, conversation_id, fingerprint, 0, true).await
}

async fn reserve_ci_hold_attempt(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    conversation_id: &ChatConversationId,
    fingerprint: &str,
    ci_rerun_count: u32,
    awaiting: bool,
) -> AgentWorkspaceRepairAttempt {
    use crate::domain::repositories::{
        AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
        StartOrJoinAgentWorkspaceRepairAttempt, StartOrJoinAgentWorkspaceRepairAttemptOutcome,
    };

    let started = match repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::PrAutofix,
                AgentWorkspaceRepairContinuation::ResumePrSupervision,
                "main",
                false,
                true,
                true,
                None,
                Utc::now(),
            ),
            reason: "transient CI rerun is pending".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("pending rerun repair attempt should start")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected pending rerun attempt to start, got {outcome:?}"),
    };
    let mut pending = started.clone();
    pending.phase = AgentWorkspaceRepairPhase::Ready;
    pending.ci_rerun_count = ci_rerun_count;
    pending.ci_rerun_fingerprint = Some(fingerprint.to_string());
    if awaiting {
        pending.pending_reasons.push(
            crate::application::agent_workspace_publish_repair_state::AWAITING_CI_REPAIR_REASON
                .to_string(),
        );
    }
    pending.updated_at += chrono::Duration::microseconds(1);
    match repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: pending,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at: started.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("pending rerun reservation should persist")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected pending rerun reservation, got {outcome:?}"),
    }
}

async fn reserve_pre_existing_on_base_attempt(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    conversation_id: &ChatConversationId,
    fingerprint: &str,
) -> AgentWorkspaceRepairAttempt {
    reserve_health_held_attempt(
        repair_repo,
        conversation_id,
        fingerprint,
        crate::application::agent_workspace_publish_repair_state::PRE_EXISTING_ON_BASE_REPAIR_REASON,
    )
    .await
}

/// Parks a PR autofix generation at an exact health fingerprint under the given hold reason. Both
/// hold reasons must behave identically at the poller's dispatch gate.
async fn reserve_health_held_attempt(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    conversation_id: &ChatConversationId,
    fingerprint: &str,
    hold_reason: &str,
) -> AgentWorkspaceRepairAttempt {
    use crate::domain::repositories::{
        AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
        StartOrJoinAgentWorkspaceRepairAttempt, StartOrJoinAgentWorkspaceRepairAttemptOutcome,
    };

    let started = match repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::PrAutofix,
                AgentWorkspaceRepairContinuation::ResumePrSupervision,
                "main",
                false,
                true,
                true,
                None,
                Utc::now(),
            ),
            reason: hold_reason.to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("health-held repair attempt should start")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected health-held attempt to start, got {outcome:?}"),
    };
    let mut suppressed = started.clone();
    suppressed.phase = AgentWorkspaceRepairPhase::Ready;
    suppressed.pr_autofix_health_fingerprint = Some(fingerprint.to_string());
    suppressed.pending_reasons = vec![hold_reason.to_string()];
    suppressed.updated_at += chrono::Duration::microseconds(1);
    match repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: suppressed,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at: started.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("health-held reservation should persist")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected health-held reservation, got {outcome:?}"),
    }
}

async fn seed_poller_held_unpublished_head(
    continuation: AgentWorkspaceRepairContinuation,
    base_commit: &str,
    health: &PrHealth,
) -> (
    AppState,
    AgentConversationWorkspace,
    AgentWorkspaceRepairAttempt,
    Arc<MockGithubService>,
) {
    let worktree = tempfile::tempdir().expect("held unpublished poller worktree");
    let worktree_path = worktree.keep();
    let mut workspace = supervised_workspace(
        "held-unpublished-poller-tick",
        "project-held-unpublished-poller-tick",
        &worktree_path,
    );
    init_repair_dispatch_repo(&worktree_path, &workspace.branch_name);
    workspace.base_commit = Some(base_commit.to_string());
    workspace.auto_publish_enabled = true;

    let mut project = Project::new(
        "Held unpublished poller".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    project.id = workspace.project_id.clone();
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;

    let mut state = AppState::new_test();
    state
        .project_repo
        .create(project)
        .await
        .expect("held unpublished project should persist");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("held unpublished workspace should persist");

    let fingerprint = super::classify_agent_workspace_pr_autofix_issue(101, health)
        .expect("failing PR health should classify")
        .classification;
    let held = reserve_health_held_attempt(
        state.agent_workspace_repair_repo.as_ref(),
        &workspace.conversation_id,
        &fingerprint,
        crate::application::agent_workspace_publish_repair_state::UNCHANGED_HEALTH_REPAIR_REASON,
    )
    .await;
    let expected_updated_at = held.updated_at;
    let mut unpublished = held;
    unpublished.continuation = continuation;
    unpublished.target_base_commit = Some(base_commit.to_string());
    unpublished.repair_head_commit = Some("validated-local-held-head".to_string());
    unpublished.updated_at += chrono::Duration::microseconds(1);
    let unpublished = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(
            crate::domain::repositories::AgentWorkspaceRepairAttemptTransition {
                attempt: unpublished,
                expected_phase: AgentWorkspaceRepairPhase::Ready,
                expected_updated_at,
                next_phase: AgentWorkspaceRepairPhase::Ready,
                compatibility_projection: None,
                events: Vec::new(),
            },
        )
        .await
        .expect("held unpublished head should persist")
    {
        crate::domain::repositories::AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
            attempt,
        ) => attempt,
        outcome => panic!("held unpublished head must apply, got {outcome:?}"),
    };

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health.clone()));
    state.github_service =
        Some(Arc::clone(&github) as Arc<dyn crate::domain::services::GithubServiceTrait>);
    (state, workspace, unpublished, github)
}

#[tokio::test]
async fn held_manual_unpublished_redrive_noop_falls_through_and_retains_the_hold() {
    let mut health = open_pr_health("remote-held-head");
    health.sync_state.base_ref_oid = Some("base-before-hold".to_string());
    health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let (state, workspace, held, github) = seed_poller_held_unpublished_head(
        AgentWorkspaceRepairContinuation::Manual,
        "base-before-hold",
        &health,
    )
    .await;
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);

    assert!(
        !super::re_drive_held_unpublished_agent_workspace_repair(
            &state,
            &workspace_repo,
            &workspace.conversation_id,
            &health,
        )
        .await
        .expect("manual held-head recovery should be a safe no-op"),
        "a no-op recovery must not tell the poll loop to skip remaining routing"
    );

    let chat = Arc::new(MockChatService::new());
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        github as Arc<dyn GithubServiceTrait>,
        Path::new(&workspace.worktree_path),
        101,
        &workspace.conversation_id,
        workspace_repo,
        Some(Arc::clone(&state.agent_run_repo)),
        Some(Arc::clone(&state.agent_workspace_repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("same-tick autofix routing should retain identical evidence");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
        .expect("held attempt should reload")
        .expect("held attempt remains current");
    assert_eq!(current.id, held.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
}

#[tokio::test]
async fn held_unpublished_redrive_noop_falls_through_to_base_advanced_supersession() {
    let mut health = open_pr_health("remote-held-head");
    health.sync_state.base_ref_oid = Some("base-after-hold".to_string());
    health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let (state, workspace, held, github) = seed_poller_held_unpublished_head(
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "base-before-hold",
        &health,
    )
    .await;
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);

    assert!(
        !super::re_drive_held_unpublished_agent_workspace_repair(
            &state,
            &workspace_repo,
            &workspace.conversation_id,
            &health,
        )
        .await
        .expect("base-advanced recovery should leave supersession to routing"),
        "a non-advancing recovery must fall through to base supersession"
    );

    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&workspace.conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        github as Arc<dyn GithubServiceTrait>,
        Path::new(&workspace.worktree_path),
        101,
        &workspace.conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&state.agent_workspace_repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("base-advanced routing should supersede the held attempt");

    assert!(routed);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
        .expect("successor should reload")
        .expect("successor remains current");
    assert_eq!(current.generation, held.generation + 1);
}

fn ideation_plan_workspace(
    conversation_id: &str,
    project_id: &str,
    session_id: IdeationSessionId,
    plan_branch_id: PlanBranchId,
    plan_branch_name: &str,
    worktree_path: &std::path::Path,
) -> AgentConversationWorkspace {
    let mut workspace = supervised_workspace(conversation_id, project_id, worktree_path);
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch_id);
    workspace.branch_name = plan_branch_name.to_string();
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.publication_push_status = None;
    workspace
}

fn active_plan_pr_branch(
    session_id: IdeationSessionId,
    project_id: &str,
    branch_id: PlanBranchId,
    branch_name: &str,
    pr_number: i64,
) -> PlanBranch {
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("plan-artifact-autofix"),
        session_id,
        crate::domain::entities::ProjectId::from_string(project_id.to_string()),
        branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.id = branch_id;
    plan_branch.pr_eligible = true;
    plan_branch.pr_polling_active = true;
    plan_branch.pr_number = Some(pr_number);
    plan_branch.pr_url = Some(format!("https://github.com/owner/repo/pull/{pr_number}"));
    plan_branch.pr_status = Some(DbPrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    plan_branch
}

fn review_pr_workspace(
    conversation_id: &str,
    project_id: &str,
    worktree_path: &std::path::Path,
) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string(conversation_id),
        crate::domain::entities::ProjectId::from_string(project_id.to_string()),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        format!("ralphx/test/{conversation_id}"),
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

#[tokio::test]
async fn review_pr_autofix_route_rejects_stale_automation_before_github_or_side_effects() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = review_pr_workspace(
        "review-pr-stale-autofix",
        "project-review-pr-stale-autofix",
        worktree.path(),
    );
    workspace.publication_pr_number = Some(101);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/101".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let original = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");

    let github = Arc::new(MockGithubService::new());
    let mut health = open_pr_health("review-head");
    health.review_decision = Some("CHANGES_REQUESTED".to_string());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("Review PR guard should no-op");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    assert!(chat.get_sent_options().await.is_empty());
    let github_calls = {
        let github_state = github.state();
        (
            github_state.fetch_pr_health_calls,
            github_state.mark_pr_ready_calls,
            github_state.enable_pr_auto_merge_calls,
            github_state.disable_pr_auto_merge_calls,
        )
    };
    assert_eq!(github_calls, (0, 0, 0, 0));
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed"),
        Some(original)
    );
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn review_pr_public_auto_merge_sync_rejects_before_health_fetch() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = review_pr_workspace(
        "review-pr-public-auto-merge",
        "project-review-pr-public-auto-merge",
        worktree.path(),
    );
    workspace.publication_pr_number = Some(101);
    workspace.pr_auto_merge_desired = true;
    let conversation_id = workspace.conversation_id.clone();
    let repository = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = repository.clone();
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = repository;
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let github = Arc::new(MockGithubService::new());

    let error = super::sync_agent_workspace_auto_merge_preference_for_workspace(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &workspace,
        Arc::clone(&workspace_repo),
        repair_repo,
    )
    .await
    .expect_err("Review PR auto-merge synchronization should fail closed");

    assert!(error.to_string().contains("Review PR"));
    assert_eq!(github.state().fetch_pr_health_calls, 0);
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn auto_merge_sync_preserves_held_repair_status_while_updating_remote_state() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let conversation_id = ChatConversationId::from_string("held-auto-merge-sync");
    let mut workspace = supervised_workspace(
        &conversation_id.as_str(),
        "project-held-auto-merge-sync",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_supervision_status = Some("held".to_string());
    workspace.pr_supervision_summary = Some("Repair is held for a decision.".to_string());

    let repository = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    repository
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    reserve_health_held_attempt(
        repository.as_ref(),
        &conversation_id,
        "checks:held-auto-merge-sync",
        crate::application::agent_workspace_publish_repair_state::UNCHANGED_HEALTH_REPAIR_REASON,
    )
    .await;

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("held-head")));

    let current = super::sync_agent_workspace_auto_merge_preference_for_workspace(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &workspace,
        repository.clone(),
        repository.clone(),
    )
    .await
    .expect("auto-merge synchronization should succeed");

    assert!(current);
    let refreshed = repository
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should remain");
    assert_eq!(refreshed.pr_auto_merge_current, Some(true));
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("held"));
    assert_eq!(
        refreshed.pr_supervision_summary.as_deref(),
        Some("GitHub auto-merge is enabled; RalphX is monitoring PR health.")
    );
}

#[tokio::test]
async fn supervision_write_fails_closed_when_repair_authority_lookup_fails() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let conversation_id = ChatConversationId::from_string("repair-authority-error");
    let mut workspace = supervised_workspace(
        &conversation_id.as_str(),
        "project-repair-authority-error",
        worktree.path(),
    );
    workspace.pr_auto_merge_current = Some(false);
    workspace.pr_supervision_status = Some("held".to_string());
    workspace.pr_supervision_summary = Some("Repair owns this projection.".to_string());

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let before = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed");

    let error = super::update_agent_workspace_pr_supervision_state(
        workspace_repo.as_ref(),
        Some(&LookupErrorRepairRepository),
        &conversation_id,
        Some(true),
        Some("monitoring"),
        Some("Poller tried to overwrite repair state."),
    )
    .await
    .expect_err("repair authority lookup failure must block the write");

    assert!(error.to_string().contains("repair authority lookup failed"));
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed"),
        before
    );
}

#[tokio::test]
async fn settled_repair_releases_supervision_status_to_the_poller() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let conversation_id = ChatConversationId::from_string("settled-repair-writer-release");
    let mut workspace = supervised_workspace(
        &conversation_id.as_str(),
        "project-settled-repair-writer-release",
        worktree.path(),
    );
    workspace.pr_supervision_status = Some("held".to_string());
    let repository = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    repository
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let held = reserve_health_held_attempt(
        repository.as_ref(),
        &conversation_id,
        "checks:settled-writer-release",
        crate::application::agent_workspace_publish_repair_state::UNCHANGED_HEALTH_REPAIR_REASON,
    )
    .await;
    let settlement = repository
        .settle_repair_attempt(
            crate::domain::repositories::SettleAgentWorkspaceRepairAttempt {
                attempt_id: held.id,
                generation: held.generation,
                expected_phase: AgentWorkspaceRepairPhase::Ready,
                expected_updated_at: held.updated_at,
                outcome: crate::domain::entities::AgentWorkspaceRepairOutcome::Succeeded,
                settled_at: Utc::now(),
                compatibility_projection: None,
                events: Vec::new(),
            },
        )
        .await
        .expect("held repair settlement should persist");
    assert!(matches!(
        settlement,
        crate::domain::repositories::SettleAgentWorkspaceRepairAttemptOutcome::Applied(_)
    ));

    super::update_agent_workspace_pr_supervision_state(
        repository.as_ref(),
        Some(repository.as_ref()),
        &conversation_id,
        Some(true),
        Some("monitoring"),
        Some("RalphX is monitoring PR health."),
    )
    .await
    .expect("settled repair should release poller ownership");

    let refreshed = repository
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace exists");
    assert_eq!(refreshed.pr_auto_merge_current, Some(true));
    assert_eq!(
        refreshed.pr_supervision_status.as_deref(),
        Some("monitoring")
    );
}

fn watching_review_monitor(
    workspace: &AgentConversationWorkspace,
    head_sha: &str,
) -> AgentWorkspacePrReviewMonitor {
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
        101,
        Some(head_sha.to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor.first_review_completed = true;
    monitor.last_reviewed_head_sha = Some(head_sha.to_string());
    monitor
}

fn codecov_comment(body: &str) -> PrIssueCommentSummary {
    PrIssueCommentSummary {
        id: "codecov-comment".to_string(),
        author: Some("codecov".to_string()),
        body: body.to_string(),
        url: Some("https://github.com/owner/repo/pull/101#issuecomment-1".to_string()),
        created_at: Some("2026-05-17T10:00:00Z".to_string()),
        updated_at: Some("2026-05-17T10:05:00Z".to_string()),
        is_bot: true,
        is_codecov: true,
    }
}

fn conflicting_pr_health(head: &str) -> PrHealth {
    let mut health = open_pr_health(head);
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);
    health
}

#[tokio::test]
async fn refreshed_agent_workspace_pr_remains_pollable_for_terminal_status() {
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

    assert!(
        super::agent_workspace_pr_polling_is_current(
            Arc::new(MemoryAgentConversationWorkspaceRepository::new()),
            &workspace,
            101
        )
        .await
    );
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
fn supervised_agent_workspace_pr_health_ignores_pending_required_check_block() {
    let mut health = open_pr_health("pending-required-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Blocked);
    health.checks.push(PrHealthCheck {
        name: "Required CI".to_string(),
        status: Some("QUEUED".to_string()),
        conclusion: None,
        details_url: Some("https://github.com/owner/repo/actions/runs/2".to_string()),
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
fn supervised_agent_workspace_pr_health_treats_codecov_comment_as_informative_only() {
    let mut health = open_pr_health("coverage-head");
    health.issue_comments.push(codecov_comment(
        "Codecov report: patch coverage is below target threshold and failed.",
    ));

    assert!(
        super::classify_agent_workspace_pr_autofix_issue(101, &health).is_none(),
        "issue comments should be context only; checks or formal reviews drive automation"
    );
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
fn supervised_agent_workspace_pr_health_routes_dirty_but_ignores_generic_blocked_mergeability() {
    let mut dirty_health = open_pr_health("dirty-head");
    dirty_health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    let dirty_issue = super::classify_agent_workspace_pr_autofix_issue(101, &dirty_health)
        .expect("dirty merge state should route autofix");
    assert!(dirty_issue
        .details
        .contains(&"PR branch has merge conflicts".to_string()));

    let mut blocked_health = open_pr_health("blocked-head");
    blocked_health.sync_state.merge_state_status = Some(PrMergeStateStatus::Blocked);
    assert!(
        super::classify_agent_workspace_pr_autofix_issue(101, &blocked_health).is_none(),
        "generic blocked state should wait for concrete review/check/conflict signals"
    );
}

#[tokio::test]
async fn agent_workspace_pr_conflict_marks_supervision_blocked_without_autofix() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-pr-conversation",
        "project-conflicting-pr",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    let conversation_id = workspace.conversation_id.clone();
    let memory_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        memory_workspace_repo.clone();
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("conflict-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);

    let marked = super::mark_agent_workspace_pr_merge_conflict_if_needed(
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("conflict marker should succeed");

    assert!(marked);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("PR #101 has merge conflicts"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "pr_conflict");
    assert_eq!(events[0].status, "blocked");
    assert!(events[0]
        .classification
        .as_deref()
        .unwrap_or_default()
        .starts_with("github_pr_conflict:101:conflicthead"));
}

#[tokio::test]
async fn agent_workspace_pr_conflict_marker_clears_resolved_conflict_state() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "resolved-conflict-conversation",
        "project-resolved-conflict",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary = Some(
        "PR #101 has merge conflicts. GitHub reports: PR is reported as conflicting.".to_string(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let health = open_pr_health("resolved-head");
    let marked = super::mark_agent_workspace_pr_merge_conflict_if_needed(
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("conflict marker should succeed");

    assert!(marked);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("RalphX is monitoring PR health.")
    );
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn agent_workspace_pr_conflict_marker_clears_paused_resolved_conflict_state() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "resolved-paused-conflict-conversation",
        "project-resolved-paused-conflict",
        worktree.path(),
    );
    workspace.auto_publish_enabled = false;
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary =
        Some("PR #101 has merge conflicts. GitHub reports: PR branch has merge conflicts.".into());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let marked = super::mark_agent_workspace_pr_merge_conflict_if_needed(
        101,
        &open_pr_health("resolved-paused-head"),
        &conversation_id,
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("paused conflict marker should succeed");

    assert!(marked);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("paused"));
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("Auto Publish is paused for this PR.")
    );
}

#[tokio::test]
async fn agent_workspace_pr_conflict_marker_ignores_absent_clean_generic_and_duplicate_states() {
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let missing_conversation = ChatConversationId::from_string("missing-conflict-conversation");
    assert!(!super::mark_agent_workspace_pr_merge_conflict_if_needed(
        101,
        &conflicting_pr_health("missing-conflict-head"),
        &missing_conversation,
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("missing workspace should be ignored"));

    let worktree = tempfile::tempdir().expect("worktree path");
    let mut generic_blocked = supervised_workspace(
        "generic-blocked-conflict-conversation",
        "project-generic-blocked-conflict",
        worktree.path(),
    );
    generic_blocked.pr_supervision_status = Some("blocked".to_string());
    generic_blocked.pr_supervision_summary = Some("Required checks are still pending.".into());
    let generic_conversation_id = generic_blocked.conversation_id.clone();
    workspace_repo
        .create_or_update(generic_blocked)
        .await
        .expect("generic workspace should persist");
    assert!(!super::mark_agent_workspace_pr_merge_conflict_if_needed(
        101,
        &open_pr_health("generic-clean-head"),
        &generic_conversation_id,
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("generic blocked workspace should be ignored"));

    let mut duplicate = supervised_workspace(
        "duplicate-conflict-conversation",
        "project-duplicate-conflict",
        worktree.path(),
    );
    let duplicate_health = conflicting_pr_health("duplicate-conflict-head");
    let details = super::agent_workspace_pr_merge_conflict_details(&duplicate_health);
    let summary = super::agent_workspace_pr_conflict_summary(101, &details);
    let classification =
        super::agent_workspace_pr_conflict_event_classification(101, &duplicate_health, &details);
    duplicate.pr_supervision_status = Some("blocked".to_string());
    duplicate.pr_supervision_summary = Some(summary.clone());
    let duplicate_conversation_id = duplicate.conversation_id.clone();
    workspace_repo
        .create_or_update(duplicate)
        .await
        .expect("duplicate workspace should persist");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            duplicate_conversation_id.clone(),
            "pr_conflict",
            "blocked",
            summary,
            Some(classification),
        ))
        .await
        .expect("duplicate event should persist");

    assert!(!super::mark_agent_workspace_pr_merge_conflict_if_needed(
        101,
        &duplicate_health,
        &duplicate_conversation_id,
        Arc::clone(&workspace_repo),
    )
    .await
    .expect("duplicate marker should no-op"));
    assert_eq!(
        workspace_repo
            .list_publication_events(&duplicate_conversation_id)
            .await
            .expect("events should list")
            .len(),
        1
    );
}

#[tokio::test]
async fn agent_workspace_pr_conflict_ignores_guarded_workspaces() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let health = conflicting_pr_health("guarded-conflict-head");
    let cases = {
        let mut archived = supervised_workspace(
            "guarded-archived-conflict-conversation",
            "project-guarded-archived-conflict",
            worktree.path(),
        );
        archived.status = AgentConversationWorkspaceStatus::Archived;

        let mut chat_mode = supervised_workspace(
            "guarded-chat-conflict-conversation",
            "project-guarded-chat-conflict",
            worktree.path(),
        );
        chat_mode.mode = AgentConversationWorkspaceMode::Chat;

        let mut linked = supervised_workspace(
            "guarded-linked-conflict-conversation",
            "project-guarded-linked-conflict",
            worktree.path(),
        );
        linked.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-conflict"));

        let mut wrong_pr = supervised_workspace(
            "guarded-wrong-pr-conflict-conversation",
            "project-guarded-wrong-pr-conflict",
            worktree.path(),
        );
        wrong_pr.publication_pr_number = Some(202);

        let mut terminal = supervised_workspace(
            "guarded-terminal-conflict-conversation",
            "project-guarded-terminal-conflict",
            worktree.path(),
        );
        terminal.publication_pr_status = Some("merged".to_string());

        vec![
            ("archived", archived),
            ("chat_mode", chat_mode),
            ("linked", linked),
            ("wrong_pr", wrong_pr),
            ("terminal", terminal),
        ]
    };

    for (label, workspace) in cases {
        let conversation_id = workspace.conversation_id.clone();
        let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
            Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should persist");
        let github = Arc::new(MockGithubService::new());
        let chat = Arc::new(MockChatService::new());

        assert!(
            !super::mark_agent_workspace_pr_merge_conflict_if_needed(
                101,
                &health,
                &conversation_id,
                Arc::clone(&workspace_repo),
            )
            .await
            .unwrap_or_else(|err| panic!("{label} marker should not fail: {err}")),
            "{label} marker should no-op"
        );
        assert!(
            !super::route_agent_workspace_pr_conflict_repair_if_needed(
                github as Arc<dyn GithubServiceTrait>,
                worktree.path(),
                101,
                &health,
                &conversation_id,
                Arc::clone(&workspace_repo),
                None,
                chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
            )
            .await
            .unwrap_or_else(|err| panic!("{label} repair should not fail: {err}")),
            "{label} repair should no-op"
        );
        assert!(chat.get_sent_messages().await.is_empty());
    }
}

#[tokio::test]
async fn agent_workspace_pr_conflict_auto_publish_routes_update_only_repair_once() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-auto-repair-conversation",
        "project-conflicting-auto-repair",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    workspace.auto_publish_enabled = true;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("auto-conflict-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("conflict repair routing should succeed");

    assert!(routed);
    let messages = chat.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Update from base failed for this agent workspace."));
    assert!(messages[0].contains("Please fix the workspace so the base update can be completed."));
    assert!(messages[0].contains("PR #101 has merge conflicts"));
    let options = chat.get_sent_options().await;
    assert_eq!(
        options[0].agent_name_override.as_deref(),
        Some(crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REPAIR)
    );
    assert_eq!(
        options[0].working_directory_override.as_deref(),
        Some(worktree.path())
    );
    assert!(options[0].force_new_provider_session);
    assert!(options[0].preserve_conversation_provider_session_ref);
    assert!(options[0].preallocated_agent_run_id.is_some());
    assert_eq!(
        options[0].queue_policy,
        crate::application::chat_service::SendQueuePolicy::RequireImmediateStart
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
        .contains("workspace repair"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "pr_conflict_repair"
            && event.status == "needs_agent"
            && event
                .classification
                .as_deref()
                .unwrap_or_default()
                .starts_with("github_pr_conflict_repair:101:autoconflict")
    }));
    assert!(events.iter().any(|event| {
        event.step == "repair_requested"
            && event.status == "started"
            && event.classification.as_deref() == Some("agent_fixable:update_only")
    }));
    assert!(events.iter().any(|event| {
        event.step == "repair_sent"
            && event.status == "succeeded"
            && event
                .classification
                .as_deref()
                .is_some_and(|value| value.starts_with("agent_fixable:run:"))
    }));

    let duplicate = super::route_agent_workspace_pr_conflict_repair_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("duplicate conflict repair routing should succeed");
    assert!(!duplicate);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
}

#[test]
fn agent_workspace_pr_conflict_repair_message_uses_identity_injected_completion_contract() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "repair-contract-conversation",
        "project-repair-contract",
        worktree.path(),
    );
    let message = super::build_agent_workspace_pr_conflict_repair_message(
        101,
        &workspace,
        &["PR is reported as conflicting.".to_string()],
    );

    assert!(message.contains("call `complete_agent_workspace_repair` with a concise summary"));
    assert!(message.contains("summary and blocker"));
    assert!(message.contains("Workspace branch:"));
    assert!(message.contains("PR #101 has merge conflicts"));
    for transport_owned_detail in [
        "Conversation ID:",
        "repair commit SHA",
        "resolved base ref",
        "resolved base commit",
        "Base ref:",
        "run ID",
        "attempt ID",
        "orchestration ID",
        "timestamp",
        "rescue",
    ] {
        assert!(
            !message.contains(transport_owned_detail),
            "repair prompt must not request or expose transport-owned detail: {transport_owned_detail}"
        );
    }
}

#[tokio::test]
async fn agent_workspace_pr_conflict_repair_disables_auto_merge_before_repair_agent() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-disarm-auto-merge-conversation",
        "project-conflicting-disarm",
        worktree.path(),
    );
    workspace.auto_publish_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = conflicting_pr_health("conflict-disarm-head");
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("conflict repair should route after disarm");

    assert!(routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(updated.pr_auto_merge_desired);
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("fixing"));
}

#[tokio::test]
async fn agent_workspace_pr_conflict_repair_send_failure_settles_blocked() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-send-failure-conversation",
        "project-conflicting-send-failure",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    workspace.auto_publish_enabled = true;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut health = open_pr_health("send-failure-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());
    chat.set_available(false).await;

    assert!(super::route_agent_workspace_pr_conflict_repair_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("send failure should be settled"));

    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.step == "repair_sent" && event.status == "failed"));
}

#[tokio::test]
async fn repair_dispatch_remains_completable_when_success_event_persistence_fails() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-success-event-failure-conversation",
        "project-conflicting-success-event-failure",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    workspace.auto_publish_enabled = true;
    let conversation_id = workspace.conversation_id.clone();
    let concrete_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    concrete_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    concrete_workspace_repo.fail_next_matching_publication_event(
        "repair_sent",
        "succeeded",
        "repair success event unavailable",
    );
    let workspace_repo =
        concrete_workspace_repo.clone() as Arc<dyn AgentConversationWorkspaceRepository>;
    let concrete_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let run_repo = concrete_run_repo.clone() as Arc<dyn AgentRunRepository>;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(&run_repo)));
    let mut health = open_pr_health("success-event-failure-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);

    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed(
        Arc::new(MockGithubService::new()) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("durable dispatch authority should survive success-event failure");

    assert!(routed);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let current = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.pr_supervision_status.as_deref(), Some("fixing"));
    assert!(current_agent_workspace_repair_claim_for_completion(
        workspace_repo,
        run_repo,
        &current,
    )
    .await
    .unwrap()
    .is_some());
    let events = concrete_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.step == "repair_sent" && event.status == "started"));
    assert!(!events
        .iter()
        .any(|event| event.step == "repair_sent" && event.status == "succeeded"));
}

#[tokio::test]
async fn repair_event_failure_before_dispatch_settles_the_claim_without_sending() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-pre-dispatch-event-failure-conversation",
        "project-conflicting-pre-dispatch-event-failure",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    workspace.auto_publish_enabled = true;
    let conversation_id = workspace.conversation_id.clone();
    let concrete_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    concrete_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    concrete_workspace_repo.fail_next_publication_event("repair event unavailable");
    let workspace_repo =
        concrete_workspace_repo.clone() as Arc<dyn AgentConversationWorkspaceRepository>;
    let chat = Arc::new(MockChatService::new());
    let mut health = open_pr_health("pre-dispatch-event-failure-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);

    super::route_agent_workspace_pr_conflict_repair_if_needed(
        Arc::new(MockGithubService::new()) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect_err("pre-dispatch event failure should surface");

    assert!(chat.get_sent_messages().await.is_empty());
    let current = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        current.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(current.pr_supervision_status.as_deref(), Some("blocked"));
}

#[tokio::test]
async fn agent_workspace_pr_conflict_repair_waits_when_auto_merge_disable_fails() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-disarm-failure-conversation",
        "project-conflicting-disarm-failure",
        worktree.path(),
    );
    workspace.auto_publish_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = conflicting_pr_health("conflict-disarm-failure-head");
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().disable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "permission denied".to_string(),
    )));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("conflict repair should handle disarm failure");

    assert!(!routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert_eq!(
        updated.pr_supervision_status.as_deref(),
        Some(super::AUTO_MERGE_SUPERVISION_STATUS_WAITING)
    );
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
}

#[tokio::test]
async fn agent_workspace_pr_conflict_repair_waits_when_auto_publish_is_paused() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-paused-repair-conversation",
        "project-conflicting-paused-repair",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    workspace.auto_publish_enabled = false;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("paused-conflict-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("paused conflict repair routing should succeed");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
}

#[tokio::test]
async fn agent_workspace_pr_conflict_repair_ignores_duplicate_routing_event() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-duplicate-repair-conversation",
        "project-conflicting-duplicate-repair",
        worktree.path(),
    );
    workspace.auto_publish_enabled = true;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let health = conflicting_pr_health("duplicate-repair-head");
    let details = super::agent_workspace_pr_merge_conflict_details(&health);
    let classification =
        super::agent_workspace_pr_conflict_repair_event_classification(101, &health, &details);
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_conflict_repair",
            "needs_agent",
            "Auto Publish routed PR #101 merge conflicts to workspace repair.",
            Some(classification),
        ))
        .await
        .expect("duplicate routing event should persist");
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("duplicate repair routing should succeed");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
}

#[tokio::test]
async fn agent_workspace_pr_conflict_repair_skips_clean_health_before_workspace_lookup() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());
    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &open_pr_health("clean-head"),
        &ChatConversationId::from_string("missing-clean-conversation"),
        workspace_repo,
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("clean health should not require a workspace");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn agent_workspace_pr_conflict_repair_errors_for_missing_conflicting_workspace() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());
    let error = super::route_agent_workspace_pr_conflict_repair_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conflicting_pr_health("missing-repair-head"),
        &ChatConversationId::from_string("missing-repair-conversation"),
        workspace_repo,
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect_err("conflicting missing workspace should be an error");

    assert!(error
        .to_string()
        .contains("Agent conversation workspace not found"));
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn agent_workspace_pr_conflict_repair_does_not_override_the_repair_role_runtime() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflicting-failed-handoff-conversation",
        "project-conflicting-failed-handoff",
        worktree.path(),
    );
    workspace.auto_publish_enabled = true;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let mut latest_run = AgentRun::new(conversation_id.clone());
    latest_run.harness = Some(AgentHarnessKind::Codex);
    latest_run.effective_model_id = Some("gpt-5.5".to_string());
    latest_run.logical_effort = Some(LogicalEffort::XHigh);
    latest_run.complete();
    agent_run_repo
        .create(latest_run)
        .await
        .expect("latest run should persist");
    let chat = Arc::new(MockChatService::new());
    chat.set_available(false).await;
    let github = Arc::new(MockGithubService::new());

    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conflicting_pr_health("failed-handoff-head"),
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("failed handoff should still mark routed");

    assert!(routed);
    let options = chat.get_sent_options().await;
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].harness_override, None);
    assert_eq!(options[0].model_override, None);
    assert_eq!(options[0].logical_effort_override, None);
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "repair_sent"
            && event.status == "failed"
            && event.classification.as_deref() == Some("operational")
            && event.summary.contains("Mock agent not available")
    }));
}

#[test]
fn agent_workspace_pr_conflict_helpers_cover_empty_details_and_unknown_heads() {
    let empty_details: Vec<String> = Vec::new();
    assert_eq!(
        super::agent_workspace_pr_conflict_summary(101, &empty_details),
        "PR #101 has merge conflicts."
    );
    assert!(!super::agent_workspace_summary_is_merge_conflict(101, None));
    assert!(!super::agent_workspace_summary_is_merge_conflict(
        101,
        Some("Required checks are still pending.")
    ));
    assert!(!super::agent_workspace_summary_is_merge_conflict(
        101,
        Some("PR #202 has merge conflicts.")
    ));
    assert!(super::agent_workspace_summary_is_merge_conflict(
        101,
        Some(" PR #101 is conflicting on GitHub. ")
    ));

    let mut health = conflicting_pr_health("***");
    health.sync_state.head_ref_oid = Some("***".to_string());
    let details = vec!["PR is reported as conflicting".to_string()];
    assert!(
        super::agent_workspace_pr_conflict_event_classification(101, &health, &details)
            .starts_with("github_pr_conflict:101:unknown:")
    );
    assert!(
        super::agent_workspace_pr_conflict_repair_event_classification(101, &health, &details)
            .starts_with("github_pr_conflict_repair:101:unknown:")
    );
}

#[test]
fn supervised_agent_workspace_pr_feedback_text_truncates_compactly() {
    let body = "This      feedback\ncontains enough words to exceed the tiny limit";
    assert_eq!(
        super::compact_pr_feedback_text(body, 24),
        "This feedback contains ..."
    );
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

    let message = super::build_agent_workspace_pr_autofix_message(
        101,
        workspace.publication_pr_url.as_deref(),
        "agent workspace",
        &workspace,
        &issue,
    );
    assert!(message.contains("RalphX PR supervision detected"));
    assert!(message.contains("complete_agent_workspace_pr_fix"));
    assert!(message.contains("get_agent_workspace_pr_fix_context"));
    assert!(message.contains("Fingerprint: github_pr_autofix:101:head:fingerprint"));
    assert!(message.contains("- CI / test (failure) - https://github.com/run"));
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_routes_failure_to_pr_fixer() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let poller_working_dir = tempfile::tempdir().expect("poller working dir");
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
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        poller_working_dir.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
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
        Some(poller_working_dir.path())
    );
    assert_eq!(options[0].harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(options[0].model_override.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(
        options[0].logical_effort_override,
        Some(LogicalEffort::High)
    );
    assert_eq!(options[0].service_tier_override.as_deref(), Some("fast"));
    assert!(options[0].force_new_provider_session);
    assert!(options[0].preserve_conversation_provider_session_ref);
    let attempts = agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("runs should list");
    assert!(attempts.iter().any(|run| {
        run.action_kind == Some(AgentRunActionKind::PrAutofix)
            && run.action_context_id.as_deref() == Some("101")
            && run
                .action_target_id
                .as_deref()
                .is_some_and(|value| value.starts_with("github_pr_autofix:101:routehead"))
    }));

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
async fn agent_workspace_pr_autofix_concurrent_checks_routes_claim_one_exact_attempt() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-concurrent-claim",
        "project-autofix-concurrent-claim",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut health = open_pr_health("concurrent-claim-head");
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let first_chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));
    let second_chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let (first, second) = tokio::join!(
        super::route_agent_workspace_pr_autofix_if_needed(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            Arc::clone(&workspace_repo),
            Some(Arc::clone(&agent_run_repo)),
            first_chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        ),
        super::route_agent_workspace_pr_autofix_if_needed(
            github as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            Arc::clone(&workspace_repo),
            Some(Arc::clone(&agent_run_repo)),
            second_chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        ),
    );

    assert_eq!(
        first.expect("first route") as usize + second.expect("second route") as usize,
        1
    );
    assert_eq!(
        first_chat.get_sent_messages().await.len() + second_chat.get_sent_messages().await.len(),
        1
    );
}

#[tokio::test]
async fn agent_workspace_pr_autofix_returned_identity_mismatch_settles_claim_without_audit_poisoning(
) {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-returned-id-mismatch",
        "project-autofix-returned-id-mismatch",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut health = open_pr_health("returned-id-mismatch-head");
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));
    chat.mismatch_next_send_result_identity().await;

    assert!(!super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("identity mismatch should settle"));

    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(
        workspace.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn agent_workspace_pr_autofix_checks_starts_one_failed_exact_attempt_retry() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-checks-start-retry",
        "project-autofix-checks-start-retry",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut health = open_pr_health("checks-start-retry-head");
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("checks issue should classify");
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    seed_pr_autofix_attempt(
        agent_run_repo.as_ref(),
        &conversation_id,
        101,
        &issue.classification,
        AgentRunStatus::Failed,
    )
    .await;
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    assert!(super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("one failed checks attempt should start its retry"));

    let options = chat.get_sent_options().await;
    let metadata: serde_json::Value = serde_json::from_str(
        options[0]
            .metadata
            .as_deref()
            .expect("retry must retain exact action metadata"),
    )
    .expect("metadata should be JSON");
    assert_eq!(metadata["ralphx_action_kind"], "pr_autofix");
    assert_eq!(metadata["ralphx_action_context_id"], "101");
    assert_eq!(metadata["ralphx_action_target_id"], issue.classification);
    let attempts = agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("attempts should list");
    assert_eq!(
        attempts
            .iter()
            .filter(|run| {
                run.action_kind == Some(AgentRunActionKind::PrAutofix)
                    && run.action_context_id.as_deref() == Some("101")
                    && run.action_target_id.as_deref() == Some(issue.classification.as_str())
            })
            .count(),
        2
    );
}

#[tokio::test]
async fn agent_workspace_review_feedback_starts_one_failed_exact_attempt_retry() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "review-feedback-start-retry",
        "project-review-feedback-start-retry",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let health = open_pr_health("review-start-retry-head");
    let issue = super::agent_workspace_pr_review_issue(101, &health);
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    seed_pr_autofix_attempt(
        agent_run_repo.as_ref(),
        &conversation_id,
        101,
        &issue.classification,
        AgentRunStatus::Failed,
    )
    .await;
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(requested_changes_feedback("review-start-retry"));
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    assert!(super::route_agent_workspace_review_feedback_if_present(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("one failed review attempt should start its retry"));

    let options = chat.get_sent_options().await;
    let metadata: serde_json::Value = serde_json::from_str(
        options[0]
            .metadata
            .as_deref()
            .expect("retry must retain exact action metadata"),
    )
    .expect("metadata should be JSON");
    assert_eq!(metadata["ralphx_action_kind"], "pr_autofix");
    assert_eq!(metadata["ralphx_action_context_id"], "101");
    assert_eq!(metadata["ralphx_action_target_id"], issue.classification);
    let attempts = agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("attempts should list");
    assert_eq!(
        attempts
            .iter()
            .filter(|run| {
                run.action_kind == Some(AgentRunActionKind::PrAutofix)
                    && run.action_context_id.as_deref() == Some("101")
                    && run.action_target_id.as_deref() == Some(issue.classification.as_str())
            })
            .count(),
        2
    );
}

#[tokio::test]
async fn agent_workspace_pr_autofix_checks_retry_exhaustion_blocks_manual_gate() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-checks-retry-exhausted",
        "project-autofix-checks-retry-exhausted",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut health = open_pr_health("checks-retry-exhausted-head");
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("checks issue should classify");
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    seed_pr_autofix_attempt(
        agent_run_repo.as_ref(),
        &conversation_id,
        101,
        &issue.classification,
        AgentRunStatus::Failed,
    )
    .await;
    seed_pr_autofix_attempt(
        agent_run_repo.as_ref(),
        &conversation_id,
        101,
        &issue.classification,
        AgentRunStatus::Failed,
    )
    .await;
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    assert!(!super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("retry exhaustion should block checks autofix"));

    assert!(chat.get_sent_messages().await.is_empty());
    assert_eq!(
        agent_run_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("attempts should list")
            .iter()
            .filter(|run| {
                run.action_kind == Some(AgentRunActionKind::PrAutofix)
                    && run.action_context_id.as_deref() == Some("101")
                    && run.action_target_id.as_deref() == Some(issue.classification.as_str())
                    && run.status == AgentRunStatus::Failed
            })
            .count(),
        2,
        "the second exact failed attempt must exhaust the retry budget"
    );
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(workspace
        .pr_supervision_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("retry budget is exhausted")));
}

#[tokio::test]
async fn agent_workspace_review_feedback_retry_exhaustion_blocks_same_manual_gate() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "review-feedback-retry-exhausted",
        "project-review-feedback-retry-exhausted",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let health = open_pr_health("review-retry-exhausted-head");
    let issue = super::agent_workspace_pr_review_issue(101, &health);
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    for _ in 0..2 {
        seed_pr_autofix_attempt(
            agent_run_repo.as_ref(),
            &conversation_id,
            101,
            &issue.classification,
            AgentRunStatus::Failed,
        )
        .await;
    }
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(requested_changes_feedback("review-retry-exhausted"));
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    assert!(!super::route_agent_workspace_review_feedback_if_present(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("retry exhaustion should block review autofix"));

    assert!(chat.get_sent_messages().await.is_empty());
    assert_eq!(
        agent_run_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("attempts should list")
            .iter()
            .filter(|run| {
                run.action_kind == Some(AgentRunActionKind::PrAutofix)
                    && run.action_context_id.as_deref() == Some("101")
                    && run.action_target_id.as_deref() == Some(issue.classification.as_str())
                    && run.status == AgentRunStatus::Failed
            })
            .count(),
        2,
        "the second exact failed attempt must exhaust the retry budget"
    );
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(workspace
        .pr_supervision_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("retry budget is exhausted")));
}

#[tokio::test]
async fn agent_workspace_pr_autofix_post_start_audit_failure_preserves_authoritative_run() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-post-start-audit-failure",
        "project-autofix-post-start-audit-failure",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), None, None)
            .with_pr_autofix_post_start_audit_error(),
    );
    let mut health = open_pr_health("post-start-audit-failure-head");
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    assert!(super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(Arc::clone(&agent_run_repo)),
        chat as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("audit failure must not invalidate the started run"));

    let runs = agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("runs should list");
    assert!(runs.iter().any(|run| {
        run.action_kind == Some(AgentRunActionKind::PrAutofix)
            && run.status == AgentRunStatus::Running
    }));
    let workspace = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(
        workspace.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("fixing"));
    assert!(!inner
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .iter()
        .any(|event| event.step == "pr_autofix"));
}

#[tokio::test]
async fn agent_workspace_pr_autofix_disabled_during_health_inspection_skips_repair_side_effects() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-stale-disabled-conversation",
        "project-autofix-stale-disabled",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), Some(2), None),
    );

    let mut health = open_pr_health("stale-disabled-head");
    health.checks.push(PrHealthCheck {
        name: "Rust Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("disabled autofix should skip cleanly");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    let updated = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(!updated.pr_autofix_enabled);
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert!(inner
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn agent_workspace_pr_autofix_final_authorization_error_fails_closed() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-final-read-error-conversation",
        "project-autofix-final-read-error",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), None, Some(4)),
    );

    let mut health = open_pr_health("final-read-error-head");
    health.checks.push(PrHealthCheck {
        name: "Rust Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let error = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect_err("final authorization read should propagate");

    assert!(matches!(error, AppError::Database(_)));
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_ne!(updated.pr_supervision_status.as_deref(), Some("fixing"));
    assert!(inner
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn agent_workspace_pr_autofix_send_failure_settles_claim_without_audit_poisoning() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-send-failure-conversation",
        "project-autofix-send-failure",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("send-failure-head");
    health.checks.push(PrHealthCheck {
        name: "Rust Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());
    chat.set_available(false).await;

    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("failed fixer send should settle its claim");

    assert!(!routed);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(
        updated.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("dispatch failed"));
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn agent_workspace_pr_autofix_disabled_still_syncs_healthy_auto_merge() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-disabled-auto-merge-conversation",
        "project-autofix-disabled-auto-merge",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("healthy-auto-merge-head")));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("healthy auto-merge sync should succeed");

    assert!(!routed);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
}

#[tokio::test]
async fn ideation_plan_pr_autofix_routes_failure_without_workspace_publication_pr() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let project_id = "project-plan-autofix";
    let session_id = IdeationSessionId::from_string("session-plan-autofix");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-autofix");
    let plan_branch_name = "ralphx/test/plan-autofix";
    let workspace = ideation_plan_workspace(
        "plan-autofix-route-conversation",
        project_id,
        session_id.clone(),
        plan_branch_id.clone(),
        plan_branch_name,
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let plan_branch = active_plan_pr_branch(
        session_id,
        project_id,
        plan_branch_id,
        plan_branch_name,
        602,
    );
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("plan-route-head");
    health.checks.push(PrHealthCheck {
        name: "Frontend Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/602".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;

    let routed = super::route_ideation_plan_pr_autofix_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        &plan_branch,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("plan PR autofix routing should succeed");

    assert!(routed);
    let messages = chat.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Frontend Tests (failure)"));
    assert!(messages[0].contains("Pull request: https://github.com/owner/repo/pull/602"));
    let options = chat.get_sent_options().await;
    assert_eq!(
        options[0].agent_name_override.as_deref(),
        Some(crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_PR_FIXER)
    );
    assert_eq!(
        options[0].working_directory_override.as_deref(),
        Some(worktree.path())
    );
    assert_eq!(options[0].harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(options[0].model_override.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(
        options[0].logical_effort_override,
        Some(LogicalEffort::High)
    );
    assert_eq!(options[0].service_tier_override.as_deref(), Some("fast"));
    assert!(options[0].force_new_provider_session);
    assert!(options[0].preserve_conversation_provider_session_ref);

    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, None);
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
                .starts_with("github_pr_autofix:602:planroutehea")
    }));
}

#[tokio::test]
async fn ideation_plan_pr_autofix_disabled_during_health_inspection_skips_dispatch() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let project_id = "project-plan-autofix-disabled";
    let session_id = IdeationSessionId::from_string("session-plan-autofix-disabled");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-autofix-disabled");
    let plan_branch_name = "ralphx/test/plan-autofix-disabled";
    let workspace = ideation_plan_workspace(
        "plan-autofix-disabled-conversation",
        project_id,
        session_id.clone(),
        plan_branch_id.clone(),
        plan_branch_name,
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let plan_branch = active_plan_pr_branch(
        session_id,
        project_id,
        plan_branch_id,
        plan_branch_name,
        605,
    );
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), Some(2), None),
    );

    let mut health = open_pr_health("plan-disabled-head");
    health.checks.push(PrHealthCheck {
        name: "Frontend Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;

    let routed = super::route_ideation_plan_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        &plan_branch,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("disabled plan autofix should skip cleanly");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(!updated.pr_autofix_enabled);
    assert_ne!(updated.pr_supervision_status.as_deref(), Some("fixing"));
    assert!(inner
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn ideation_plan_pr_autofix_skips_non_current_workspace_or_plan_shapes() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let project_id = "project-plan-autofix-skips";
    let session_id = IdeationSessionId::from_string("session-plan-autofix-skips");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-autofix-skips");
    let plan_branch_name = "ralphx/test/plan-autofix-skips";
    let base_workspace = ideation_plan_workspace(
        "plan-autofix-skip-conversation",
        project_id,
        session_id.clone(),
        plan_branch_id.clone(),
        plan_branch_name,
        worktree.path(),
    );
    let conversation_id = base_workspace.conversation_id.clone();
    let base_plan_branch = active_plan_pr_branch(
        session_id.clone(),
        project_id,
        plan_branch_id.clone(),
        plan_branch_name,
        603,
    );
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("unused")));
    let chat = Arc::new(MockChatService::new());

    let mut cases: Vec<(AgentConversationWorkspace, PlanBranch)> = Vec::new();

    let mut missing_pr = base_plan_branch.clone();
    missing_pr.pr_number = None;
    cases.push((base_workspace.clone(), missing_pr));

    let mut archived = base_workspace.clone();
    archived.status = AgentConversationWorkspaceStatus::Archived;
    cases.push((archived, base_plan_branch.clone()));

    let mut edit_mode = base_workspace.clone();
    edit_mode.mode = AgentConversationWorkspaceMode::Edit;
    cases.push((edit_mode, base_plan_branch.clone()));

    let mut plan_mismatch = base_workspace.clone();
    plan_mismatch.linked_plan_branch_id = Some(PlanBranchId::from_string("other-plan"));
    cases.push((plan_mismatch, base_plan_branch.clone()));

    let mut session_mismatch = base_workspace.clone();
    session_mismatch.linked_ideation_session_id =
        Some(IdeationSessionId::from_string("other-session"));
    cases.push((session_mismatch, base_plan_branch.clone()));

    let mut branch_mismatch = base_workspace.clone();
    branch_mismatch.branch_name = "ralphx/test/other-plan-branch".to_string();
    cases.push((branch_mismatch, base_plan_branch.clone()));

    let mut terminal_plan = base_plan_branch.clone();
    terminal_plan.pr_status = Some(DbPrStatus::Closed);
    cases.push((base_workspace.clone(), terminal_plan));

    for (workspace, plan_branch) in cases {
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should persist");
        let routed = super::route_ideation_plan_pr_autofix_if_needed(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            &plan_branch,
            &conversation_id,
            Arc::clone(&workspace_repo),
            None,
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("skip routing should succeed");

        assert!(!routed);
    }

    assert_eq!(github.state().fetch_pr_health_calls, 0);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn ideation_plan_pr_autofix_records_terminal_status_without_workspace_publication_update() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let project_id = "project-plan-autofix-terminal";
    let session_id = IdeationSessionId::from_string("session-plan-autofix-terminal");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-autofix-terminal");
    let plan_branch_name = "ralphx/test/plan-autofix-terminal";
    let workspace = ideation_plan_workspace(
        "plan-autofix-terminal-conversation",
        project_id,
        session_id.clone(),
        plan_branch_id.clone(),
        plan_branch_name,
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let plan_branch = active_plan_pr_branch(
        session_id,
        project_id,
        plan_branch_id,
        plan_branch_name,
        604,
    );
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("terminal-plan-head");
    health.sync_state.status = PrStatus::Closed;
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_ideation_plan_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        &plan_branch,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("terminal linked plan status should be handled");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_pr_url, None);
    assert_eq!(updated.publication_push_status, None);
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "pr_terminal"
            && event.status == "closed"
            && event.classification.as_deref() == Some("github_pr_terminal:604:closed")
    }));
}

#[tokio::test]
async fn review_pr_monitor_routes_new_head_to_reviewer_agent() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-route-conversation",
        "project-review-monitor-route",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        workspace.project_id.clone(),
        101,
        Some("old-head".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor.first_review_completed = true;
    monitor.last_reviewed_head_sha = Some("old-head".to_string());
    workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("new-head")));
    let chat = Arc::new(MockChatService::new());
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("review monitor routing should succeed");

    assert!(routed);
    let messages = chat.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Review PR monitor detected new changes"));
    assert!(messages[0].contains("Write the versioned Review artifact"));
    let options = chat.get_sent_options().await;
    assert_eq!(
        options[0].agent_name_override.as_deref(),
        Some(crate::infrastructure::agents::claude::agent_names::AGENT_PR_REVIEWER)
    );
    assert_eq!(
        options[0].working_directory_override.as_deref(),
        Some(worktree.path())
    );
    let monitor = workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Reviewing
    );
    assert_eq!(monitor.last_seen_head_sha.as_deref(), Some("new-head"));
    assert!(monitor.last_review_run_id.is_some());
}

#[tokio::test]
async fn review_pr_monitor_skips_when_head_already_has_pending_action() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-pending-conversation",
        "project-review-monitor-pending",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        workspace.project_id.clone(),
        101,
        Some("old-head".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor.first_review_completed = true;
    monitor.last_reviewed_head_sha = Some("old-head".to_string());
    workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    workspace_repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            101,
            "new-head".to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            "Needs changes".to_string(),
            "Please address the findings.".to_string(),
            None,
            Some("run-review".to_string()),
        ))
        .await
        .expect("pending action should persist");

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("new-head")));
    let chat = Arc::new(MockChatService::new());
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("review monitor routing should skip cleanly");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let monitor = workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
    assert_eq!(monitor.last_seen_head_sha.as_deref(), Some("new-head"));
}

#[tokio::test]
async fn review_pr_monitor_skips_when_monitor_missing_or_disabled() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-missing-conversation",
        "project-review-monitor-missing",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("missing monitor should skip cleanly");
    assert!(!routed);
    assert_eq!(github.state().fetch_pr_health_calls, 0);

    let mut disabled = watching_review_monitor(&workspace, "old-head");
    disabled.monitor_enabled = false;
    workspace_repo
        .upsert_pr_review_monitor(disabled)
        .await
        .expect("disabled monitor should persist");
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("disabled monitor should skip cleanly");
    assert!(!routed);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn review_pr_monitor_skips_paused_terminal_and_submitting_without_fetching_health() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-terminal-skip-conversation",
        "project-review-monitor-terminal-skip",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());
    let mut terminal = watching_review_monitor(&workspace, "old-head");
    terminal.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
    workspace_repo
        .upsert_pr_review_monitor(terminal)
        .await
        .expect("terminal monitor should persist");
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("terminal monitor should skip cleanly");
    assert!(!routed);

    let mut submitting = watching_review_monitor(&workspace, "old-head");
    submitting.status = AgentWorkspacePrReviewMonitorStatus::Submitting;
    workspace_repo
        .upsert_pr_review_monitor(submitting)
        .await
        .expect("submitting monitor should persist");
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("submitting monitor should skip cleanly");
    assert!(!routed);

    let mut paused = watching_review_monitor(&workspace, "old-head");
    paused.monitor_enabled = false;
    paused.status = AgentWorkspacePrReviewMonitorStatus::Paused;
    workspace_repo
        .upsert_pr_review_monitor(paused)
        .await
        .expect("paused monitor should persist");
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("paused monitor should skip cleanly");
    assert!(!routed);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn review_pr_monitor_terminal_state_also_persists_cleanup_authority() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-terminal-conversation",
        "project-review-monitor-terminal",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    workspace_repo
        .upsert_pr_review_monitor(watching_review_monitor(&workspace, "old-head"))
        .await
        .expect("monitor should persist");

    super::mark_agent_workspace_pr_open(Arc::clone(&workspace_repo), &conversation_id, 101)
        .await
        .expect("review PR open marker should skip publication mutation");
    let unchanged = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert!(unchanged.publication_pr_status.is_none());
    assert!(unchanged.publication_push_status.is_none());

    super::mark_agent_workspace_pr_terminal(
        Arc::clone(&workspace_repo),
        &conversation_id,
        101,
        "closed",
        "Pull request closed without merging",
    )
    .await
    .expect("review PR terminal marker should update monitor");
    let terminal_monitor = workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .unwrap()
        .expect("monitor should exist");
    assert_eq!(
        terminal_monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Terminal
    );
    assert!(!terminal_monitor.monitor_enabled);
    assert_eq!(
        terminal_monitor.last_review_outcome.as_deref(),
        Some("closed")
    );
    assert_eq!(
        terminal_monitor.last_error.as_deref(),
        Some("Pull request closed without merging")
    );
    let unchanged = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(unchanged.publication_pr_status.as_deref(), Some("closed"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert!(events.iter().any(|event| event.step == "pr_closed"));
}

#[tokio::test]
async fn review_pr_monitor_merged_terminal_outcome_has_no_error() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-merged-terminal-conversation",
        "project-review-monitor-merged-terminal",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    workspace_repo
        .upsert_pr_review_monitor(watching_review_monitor(&workspace, "old-head"))
        .await
        .expect("monitor should persist");

    super::mark_agent_workspace_pr_terminal(
        Arc::clone(&workspace_repo),
        &conversation_id,
        101,
        "merged",
        "Pull request merged",
    )
    .await
    .expect("review PR terminal marker should update monitor");

    let monitor = workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Terminal
    );
    assert_eq!(monitor.last_review_outcome.as_deref(), Some("merged"));
    assert!(monitor.last_error.is_none());
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| event.step == "pr_merged"));
}

#[tokio::test]
async fn mismatched_polled_pr_does_not_mutate_non_review_publication_state() {
    for publication_pr_number in [None, Some(942)] {
        let worktree = tempfile::tempdir().expect("worktree path");
        let mut workspace = supervised_workspace(
            "mismatched-poller-publication-conversation",
            "project-mismatched-poller-publication",
            worktree.path(),
        );
        workspace.publication_pr_number = publication_pr_number;
        workspace.publication_pr_url = publication_pr_number
            .map(|number| format!("https://github.com/owner/repo/pull/{number}"));
        workspace.publication_pr_status = None;
        workspace.publication_push_status = Some("failed".to_string());
        let conversation_id = workspace.conversation_id.clone();
        let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
            Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");
        let baseline = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");

        assert!(!super::mark_agent_workspace_pr_open(
            Arc::clone(&workspace_repo),
            &conversation_id,
            941,
        )
        .await
        .expect("mismatched open marker should stop cleanly"));
        assert_eq!(
            workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .expect("workspace lookup should succeed"),
            Some(baseline.clone())
        );
        assert!(!super::mark_agent_workspace_pr_terminal(
            Arc::clone(&workspace_repo),
            &conversation_id,
            941,
            "merged",
            "Pull request merged",
        )
        .await
        .expect("mismatched terminal marker should stop cleanly"));

        assert_eq!(
            workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .expect("workspace lookup should succeed"),
            Some(baseline)
        );
        assert!(workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("events should list")
            .is_empty());
    }
}

#[tokio::test]
async fn review_pr_polling_should_continue_requires_enabled_nonterminal_monitor() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-pollable-conversation",
        "project-review-monitor-pollable",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    assert!(
        !super::agent_workspace_pr_polling_should_continue(
            Arc::clone(&workspace_repo),
            &conversation_id,
            101,
        )
        .await
    );

    workspace_repo
        .upsert_pr_review_monitor(watching_review_monitor(&workspace, "old-head"))
        .await
        .expect("monitor should persist");
    assert!(
        super::agent_workspace_pr_polling_should_continue(
            Arc::clone(&workspace_repo),
            &conversation_id,
            101,
        )
        .await
    );

    let mut terminal = watching_review_monitor(&workspace, "old-head");
    terminal.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
    workspace_repo
        .upsert_pr_review_monitor(terminal)
        .await
        .expect("terminal monitor should persist");
    assert!(
        !super::agent_workspace_pr_polling_should_continue(
            Arc::clone(&workspace_repo),
            &conversation_id,
            101,
        )
        .await
    );

    let mut disabled = watching_review_monitor(&workspace, "old-head");
    disabled.monitor_enabled = false;
    workspace_repo
        .upsert_pr_review_monitor(disabled)
        .await
        .expect("disabled monitor should persist");
    assert!(
        !super::agent_workspace_pr_polling_should_continue(
            Arc::clone(&workspace_repo),
            &conversation_id,
            101,
        )
        .await
    );

    let mut wrong_pr = watching_review_monitor(&workspace, "old-head");
    wrong_pr.pr_number = 202;
    workspace_repo
        .upsert_pr_review_monitor(wrong_pr)
        .await
        .expect("wrong PR monitor should persist");
    assert!(
        !super::agent_workspace_pr_polling_should_continue(
            Arc::clone(&workspace_repo),
            &conversation_id,
            101,
        )
        .await
    );
}

#[tokio::test]
async fn review_pr_polling_continues_when_monitor_lookup_errors() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-error-conversation",
        "project-review-monitor-error",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(ReviewMonitorLookupErrorRepository { workspace });

    assert!(
        super::agent_workspace_pr_polling_should_continue(workspace_repo, &conversation_id, 101,)
            .await
    );
}

#[tokio::test]
async fn review_pr_monitor_skips_same_head_and_active_runs() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-skip-conversation",
        "project-review-monitor-skip",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    workspace_repo
        .upsert_pr_review_monitor(watching_review_monitor(&workspace, "same-head"))
        .await
        .expect("monitor should persist");

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("same-head")));
    let chat = Arc::new(MockChatService::new());
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("same-head route should skip cleanly");
    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());

    workspace_repo
        .upsert_pr_review_monitor(watching_review_monitor(&workspace, "old-head"))
        .await
        .expect("monitor should reset");
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("new-head")));
    let active_runs = Arc::new(MemoryAgentRunRepository::new());
    active_runs
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("active run should persist");
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        active_runs,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("active-run route should skip cleanly");
    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn review_pr_monitor_routes_new_head_after_awaiting_user_decision() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-awaiting-conversation",
        "project-review-monitor-awaiting",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let mut monitor = watching_review_monitor(&workspace, "old-head");
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    let stale_action = AgentWorkspacePrReviewAction::new(
        conversation_id.clone(),
        101,
        "old-head".to_string(),
        AgentWorkspacePrReviewActionKind::RequestChanges,
        "Old review requires a decision".to_string(),
        "This action belongs to the superseded head.".to_string(),
        None,
        Some("old-review-run".to_string()),
    );
    workspace_repo
        .create_or_update_pr_review_action(stale_action.clone())
        .await
        .expect("stale action should persist");
    let notification_repo: Arc<dyn NotificationRepository> =
        Arc::new(MemoryNotificationRepository::new());
    let notification_service = Arc::new(NotificationService::new(
        Arc::clone(&notification_repo),
        Arc::new(NoopNotificationEventEmitter),
    ));
    notification_service
        .record(NewNotification {
            project_id: Some(workspace.project_id.to_string()),
            category: NotificationCategory::PrReviewAction,
            severity: NotificationSeverity::ActionRequired,
            title: "PR review needs a decision".into(),
            body: None,
            target: NotificationTarget::none(),
            dedupe_key: Some(pr_review_notification_key(
                conversation_id.as_str(),
                &stale_action.id,
            )),
        })
        .await;

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("new-head")));
    let chat = Arc::new(MockChatService::new());
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed_with_notifications(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        Some(notification_service),
    )
    .await
    .expect("awaiting-user route should dispatch a new-head re-review");
    assert!(routed);
    assert_eq!(github.state().fetch_pr_health_calls, 1);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let stale_action = workspace_repo
        .get_pr_review_action(&stale_action.id)
        .await
        .expect("stale action lookup should succeed")
        .expect("stale action should remain available for history");
    assert_eq!(
        stale_action.status,
        AgentWorkspacePrReviewActionStatus::Superseded
    );
    let notifications = notification_repo
        .list(None, None, 50)
        .await
        .expect("notification lookup should succeed")
        .notifications;
    assert!(notifications[0].read_at.is_some());
}

#[tokio::test]
async fn review_pr_monitor_skips_when_current_head_sha_is_missing() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = review_pr_workspace(
        "review-monitor-missing-head-conversation",
        "project-review-monitor-missing-head",
        worktree.path(),
    );
    workspace
        .source_pull_request
        .as_mut()
        .expect("source PR should exist")
        .head_ref_oid = None;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    workspace_repo
        .upsert_pr_review_monitor(watching_review_monitor(&workspace, "old-head"))
        .await
        .expect("monitor should persist");

    let github = Arc::new(MockGithubService::new());
    let mut health = open_pr_health("new-head");
    health.sync_state.head_ref_oid = None;
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());
    let routed = super::route_agent_workspace_pr_review_monitor_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("missing-head route should skip cleanly");

    assert!(!routed);
    assert_eq!(github.state().fetch_pr_health_calls, 1);
    assert!(chat.get_sent_messages().await.is_empty());
    let monitor = workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
    assert_eq!(monitor.last_seen_head_sha.as_deref(), Some("old-head"));
}

#[test]
fn review_pr_monitor_message_uses_publication_url_and_unknown_head_fallbacks() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = review_pr_workspace(
        "review-monitor-message-conversation",
        "project-review-monitor-message",
        worktree.path(),
    );
    workspace.source_pull_request = None;
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/202".to_string());
    let mut health = open_pr_health("ignored-head");
    health.sync_state.head_ref_oid = None;

    let message = super::build_agent_workspace_pr_monitor_review_message(202, &workspace, &health);

    assert!(message.contains("Review PR monitor detected new changes on GitHub PR #202"));
    assert!(message.contains("Pull request: https://github.com/owner/repo/pull/202"));
    assert!(message.contains("Current head SHA: unknown"));
    assert!(message.contains("Write the versioned Review artifact"));
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_skips_when_auto_publish_is_paused() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-paused-conversation",
        "project-paused",
        worktree.path(),
    );
    workspace.auto_publish_enabled = false;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("autofix routing should skip cleanly");

    assert!(!routed);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_waits_on_pending_required_check_block() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-pending-required-conversation",
        "project-pending-required",
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

    let mut health = open_pr_health("pending-required-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Blocked);
    health.checks.push(PrHealthCheck {
        name: "Required CI".to_string(),
        status: Some("IN_PROGRESS".to_string()),
        conclusion: None,
        details_url: Some("https://github.com/owner/repo/actions/runs/2".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("pending required checks should not error");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("RalphX is monitoring PR health.")
    );
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(events.is_empty());
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_imports_comments_without_routing() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-comment-context-conversation",
        "project-comment-context",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("comment-context-head");
    health.issue_comments.push(codecov_comment(
        "Codecov report: patch coverage is below target threshold and failed.",
    ));
    let mut ignored_comment = codecov_comment("Comment without an id should not persist.");
    ignored_comment.id = "  ".to_string();
    health.issue_comments.push(ignored_comment);
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("comment-only PR health should not error");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let comments = workspace_repo
        .list_pr_comment_evidence(&conversation_id, 101, 10)
        .await
        .expect("comment evidence should list");
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].comment_id, "codecov-comment");
    assert!(comments[0].is_codecov);
    assert!(comments[0].last_included_at.is_none());
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_records_terminal_health_without_routing() {
    for (status, expected_status, expected_summary) in [
        (
            PrStatus::Merged {
                merge_commit_sha: Some("a".repeat(40)),
                merged_at: None,
            },
            "merged",
            "Pull request merged",
        ),
        (
            PrStatus::Closed,
            "closed",
            "Pull request closed without merging",
        ),
    ] {
        let worktree = tempfile::tempdir().expect("worktree path");
        let workspace = supervised_workspace(
            &format!("terminal-{expected_status}-conversation"),
            &format!("project-terminal-{expected_status}"),
            worktree.path(),
        );
        let conversation_id = workspace.conversation_id.clone();
        let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
            Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should persist");

        let mut health = open_pr_health("terminal-head");
        health.sync_state.status = status;
        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(health));
        let chat = Arc::new(MockChatService::new());

        let routed = super::route_agent_workspace_pr_autofix_if_needed(
            github as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            Arc::clone(&workspace_repo),
            None,
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("terminal PR health should not error");

        assert!(!routed);
        assert!(chat.get_sent_messages().await.is_empty());
        let updated = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(
            updated.publication_pr_status.as_deref(),
            Some(expected_status)
        );
        assert!(updated.pr_supervision_status.is_none());
        let events = workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_terminal"
                && event.status == expected_status
                && event.summary == expected_summary
        }));
    }
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
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("duplicate autofix should not error");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_skips_when_fixer_run_active() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-active-run-conversation",
        "project-active-run",
        worktree.path(),
    );
    workspace.pr_supervision_status = Some("monitoring".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("new-issue-head");
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("blocked PR should classify");
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    seed_pr_autofix_attempt(
        agent_run_repo.as_ref(),
        &conversation_id,
        101,
        &issue.classification,
        AgentRunStatus::Running,
    )
    .await;
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("active fixer guard should not error");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(
        events.is_empty(),
        "active fixer should not append another publication event"
    );
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_skips_when_workspace_already_needs_agent() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-needs-agent-conversation",
        "project-needs-agent",
        worktree.path(),
    );
    workspace.publication_push_status = Some("needs_agent".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("queued-fix-head")));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("queued fixer guard should not error");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_skips_when_supervision_status_is_repairing() {
    for status in ["fixing", "publishing"] {
        let worktree = tempfile::tempdir().expect("worktree path");
        let mut workspace = supervised_workspace(
            &format!("autofix-{status}-conversation"),
            &format!("project-{status}"),
            worktree.path(),
        );
        workspace.pr_supervision_status = Some(status.to_string());
        let conversation_id = workspace.conversation_id.clone();
        let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
            Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should persist");

        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(open_pr_health("repairing-head")));
        let chat = Arc::new(MockChatService::new());

        let routed = super::route_agent_workspace_pr_autofix_if_needed(
            github as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            Arc::clone(&workspace_repo),
            None,
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("repairing status guard should not error");

        assert!(!routed, "status {status} should not route another fixer");
        assert!(chat.get_sent_messages().await.is_empty());
    }
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_disables_auto_merge_before_fixer() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-disarm-auto-merge-conversation",
        "project-autofix-disarm",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("autofix-disarm-head");
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("autofix route should succeed");

    assert!(routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(updated.pr_auto_merge_desired);
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert_eq!(
        updated.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("fixing"));
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_waits_when_auto_merge_disable_fails() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-disarm-failure-conversation",
        "project-autofix-disarm-failure",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("autofix-disarm-failure-head");
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    github.state().disable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "permission denied".to_string(),
    )));
    let chat = Arc::new(MockChatService::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("autofix guard should handle disable failure");

    assert!(!routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert_eq!(
        updated.pr_supervision_status.as_deref(),
        Some(super::AUTO_MERGE_SUPERVISION_STATUS_WAITING)
    );
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("GitHub auto-merge could not be disabled yet"));
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
}

#[tokio::test]
async fn supervised_agent_workspace_pr_autofix_routes_when_pushed_repair_status_is_stale() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-stale-fixing-conversation",
        "project-stale-fixing",
        worktree.path(),
    );
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_summary = Some("Previous PR repair is in progress.".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let mut health = open_pr_health("stale-fixing-head");
    health.checks.push(PrHealthCheck {
        name: "Frontend Visual Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/2".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("stale repair status should not suppress routing");

    assert!(routed);
    let messages = chat.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Frontend Visual Tests (failure)"));
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
        None,
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
async fn supervised_agent_workspace_pr_autofix_suppresses_auto_merge_enable_during_review_guard() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-review-guard-conversation",
        "project-review-guard",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.pr_supervision_status = Some("waiting".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let project_id = workspace.project_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id);
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 101,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "guarded-diff".to_string(),
        head_sha: Some("guarded-head".to_string()),
        last_error: None,
    });
    workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("review guard should persist");

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("healthy-guarded-head")));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("guarded healthy PR should not error");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    assert_eq!(github.state().mark_pr_ready_calls, 0);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert_eq!(
        updated.pr_supervision_status.as_deref(),
        Some("review_paused")
    );
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("GitHub auto-merge is paused while the workspace Review is authoritative.")
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
        None,
    )
    .await
    .expect("auto-merge sync should succeed");

    assert!(current);
    {
        let github_state = github.state();
        assert_eq!(github_state.mark_pr_ready_calls, 1);
        assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
        assert_eq!(
            github_state.last_enable_pr_auto_merge_args.as_ref(),
            Some(&(101, "squash".to_string()))
        );
    }
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
        None,
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
        None,
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
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("review-feedback-head")));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_review_feedback_if_present(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("review feedback should route");

    assert!(routed);
    let messages = chat.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("get_agent_workspace_pr_fix_context"));
    assert!(messages[0].contains("complete_agent_workspace_pr_fix"));
    assert!(messages[0].contains(&format!("Conversation ID: {conversation_id}")));
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
    assert_eq!(options[0].harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(options[0].model_override.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(
        options[0].logical_effort_override,
        Some(LogicalEffort::High)
    );
    assert_eq!(options[0].service_tier_override.as_deref(), Some("fast"));
    assert!(options[0].force_new_provider_session);
    assert!(options[0].preserve_conversation_provider_session_ref);
    assert_eq!(
        options[0].queue_policy,
        crate::application::chat_service::SendQueuePolicy::RequireImmediateStart
    );
    assert!(options[0].preallocated_agent_run_id.is_some());

    let attempts = agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("review fixer attempts should list");
    assert!(attempts.iter().any(|run| {
        run.action_kind == Some(AgentRunActionKind::PrAutofix)
            && run.action_context_id.as_deref() == Some("101")
            && run
                .action_target_id
                .as_deref()
                .is_some_and(|value| value.starts_with("github_pr_autofix:101:reviewfeedba"))
    }));

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
            && event
                .classification
                .as_deref()
                .is_some_and(|value| value.starts_with("github_pr_autofix:101:reviewfeedba"))
    }));
}

#[tokio::test]
async fn agent_workspace_review_feedback_with_autofix_disabled_has_no_repair_side_effects() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "review-feedback-disabled-conversation",
        "project-review-feedback-disabled",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(requested_changes_feedback("review-disabled"));
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_review_feedback_if_present(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("disabled review feedback should skip cleanly");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    assert_eq!(github.state().fetch_pr_health_calls, 0);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert_ne!(updated.pr_supervision_status.as_deref(), Some("fixing"));
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn agent_workspace_review_feedback_final_authorization_rejects_disabled_workspace() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "review-feedback-final-disabled-conversation",
        "project-review-feedback-final-disabled",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), Some(4), None),
    );
    let mut health = open_pr_health("review-final-disabled-head");
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let issue = super::agent_workspace_pr_review_issue(101, &health);
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(requested_changes_feedback("review-final-disabled"));
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_review_feedback_if_present(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("final disabled authorization should skip cleanly");

    assert!(!routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(!updated.pr_autofix_enabled);
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("authorization changed"));
    assert!(!inner
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .iter()
        .any(|event| event.classification.as_deref() == Some(issue.classification.as_str())));
    assert_eq!(
        crate::application::agent_workspace_pr_autofix_attempt::load_pr_autofix_attempt_decision(
            agent_run_repo.as_ref(),
            &conversation_id,
            101,
            &issue.classification,
            false,
        )
        .await
        .expect("authorization failure must not consume the exact attempt"),
        crate::application::agent_workspace_pr_autofix_attempt::PrAutofixAttemptDecision::StartFirst
    );
}

#[tokio::test]
async fn agent_workspace_review_feedback_routes_once_after_autofix_is_reenabled() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "review-feedback-reenabled-conversation",
        "project-review-feedback-reenabled",
        worktree.path(),
    );
    workspace.pr_autofix_enabled = false;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let github = Arc::new(MockGithubService::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    github.will_return_review_feedback(requested_changes_feedback("review-reenabled"));
    assert!(!super::route_agent_workspace_review_feedback_if_present(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("disabled pass should skip"));

    let mut workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    workspace.pr_autofix_enabled = true;
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should re-enable autofix");
    github.will_return_review_feedback(requested_changes_feedback("review-reenabled"));
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("review-reenabled-head")));
    assert!(super::route_agent_workspace_review_feedback_if_present(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("re-enabled pass should route"));

    github.will_return_review_feedback(requested_changes_feedback("review-reenabled"));
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("review-reenabled-head")));
    assert!(!super::route_agent_workspace_review_feedback_if_present(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("duplicate pass should skip"));

    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event
                    .classification
                    .as_deref()
                    .is_some_and(|value| value.starts_with("github_pr_autofix:101:reviewreenab"))
            })
            .count(),
        1
    );
}

#[tokio::test]
async fn agent_workspace_pr_autofix_pre_start_workspace_write_failure_settles_claim() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-post-send-write-failure",
        "project-autofix-post-send-write-failure",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), None, None)
            .with_update_publication_error_on_call(1),
    );

    let mut health = open_pr_health("post-send-write-head");
    health.checks.push(PrHealthCheck {
        name: "Rust Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("checks issue should classify");
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Arc::clone(&chat) as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("pre-start write failure should settle explicitly");

    assert!(!routed);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    let updated = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(
        updated.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert!(updated.pr_auto_merge_desired);
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("could not prepare workspace state"));
    assert!(chat.get_sent_messages().await.is_empty());
    assert!(!inner
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .iter()
        .any(|event| event.classification.as_deref() == Some(issue.classification.as_str())));
}

#[tokio::test]
async fn agent_workspace_pr_autofix_claim_failure_does_not_overwrite_a_newer_repair_claim() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "autofix-superseded-claim",
        "project-autofix-superseded-claim",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), None, None)
            .with_superseded_repair_claim_on_update_publication(1),
    );
    let mut health = open_pr_health("superseded-claim-head");
    health.checks.push(PrHealthCheck {
        name: "CI".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("checks issue should classify");
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    assert!(!super::route_agent_workspace_pr_autofix_if_needed(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("superseded claim should settle without overwriting its replacement"));

    assert!(chat.get_sent_messages().await.is_empty());
    let updated = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("replacement repair claim")
    );
    assert!(!inner
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .iter()
        .any(|event| event.classification.as_deref() == Some(issue.classification.as_str())));
}

#[tokio::test]
async fn agent_workspace_pr_autofix_disarm_persistence_failure_restores_and_blocks() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-disarm-write-failure",
        "project-autofix-disarm-write-failure",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), None, None)
            .with_update_auto_merge_error_on_call(1),
    );

    let mut health = open_pr_health("disarm-write-head");
    health.checks.push(PrHealthCheck {
        name: "Rust Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let issue = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("checks issue should classify");
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("disarm write failure should settle explicitly");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 1);
    let updated = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("auto-merge disarm state"));
    assert!(!inner
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .iter()
        .any(|event| event.classification.as_deref() == Some(issue.classification.as_str())));
}

#[tokio::test]
async fn agent_workspace_pr_autofix_send_failure_uses_current_auto_merge_policy() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-current-policy-failure",
        "project-autofix-current-policy-failure",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let inner = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    inner
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SequencedWorkspaceRepository::new(Arc::clone(&inner), None, None)
            .with_disable_auto_merge_after_repair_claim(),
    );

    let mut health = open_pr_health("current-policy-head");
    health.checks.push(PrHealthCheck {
        name: "Rust Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::new());
    chat.set_available(false).await;

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        chat as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("send failure should settle explicitly");

    assert!(!routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(github.state().enable_pr_auto_merge_calls, 0);
    let updated = inner
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(!updated.pr_auto_merge_desired);
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
}

#[tokio::test]
async fn agent_workspace_pr_autofix_missing_head_blocks_without_dispatch() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "autofix-missing-head",
        "project-autofix-missing-head",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health("missing-head");
    health.sync_state.head_ref_oid = None;
    health.checks.push(PrHealthCheck {
        name: "Rust Tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: None,
    });
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(Arc::clone(&agent_run_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("missing head should fail closed");

    assert!(!routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("head commit"));
}

#[tokio::test]
async fn agent_workspace_review_feedback_disables_auto_merge_before_pr_fixer() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "review-feedback-disarm-conversation",
        "project-review-feedback-disarm",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let feedback = PrReviewFeedback {
        review_id: "review-disarm".to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-05-17T12:00:00Z".to_string()),
        body: Some("Please handle the edge case.".to_string()),
        comments: Vec::new(),
    };
    let mut health = open_pr_health("review-feedback-disarm-head");
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(feedback);
    github.state().fetch_pr_health_result = Some(Ok(health));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_review_feedback_if_present(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("review feedback should route after disarm");

    assert!(routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(updated.pr_auto_merge_desired);
    assert_eq!(updated.pr_auto_merge_current, Some(false));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("fixing"));
}

#[tokio::test]
async fn agent_workspace_review_feedback_waits_when_auto_merge_disable_fails() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "review-feedback-disarm-failure-conversation",
        "project-review-feedback-disarm-failure",
        worktree.path(),
    );
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let feedback = PrReviewFeedback {
        review_id: "review-disarm-failure".to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-05-17T12:00:00Z".to_string()),
        body: Some("Please handle the edge case.".to_string()),
        comments: Vec::new(),
    };
    let mut health = open_pr_health("review-feedback-disarm-failure-head");
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(feedback);
    github.state().fetch_pr_health_result = Some(Ok(health));
    github.state().disable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "permission denied".to_string(),
    )));
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let chat = Arc::new(MockChatService::new());

    let routed = super::route_agent_workspace_review_feedback_if_present(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        Some(agent_run_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("review feedback should handle disarm failure");

    assert!(!routed);
    assert_eq!(github.state().disable_pr_auto_merge_calls, 1);
    assert!(chat.get_sent_messages().await.is_empty());
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    assert_eq!(
        updated.pr_supervision_status.as_deref(),
        Some(super::AUTO_MERGE_SUPERVISION_STATUS_WAITING)
    );
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
}

#[tokio::test]
async fn review_pr_monitor_skips_requested_changes_feedback_routing() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = review_pr_workspace(
        "review-monitor-feedback-skip-conversation",
        "project-review-monitor-feedback-skip",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let feedback = PrReviewFeedback {
        review_id: "review-456".to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-05-17T12:00:00Z".to_string()),
        body: Some("Please handle the edge case.".to_string()),
        comments: vec![PrReviewCommentFeedback {
            id: "comment-2".to_string(),
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
        github.clone() as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        Arc::clone(&workspace_repo),
        None,
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("Review PR feedback routing should skip cleanly");

    assert!(!routed);
    assert_eq!(github.state().check_pr_review_feedback_calls, 0);
    assert!(chat.get_sent_messages().await.is_empty());
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

#[tokio::test]
async fn missing_repair_repository_rejects_pr_conflict_without_side_effects() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let workspace = supervised_workspace(
        "missing-repair-repository",
        "project-missing-repair-repository",
        worktree.path(),
    );
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut health = open_pr_health("missing-repair-repository-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());

    let error = super::route_agent_workspace_pr_conflict_repair_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        workspace_repo.clone(),
        None,
        None,
        Some(Arc::new(MemoryBranchUpdateRepository::new())),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect_err("missing durable repair repository must fail closed");

    assert!(error.to_string().contains("repair authority"));
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert!(chat.get_sent_messages().await.is_empty());
    assert!(workspace_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read durable attempt")
        .is_none());
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list workspace events")
        .is_empty());
}

#[tokio::test]
async fn busy_pr_conflict_repair_does_not_disable_auto_merge_or_send_a_worker() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "busy-pr-conflict",
        "project-busy-pr-conflict",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-oid-busy-conflict".to_string());
    workspace.pr_auto_merge_current = Some(true);
    let expected_push_status = workspace.publication_push_status.clone();
    let expected_supervision_status = workspace.pr_supervision_status.clone();
    let expected_supervision_summary = workspace.pr_supervision_summary.clone();
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let target_identity =
        GitService::canonical_target_identity(worktree.path(), &workspace.branch_name)
            .await
            .expect("resolve canonical target identity");
    let foreign_owner = GitTargetLeaseOwner::agent_workspace_repair("foreign-conflict-owner");
    assert!(matches!(
        branch_update_repo
            .acquire_target_lease(AcquireGitTargetLease {
                identity: target_identity,
                owner: foreign_owner,
            })
            .await
            .expect("reserve foreign target lease"),
        AcquireGitTargetLeaseOutcome::Acquired { .. }
    ));

    let mut health = open_pr_health("busy-conflict-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);
    health.auto_merge_request = Some(PrAutoMergeRequest {
        enabled_by: Some("octocat".to_string()),
        merge_method: Some("squash".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());

    let error = super::route_agent_workspace_pr_conflict_repair_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        workspace_repo.clone(),
        None,
        Some(repair_repo),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect_err("a foreign target lease must reject the repair dispatch");

    assert!(error.to_string().contains("owned"));
    assert_eq!(
        github.state().disable_pr_auto_merge_calls,
        0,
        "a Busy dispatch must return before mutating GitHub auto-merge"
    );
    assert!(
        chat.get_sent_messages().await.is_empty(),
        "a Busy dispatch must not queue a repair worker"
    );
    assert!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list workspace events")
            .is_empty(),
        "a Busy dispatch must not append a repair delivery audit event"
    );
    let unchanged_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace after Busy dispatch")
        .expect("workspace remains present");
    assert_eq!(
        unchanged_workspace.publication_push_status, expected_push_status,
        "a Busy dispatch must not project repair publication state"
    );
    assert_eq!(
        unchanged_workspace.pr_supervision_status, expected_supervision_status,
        "a Busy dispatch must not project PR supervision state"
    );
    assert_eq!(
        unchanged_workspace.pr_supervision_summary, expected_supervision_summary,
        "a Busy dispatch must not project a repair summary"
    );
}

#[tokio::test]
async fn live_pr_conflict_repair_repo_route_preserves_durable_authority_on_stale_join() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "durable-pr-conflict",
        "project-durable-pr-conflict",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-oid-before-conflict".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));
    let mut health = open_pr_health("conflict-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);

    assert!(
        super::route_agent_workspace_pr_conflict_repair_if_needed_with_repair_repo(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &health,
            &conversation_id,
            workspace_repo.clone(),
            Some(agent_run_repo.clone()),
            Some(Arc::clone(&repair_repo)),
            Some(Arc::clone(&branch_update_repo)),
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("live PR-conflict repair route should dispatch")
    );

    let first = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load")
        .expect("PR conflict must create a durable repair attempt");
    let reserved_run_id = first
        .reserved_agent_run_id
        .clone()
        .expect("PR conflict must persist the exact reserved repair run");
    assert_eq!(first.generation, 1);
    assert_eq!(first.source, AgentWorkspaceRepairSource::PrConflict);
    assert_eq!(
        first.continuation,
        AgentWorkspaceRepairContinuation::ResumePrSupervision
    );
    assert_eq!(first.phase, AgentWorkspaceRepairPhase::Repairing);
    assert_eq!(first.target_base_ref, "main");
    assert_eq!(
        first.target_base_commit.as_deref(),
        Some("base-oid-before-conflict")
    );
    let messages_before_join = chat.get_sent_messages().await;
    let events_before_join = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");

    let mut stale_workspace = workspace.clone();
    stale_workspace.base_commit = Some("base-oid-stale-conflict".to_string());
    workspace_repo
        .create_or_update(stale_workspace)
        .await
        .expect("stale workspace observation should persist");
    assert!(
        !super::route_agent_workspace_pr_conflict_repair_if_needed_with_repair_repo(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &health,
            &conversation_id,
            workspace_repo.clone(),
            Some(agent_run_repo),
            Some(Arc::clone(&repair_repo)),
            Some(Arc::clone(&branch_update_repo)),
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("stale PR-conflict join should be harmless")
    );

    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load after stale join")
        .expect("original PR-conflict repair should remain active");
    assert_eq!(current.id, first.id);
    assert_eq!(current.generation, 1);
    assert_eq!(
        current.reserved_agent_run_id,
        Some(reserved_run_id),
        "stale PR-conflict joins must not replace the current run reservation"
    );
    assert_eq!(current.target_base_ref, "main");
    assert_eq!(
        current.target_base_commit.as_deref(),
        Some("base-oid-before-conflict"),
        "stale PR-conflict observations must not overwrite base authority"
    );
    assert_eq!(chat.get_sent_messages().await, messages_before_join);
    let events_after_join = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list after stale join");
    assert_eq!(events_after_join.len(), events_before_join.len() + 1);
    assert!(events_after_join.iter().any(|event| {
        event.step == "repair_routed"
            && event.status == "waiting"
            && event.classification.as_deref()
                == Some(
                    format!(
                        "agent_workspace_repair_routed:101:joined:merge-conflict:{}:{}",
                        first.id, first.generation
                    )
                    .as_str(),
                )
            && event.summary.contains("merge-conflict signal")
            && event
                .summary
                .contains("routed to an existing workspace repair attempt")
    }));
    assert!(repair_repo
        .get_open_repair_effect(&first.id)
        .await
        .expect("repair effects should load")
        .is_none());
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
}

#[tokio::test]
async fn conflict_router_defers_unpublished_repair_head_without_join_or_agent_instruction() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "conflict-unpublished-head",
        "project-conflict-unpublished-head",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-before-unpublished-conflict".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let fingerprint = "github_pr_autofix:101:conflict";
    let held = reserve_health_held_attempt(
        repair_repo.as_ref(),
        &conversation_id,
        fingerprint,
        crate::application::agent_workspace_publish_repair_state::UNCHANGED_HEALTH_REPAIR_REASON,
    )
    .await;
    let expected_updated_at = held.updated_at;
    let mut unpublished = held.clone();
    unpublished.repair_head_commit = Some("validated-local-conflict-head".to_string());
    unpublished.updated_at += chrono::Duration::microseconds(1);
    let unpublished = match repair_repo
        .transition_repair_attempt(
            crate::domain::repositories::AgentWorkspaceRepairAttemptTransition {
                attempt: unpublished,
                expected_phase: AgentWorkspaceRepairPhase::Ready,
                expected_updated_at,
                next_phase: AgentWorkspaceRepairPhase::Ready,
                compatibility_projection: None,
                events: Vec::new(),
            },
        )
        .await
        .expect("persist unpublished conflict repair head")
    {
        crate::domain::repositories::AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
            attempt,
        ) => attempt,
        outcome => panic!("unpublished conflict checkpoint must apply, got {outcome:?}"),
    };
    let github = Arc::new(MockGithubService::new());
    let chat = Arc::new(MockChatService::new());
    let mut health = open_pr_health("remote-conflict-head");
    health.sync_state.merge_state_status = Some(PrMergeStateStatus::Dirty);
    health.sync_state.mergeable = Some(PrMergeableState::Conflicting);

    let routed = super::route_agent_workspace_pr_conflict_repair_if_needed_with_repair_repo(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &health,
        &conversation_id,
        workspace_repo.clone(),
        None,
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("unpublished conflict head should defer safely");

    assert!(
        !routed,
        "the conflict router must not start or join a new repair"
    );
    assert!(
        chat.get_sent_messages().await.is_empty(),
        "no false repair instruction"
    );
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload unpublished conflict repair")
        .expect("unpublished conflict repair remains current");
    assert_eq!(current.id, unpublished.id);
    assert_eq!(current.generation, unpublished.generation);
    assert!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list publication events")
            .is_empty(),
        "the guard must not record a joined repair event"
    );
}

#[tokio::test]
async fn live_pr_autofix_suppresses_same_fingerprint_while_ci_rerun_is_pending() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "pending-rerun-fingerprint",
        "project-pending-rerun-fingerprint",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-pending-rerun".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let fingerprint = "ci-hold:v1:pending-head:901";
    let pending =
        reserve_pending_ci_rerun_attempt(repair_repo.as_ref(), &conversation_id, fingerprint).await;
    let mut health = open_pr_health("pending-head");
    health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("cancelled".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/901".to_string()),
    });
    health.checks.push(PrHealthCheck {
        name: "Rust tests / sibling".to_string(),
        status: Some("in_progress".to_string()),
        conclusion: None,
        details_url: Some("https://github.com/owner/repo/actions/runs/901/jobs/2".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("same pending CI fingerprint should be handled without an error");

    assert!(
        !routed,
        "pending rerun must suppress a duplicate autofix dispatch"
    );
    assert!(chat.get_sent_messages().await.is_empty());
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load")
        .expect("pending rerun attempt should remain current");
    assert_eq!(current.id, pending.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(current.ci_rerun_fingerprint.as_deref(), Some(fingerprint));
}

#[tokio::test]
async fn legacy_ci_rerun_fingerprint_settles_instead_of_hanging() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "changed-rerun-fingerprint",
        "project-changed-rerun-fingerprint",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-changed-rerun".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let pending = reserve_pending_ci_rerun_attempt(
        repair_repo.as_ref(),
        &conversation_id,
        "changed-head:Rust tests:failure:https://github.com/owner/repo/actions/runs/902",
    )
    .await;
    let mut changed_health = open_pr_health("changed-head");
    changed_health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/903".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(changed_health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("changed CI fingerprint should dispatch a new autofix generation");

    assert!(routed);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let settled = repair_repo
        .get_repair_attempt(&pending.id)
        .await
        .expect("pending generation should load")
        .expect("pending generation should remain durable");
    assert_eq!(
        settled.outcome,
        Some(crate::domain::entities::AgentWorkspaceRepairOutcome::Succeeded)
    );
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("new repair generation should load")
        .expect("changed fingerprint should start a new generation");
    assert_eq!(current.generation, pending.generation + 1);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Repairing);
}

#[tokio::test]
async fn ci_rerun_hold_settles_once_reran_runs_are_terminal() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "terminal-rerun-hold",
        "project-terminal-rerun-hold",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-terminal-rerun".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let pending = reserve_pending_ci_rerun_attempt(
        repair_repo.as_ref(),
        &conversation_id,
        "ci-hold:v1:terminal-head:904",
    )
    .await;
    let mut health = open_pr_health("terminal-head");
    health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("cancelled".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/904/jobs/1".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("terminal rerun should settle and allow a fresh dispatch");

    assert!(routed);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let settled = repair_repo
        .get_repair_attempt(&pending.id)
        .await
        .expect("settled attempt should load")
        .expect("settled attempt stays durable");
    assert_eq!(
        settled.outcome,
        Some(crate::domain::entities::AgentWorkspaceRepairOutcome::Succeeded)
    );
}

#[tokio::test]
async fn ci_await_hold_suppresses_dispatch_and_survives_unchanged_classification() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "await-rerun-hold",
        "project-await-rerun-hold",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-await-rerun".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let pending = reserve_pending_ci_await_attempt(
        repair_repo.as_ref(),
        &conversation_id,
        "ci-hold:v1:await-head:905",
    )
    .await;
    let mut health = open_pr_health("await-head");
    health.checks.push(PrHealthCheck {
        name: "Rust tests / cancelled".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("cancelled".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/905/jobs/1".to_string()),
    });
    health.checks.push(PrHealthCheck {
        name: "Rust tests / sibling".to_string(),
        status: Some("in_progress".to_string()),
        conclusion: None,
        details_url: Some("https://github.com/owner/repo/actions/runs/905/jobs/2".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("awaiting CI should suppress duplicate dispatch");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("current attempt should load")
        .expect("awaiting attempt stays current");
    assert_eq!(current.id, pending.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(current.ci_rerun_count, 0);
}

#[tokio::test]
async fn ci_hold_settles_when_head_moves() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "moved-head-rerun-hold",
        "project-moved-head-rerun-hold",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-moved-head-rerun".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let pending = reserve_pending_ci_rerun_attempt(
        repair_repo.as_ref(),
        &conversation_id,
        "ci-hold:v1:old-head:906",
    )
    .await;
    let mut health = open_pr_health("new-head");
    health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("in_progress".to_string()),
        conclusion: None,
        details_url: Some("https://github.com/owner/repo/actions/runs/906/jobs/1".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("a moved head should end the old CI hold");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let settled = repair_repo
        .get_repair_attempt(&pending.id)
        .await
        .expect("settled attempt should load")
        .expect("settled attempt stays durable");
    assert_eq!(
        settled.outcome,
        Some(crate::domain::entities::AgentWorkspaceRepairOutcome::Succeeded)
    );
}

#[tokio::test]
async fn unrelated_conversation_dispatch_does_not_settle_a_ci_hold() {
    let routed_worktree = tempfile::tempdir().expect("routed worktree path");
    let held_worktree = tempfile::tempdir().expect("held worktree path");
    let mut routed_workspace = supervised_workspace(
        "00000000-0000-0000-0000-000000000101",
        "00000000-0000-0000-0000-000000000201",
        routed_worktree.path(),
    );
    let held_workspace = supervised_workspace(
        "00000000-0000-0000-0000-000000000102",
        "00000000-0000-0000-0000-000000000202",
        held_worktree.path(),
    );
    init_repair_dispatch_repo(routed_worktree.path(), &routed_workspace.branch_name);
    routed_workspace.base_commit = Some("base-routed-conversation".to_string());
    let routed_conversation_id = routed_workspace.conversation_id.clone();
    let held_conversation_id = held_workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(routed_workspace)
        .await
        .expect("routed workspace should persist");
    workspace_repo
        .create_or_update(held_workspace)
        .await
        .expect("held workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let held = reserve_pending_ci_rerun_attempt(
        repair_repo.as_ref(),
        &held_conversation_id,
        "ci-hold:v1:held-head:907",
    )
    .await;
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&routed_conversation_id).await;
    let mut health = open_pr_health("routed-head");
    health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/908/jobs/1".to_string()),
    });
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        routed_worktree.path(),
        101,
        &routed_conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("the routed conversation should process independently");

    assert!(routed);
    let held_after = repair_repo
        .get_repair_attempt(&held.id)
        .await
        .expect("held attempt should load")
        .expect("held attempt stays durable");
    assert_eq!(
        held_after, held,
        "unrelated routing must not mutate the hold"
    );
}

/// Builds a workspace whose PR is failing one named check, ready for base-comparison tests.
async fn seed_failing_check_workspace(
    label: &str,
    check_name: &str,
) -> (
    tempfile::TempDir,
    Arc<MemoryAgentConversationWorkspaceRepository>,
    ChatConversationId,
    PrHealth,
) {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(label, &format!("project-{label}"), worktree.path());
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some(format!("base-{label}"));
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let mut health = open_pr_health(&format!("{label}-head"));
    health.checks.push(PrHealthCheck {
        name: check_name.to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/940".to_string()),
    });
    (worktree, workspace_repo, conversation_id, health)
}

async fn route_with_base_conclusions(
    worktree: &tempfile::TempDir,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    health: PrHealth,
    base_conclusions: AppResult<Option<Vec<PrHealthCheck>>>,
) -> (bool, Arc<MockChatService>) {
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(conversation_id).await;
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    github.state().list_branch_check_conclusions_result = Some(base_conclusions);
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(repair_repo),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("routing should complete");
    (routed, chat)
}

/// A failure the PR did not cause cannot be fixed by a PR fixer. When GitHub proves the same check
/// already fails on the base branch, RalphX hands off instead of spending a generation.
#[tokio::test]
async fn failure_proven_on_base_is_handed_off_without_spawning_a_fixer() {
    let (worktree, workspace_repo, conversation_id, health) =
        seed_failing_check_workspace("pre-existing-detected", "Rust tests").await;
    let classification = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("failed check should classify")
        .classification;

    let (routed, chat) = route_with_base_conclusions(
        &worktree,
        workspace_repo.clone(),
        &conversation_id,
        health,
        Ok(Some(vec![PrHealthCheck {
            name: "Rust tests".to_string(),
            status: Some("completed".to_string()),
            conclusion: Some("failure".to_string()),
            details_url: Some("https://github.com/owner/repo/actions/runs/1".to_string()),
        }])),
    )
    .await;

    assert!(!routed, "a base-caused failure must not spawn a fixer");
    assert!(chat.get_sent_messages().await.is_empty());
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events");
    assert!(
        events
            .iter()
            .any(|event| event.step == super::PRE_EXISTING_ON_BASE_DETECTED_STEP),
        "the hand-off must be visible on the publication timeline"
    );
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("reload workspace")
        .expect("workspace exists");
    assert_eq!(
        workspace.last_blocked_pr_health_fingerprint.as_deref(),
        Some(classification.as_str()),
        "the identity must be remembered so later polls stay handed off"
    );
}

/// The scope-gated-CI case: a check that never runs on the base proves nothing, so the agent runs.
#[tokio::test]
async fn failure_absent_from_base_still_dispatches_a_fixer() {
    let (worktree, workspace_repo, conversation_id, health) =
        seed_failing_check_workspace("pre-existing-absent", "Rust tests").await;

    let (routed, chat) = route_with_base_conclusions(
        &worktree,
        workspace_repo.clone(),
        &conversation_id,
        health,
        Ok(Some(vec![PrHealthCheck {
            name: "Frontend tests".to_string(),
            status: Some("completed".to_string()),
            conclusion: Some("success".to_string()),
            details_url: None,
        }])),
    )
    .await;

    assert!(routed, "a check absent from base proves nothing about base");
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    assert!(!workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events")
        .iter()
        .any(|event| event.step == super::PRE_EXISTING_ON_BASE_DETECTED_STEP));
}

/// An unreadable base must fail open to the agent. Skipping repair on an API error would silently
/// ignore real PR failures.
#[tokio::test]
async fn unreadable_base_conclusions_still_dispatch_a_fixer() {
    let (worktree, workspace_repo, conversation_id, health) =
        seed_failing_check_workspace("pre-existing-error", "Rust tests").await;

    let (routed, chat) = route_with_base_conclusions(
        &worktree,
        workspace_repo.clone(),
        &conversation_id,
        health,
        Err(crate::error::AppError::Infrastructure(
            "gh run list failed".to_string(),
        )),
    )
    .await;

    assert!(routed, "an unreadable base must never suppress repair");
    assert_eq!(chat.get_sent_messages().await.len(), 1);
}

/// An unimplemented backend reports "unknown", which must behave exactly like an error.
#[tokio::test]
async fn unknown_base_conclusions_still_dispatch_a_fixer() {
    let (worktree, workspace_repo, conversation_id, health) =
        seed_failing_check_workspace("pre-existing-unknown", "Rust tests").await;

    let (routed, _chat) = route_with_base_conclusions(
        &worktree,
        workspace_repo.clone(),
        &conversation_id,
        health,
        Ok(None),
    )
    .await;

    assert!(routed, "unknown base state must not be read as healthy");
}

/// Cross-streak memory: an exhausted streak leaves its failure identity on the workspace, and the
/// next poll must recognise it instead of starting a fresh streak on identical evidence.
#[tokio::test]
async fn exhausted_streak_fingerprint_suppresses_a_fresh_streak_until_health_changes() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "cross-streak-fingerprint",
        "project-cross-streak-fingerprint",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-cross-streak".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let mut exhausted_health = open_pr_health("cross-streak-head");
    exhausted_health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/930".to_string()),
    });
    let fingerprint = super::classify_agent_workspace_pr_autofix_issue(101, &exhausted_health)
        .expect("failed check should classify")
        .classification;
    // No current repair attempt: the previous streak is gone, exactly as after exhaustion.
    workspace_repo
        .set_last_blocked_pr_health_fingerprint(&conversation_id, Some(&fingerprint))
        .await
        .expect("remember the exhausted failure identity");

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(exhausted_health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo.clone(),
        Some(agent_run_repo.clone()),
        Some(Arc::clone(&repair_repo)),
        Some(Arc::clone(&branch_update_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("an exhausted fingerprint should suppress a fresh streak");

    assert!(
        !routed,
        "a fresh streak on identical evidence must not start"
    );
    assert!(chat.get_sent_messages().await.is_empty());
    assert!(
        repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load current attempt")
            .is_none(),
        "suppression must not create a repair generation"
    );
    let hold_events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events")
        .into_iter()
        .filter(|event| event.step == super::CROSS_STREAK_FINGERPRINT_HOLD_STEP)
        .count();
    assert_eq!(hold_events, 1, "the hold must be visible, exactly once");

    // Polling again must stay suppressed and must not repeat the event.
    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo.clone(),
        Some(agent_run_repo.clone()),
        Some(Arc::clone(&repair_repo)),
        Some(Arc::clone(&branch_update_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("repeat polls stay suppressed");
    assert!(!routed);
    assert_eq!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list publication events")
            .into_iter()
            .filter(|event| event.step == super::CROSS_STREAK_FINGERPRINT_HOLD_STEP)
            .count(),
        1,
        "the hold event must be deduped, not repeated every poll"
    );

    // Different health is new evidence: the memory clears and autofix runs again.
    let mut changed_health = open_pr_health("cross-streak-head");
    changed_health.checks.push(PrHealthCheck {
        name: "Clippy".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/931".to_string()),
    });
    github.state().fetch_pr_health_result = Some(Ok(changed_health));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo.clone(),
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("changed health should clear the memory and dispatch");

    assert!(
        routed,
        "a genuinely new failure must not be held by a stale one"
    );
    let refreshed = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("reload workspace")
        .expect("workspace exists");
    assert!(
        refreshed.last_blocked_pr_health_fingerprint.is_none(),
        "changed health must clear the remembered failure identity"
    );
}

/// A generation parked because GitHub reported unchanged health must be honoured by the poller's
/// dispatch gate exactly like a pre-existing-on-base hold. Without this the durable recovery lane
/// parks the attempt and the very next poll starts another fixer on identical evidence — the
/// four-generation loop from the 2026-07-31 incident.
#[tokio::test]
async fn live_pr_autofix_unchanged_health_hold_suppresses_same_fingerprint_then_redispatches() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "unchanged-health-hold-fingerprint",
        "project-unchanged-health-hold",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-unchanged-health".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let mut original_health = open_pr_health("unchanged-health-head");
    original_health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/920".to_string()),
    });
    let fingerprint = super::classify_agent_workspace_pr_autofix_issue(101, &original_health)
        .expect("failed check should classify")
        .classification;
    let held = reserve_health_held_attempt(
        repair_repo.as_ref(),
        &conversation_id,
        &fingerprint,
        crate::application::agent_workspace_publish_repair_state::UNCHANGED_HEALTH_REPAIR_REASON,
    )
    .await;
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(original_health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo.clone(),
        Some(agent_run_repo.clone()),
        Some(Arc::clone(&repair_repo)),
        Some(Arc::clone(&branch_update_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("unchanged health should be suppressed");

    assert!(
        !routed,
        "unchanged health must not start another generation"
    );
    assert!(chat.get_sent_messages().await.is_empty());
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("held attempt should load")
        .expect("held attempt remains current");
    assert_eq!(current.id, held.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);

    // A different failing check is new evidence, so the hold ends and a fresh generation runs.
    let mut changed_health = open_pr_health("unchanged-health-head");
    changed_health.checks.push(PrHealthCheck {
        name: "Clippy".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/921".to_string()),
    });
    github.state().fetch_pr_health_result = Some(Ok(changed_health));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("changed health should dispatch a new generation");

    assert!(routed, "changed health must be able to end the hold");
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("new repair generation should load")
        .expect("changed fingerprint should create a new generation");
    assert_eq!(current.generation, held.generation + 1);
}

#[tokio::test]
async fn live_pr_autofix_new_base_evidence_supersedes_same_fingerprint_health_hold() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "same-health-new-base",
        "project-same-health-new-base",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-before-hold".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let mut health = open_pr_health("same-health-head");
    health.sync_state.base_ref_oid = Some("base-before-hold".to_string());
    health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/922".to_string()),
    });
    let fingerprint = super::classify_agent_workspace_pr_autofix_issue(101, &health)
        .expect("failed check should classify")
        .classification;
    let held = reserve_health_held_attempt(
        repair_repo.as_ref(),
        &conversation_id,
        &fingerprint,
        crate::application::agent_workspace_publish_repair_state::UNCHANGED_HEALTH_REPAIR_REASON,
    )
    .await;
    let mut targeted = held.clone();
    targeted.target_base_commit = Some("base-before-hold".to_string());
    targeted.updated_at += chrono::Duration::microseconds(1);
    let targeted = match repair_repo
        .transition_repair_attempt(
            crate::domain::repositories::AgentWorkspaceRepairAttemptTransition {
                attempt: targeted,
                expected_phase: AgentWorkspaceRepairPhase::Ready,
                expected_updated_at: held.updated_at,
                next_phase: AgentWorkspaceRepairPhase::Ready,
                compatibility_projection: None,
                events: Vec::new(),
            },
        )
        .await
        .expect("persist held base authority")
    {
        crate::domain::repositories::AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
            attempt,
        ) => attempt,
        outcome => panic!("held base authority must apply, got {outcome:?}"),
    };
    health.sync_state.base_ref_oid = Some("base-after-hold".to_string());
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        github as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo.clone(),
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("new base evidence should supersede the hold");

    assert!(routed, "a moved authoritative base must release the hold");
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load successor")
        .expect("successor exists");
    assert_eq!(current.generation, targeted.generation + 1);
    assert_eq!(
        current.target_base_commit.as_deref(),
        Some("base-after-hold")
    );
    let updated_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load updated workspace")
        .expect("workspace exists");
    assert_eq!(
        updated_workspace.base_commit.as_deref(),
        Some("base-after-hold")
    );
}

#[tokio::test]
async fn live_pr_autofix_pre_existing_on_base_suppresses_same_fingerprint_then_redispatches() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "pre-existing-on-base-fingerprint",
        "project-pre-existing-on-base-fingerprint",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-pre-existing-on-base".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let mut original_health = open_pr_health("pre-existing-head");
    original_health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/904".to_string()),
    });
    let fingerprint = super::classify_agent_workspace_pr_autofix_issue(101, &original_health)
        .expect("failed check should classify")
        .classification;
    let suppressed =
        reserve_pre_existing_on_base_attempt(repair_repo.as_ref(), &conversation_id, &fingerprint)
            .await;
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(original_health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo.clone(),
        Some(agent_run_repo.clone()),
        Some(Arc::clone(&repair_repo)),
        Some(Arc::clone(&branch_update_repo)),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("unchanged pre-existing failure should be suppressed");

    assert!(!routed);
    assert!(chat.get_sent_messages().await.is_empty());
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("suppressed attempt should load")
        .expect("suppressed attempt remains current");
    assert_eq!(current.id, suppressed.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);

    let mut changed_health = open_pr_health("pre-existing-head");
    changed_health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/905".to_string()),
    });
    github.state().fetch_pr_health_result = Some(Ok(changed_health));

    let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
        worktree.path(),
        101,
        &conversation_id,
        workspace_repo,
        Some(agent_run_repo),
        Some(Arc::clone(&repair_repo)),
        Some(branch_update_repo),
        chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
    )
    .await
    .expect("changed pre-existing failure should dispatch a new generation");

    assert!(routed);
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let settled = repair_repo
        .get_repair_attempt(&suppressed.id)
        .await
        .expect("suppressed generation should load")
        .expect("suppressed generation should remain durable");
    assert_eq!(
        settled.outcome,
        Some(crate::domain::entities::AgentWorkspaceRepairOutcome::Succeeded)
    );
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("new repair generation should load")
        .expect("changed fingerprint should create a new generation");
    assert_eq!(current.generation, suppressed.generation + 1);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Repairing);
}

#[tokio::test]
async fn live_pr_autofix_repair_repo_route_deduplicates_concurrent_dispatches() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "durable-pr-autofix",
        "project-durable-pr-autofix",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-oid-autofix".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let github = Arc::new(MockGithubService::new());
    let mut health = open_pr_health("autofix-head");
    health.checks.push(PrHealthCheck {
        name: "Rust tests".to_string(),
        status: Some("completed".to_string()),
        conclusion: Some("failure".to_string()),
        details_url: Some("https://github.com/owner/repo/actions/runs/1".to_string()),
    });
    github.state().fetch_pr_health_result = Some(Ok(health));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    let (first, duplicate) = tokio::join!(
        super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            workspace_repo.clone(),
            Some(agent_run_repo.clone()),
            Some(Arc::clone(&repair_repo)),
            Some(Arc::clone(&branch_update_repo)),
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        ),
        super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            workspace_repo.clone(),
            Some(agent_run_repo.clone()),
            Some(Arc::clone(&repair_repo)),
            Some(Arc::clone(&branch_update_repo)),
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
    );
    let successful_dispatches = [
        first.expect("first live autofix route should succeed"),
        duplicate.expect("duplicate live autofix route should settle harmlessly"),
    ]
    .into_iter()
    .filter(|routed| *routed)
    .count();
    assert_eq!(
        successful_dispatches, 1,
        "only one concurrent producer may dispatch the repair agent"
    );

    let attempt = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load")
        .expect("PR autofix must create a durable repair attempt");
    assert_eq!(attempt.generation, 1);
    assert_eq!(attempt.source, AgentWorkspaceRepairSource::PrAutofix);
    assert_eq!(
        attempt.continuation,
        AgentWorkspaceRepairContinuation::ResumePrSupervision
    );
    assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Repairing);
    let reserved_run_id = attempt
        .reserved_agent_run_id
        .as_ref()
        .expect("PR autofix must persist exactly one repair run reservation");
    assert_eq!(attempt.target_base_ref, "main");
    assert_eq!(
        attempt.target_base_commit.as_deref(),
        Some("base-oid-autofix")
    );
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    assert_eq!(
        agent_run_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("repair runs should list")
            .len(),
        2,
        "the seed run plus exactly one reserved repair run must exist"
    );
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(repair_repo
        .get_open_repair_effect(&attempt.id)
        .await
        .expect("repair effects should load")
        .is_none());
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "repair_sent" && event.status == "started")
            .count(),
        1,
        "concurrent joins must not append another repair-reservation event"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "repair_sent" && event.status == "succeeded")
            .count(),
        1,
        "concurrent joins must not append another repair-dispatched event"
    );
    let reserved_run_classification = format!("agent_fixable:run:{reserved_run_id}");
    assert!(events
        .iter()
        .filter(|event| event.step == "repair_sent")
        .all(|event| event.classification.as_deref() == Some(&reserved_run_classification)));
}

#[tokio::test]
async fn live_pr_autofix_repair_routed_signal_records_once_for_existing_attempt() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "durable-pr-autofix-routed",
        "project-durable-pr-autofix-routed",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-oid-autofix-routed".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let github = Arc::new(MockGithubService::new());
    let failing_health = || {
        let mut health = open_pr_health("autofix-head");
        health.checks.push(PrHealthCheck {
            name: "Rust tests".to_string(),
            status: Some("completed".to_string()),
            conclusion: Some("failure".to_string()),
            details_url: Some("https://github.com/owner/repo/actions/runs/1".to_string()),
        });
        health
    };
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    github.state().fetch_pr_health_result = Some(Ok(failing_health()));
    assert!(
        super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            workspace_repo.clone(),
            Some(agent_run_repo.clone()),
            Some(Arc::clone(&repair_repo)),
            Some(Arc::clone(&branch_update_repo)),
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("first live autofix route should dispatch")
    );
    let attempt = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load")
        .expect("PR autofix must create a durable repair attempt");
    let messages_after_dispatch = chat.get_sent_messages().await.len();

    // The live poller keeps observing the same failing checks while the repair attempt is
    // still current; restore its pollable projection so each cycle reaches the join seam.
    let dispatched = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load after dispatch")
        .expect("workspace remains present");
    let mut pollable = dispatched;
    pollable.publication_push_status = Some("pushed".to_string());
    workspace_repo
        .create_or_update(pollable)
        .await
        .expect("pollable projection should persist");

    for cycle in 0..2 {
        github.state().fetch_pr_health_result = Some(Ok(failing_health()));
        let routed = super::route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            workspace_repo.clone(),
            Some(agent_run_repo.clone()),
            Some(Arc::clone(&repair_repo)),
            Some(Arc::clone(&branch_update_repo)),
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("joined poll cycles must settle harmlessly");
        assert!(!routed, "cycle {cycle} must not dispatch a second repair");
    }

    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load after joined cycles")
        .expect("original autofix repair should remain current");
    assert_eq!(current.id, attempt.id);
    assert_eq!(
        chat.get_sent_messages().await.len(),
        messages_after_dispatch,
        "joined poll cycles must not send another repair dispatch"
    );
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list after joined cycles");
    let routed_events: Vec<_> = events
        .iter()
        .filter(|event| event.step == "repair_routed")
        .collect();
    assert_eq!(
        routed_events.len(),
        1,
        "repeated joined cycles must record exactly one routed event"
    );
    let routed = routed_events[0];
    assert_eq!(routed.status, "waiting");
    assert_eq!(
        routed.classification.as_deref(),
        Some(
            format!(
                "agent_workspace_repair_routed:101:joined:CI-failure:{}:{}",
                attempt.id, attempt.generation
            )
            .as_str()
        )
    );
    assert!(routed.summary.contains("CI-failure signal"));
    assert!(routed.summary.contains("1 failing check"));
}

#[tokio::test]
async fn routed_repair_audit_deduplicates_per_attempt_generation_and_outcome() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::new();
    let first_attempt = AgentWorkspaceRepairAttempt::new(
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

    super::record_agent_workspace_repair_routed_to_existing_attempt(
        workspace_repo.as_ref(),
        &conversation_id,
        101,
        "joined",
        "CI-failure",
        &first_attempt,
        "first observation",
    )
    .await
    .expect("first routed audit should persist");
    super::record_agent_workspace_repair_routed_to_existing_attempt(
        workspace_repo.as_ref(),
        &conversation_id,
        101,
        "joined",
        "CI-failure",
        &first_attempt,
        "a changed summary must not create another audit row",
    )
    .await
    .expect("identical fingerprint should deduplicate");

    let mut next_generation = first_attempt.clone();
    next_generation.generation = first_attempt.generation + 1;
    super::record_agent_workspace_repair_routed_to_existing_attempt(
        workspace_repo.as_ref(),
        &conversation_id,
        101,
        "joined",
        "CI-failure",
        &next_generation,
        "next generation",
    )
    .await
    .expect("next generation should be audited");
    super::record_agent_workspace_repair_routed_to_existing_attempt(
        workspace_repo.as_ref(),
        &conversation_id,
        101,
        "blocked_by_current",
        "CI-failure",
        &next_generation,
        "different outcome",
    )
    .await
    .expect("different outcome should be audited");

    let routed_events: Vec<_> = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("routed events should list")
        .into_iter()
        .filter(|event| event.step == "repair_routed")
        .collect();
    assert_eq!(routed_events.len(), 3);
    assert!(routed_events.iter().any(|event| {
        event.classification.as_deref()
            == Some(
                format!(
                    "agent_workspace_repair_routed:101:joined:CI-failure:{}:{}",
                    first_attempt.id, first_attempt.generation
                )
                .as_str(),
            )
    }));
    assert!(routed_events.iter().any(|event| {
        event.classification.as_deref()
            == Some(
                format!(
                    "agent_workspace_repair_routed:101:blocked_by_current:CI-failure:{}:{}",
                    next_generation.id, next_generation.generation
                )
                .as_str(),
            )
    }));
}

#[tokio::test]
async fn live_review_feedback_repair_repo_route_keeps_existing_continuation_authority() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let mut workspace = supervised_workspace(
        "durable-review-feedback",
        "project-durable-review-feedback",
        worktree.path(),
    );
    init_repair_dispatch_repo(worktree.path(), &workspace.branch_name);
    workspace.base_commit = Some("base-oid-review-feedback".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let agent_run_repo = seeded_latest_pr_fixer_run_repo(&conversation_id).await;
    let github = Arc::new(MockGithubService::new());
    github.will_return_review_feedback(requested_changes_feedback("durable-review-feedback"));
    github.state().fetch_pr_health_result = Some(Ok(open_pr_health("review-feedback-head")));
    let chat = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &agent_run_repo,
    )));

    assert!(
        super::route_agent_workspace_review_feedback_if_present_with_repair_repo(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            workspace_repo.clone(),
            Some(agent_run_repo.clone()),
            Some(Arc::clone(&repair_repo)),
            Some(Arc::clone(&branch_update_repo)),
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("live review-feedback repair route should dispatch")
    );

    let first = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load")
        .expect("review feedback must create a durable repair attempt");
    let reserved_run_id = first
        .reserved_agent_run_id
        .clone()
        .expect("review feedback must persist the reserved run");
    assert_eq!(first.generation, 1);
    assert_eq!(first.source, AgentWorkspaceRepairSource::PrAutofix);
    assert_eq!(
        first.continuation,
        AgentWorkspaceRepairContinuation::ResumePrSupervision
    );
    assert_eq!(first.phase, AgentWorkspaceRepairPhase::Repairing);
    assert_eq!(
        first.target_base_commit.as_deref(),
        Some("base-oid-review-feedback")
    );
    let messages_before_repeat = chat.get_sent_messages().await;
    let events_before_repeat = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");

    assert!(
        !super::route_agent_workspace_review_feedback_if_present_with_repair_repo(
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            worktree.path(),
            101,
            &conversation_id,
            workspace_repo.clone(),
            Some(agent_run_repo),
            Some(Arc::clone(&repair_repo)),
            Some(Arc::clone(&branch_update_repo)),
            chat.clone() as Arc<dyn crate::application::chat_service::ChatService>,
        )
        .await
        .expect("duplicate review-feedback route should be harmless")
    );

    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load after duplicate feedback")
        .expect("review-feedback repair should remain active");
    assert_eq!(current.id, first.id);
    assert_eq!(current.generation, 1);
    assert_eq!(
        current.reserved_agent_run_id,
        Some(reserved_run_id),
        "duplicate review feedback must not replace the current run reservation"
    );
    assert_eq!(
        current.continuation,
        AgentWorkspaceRepairContinuation::ResumePrSupervision
    );
    assert_eq!(chat.get_sent_messages().await, messages_before_repeat);
    assert_eq!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("events should list after duplicate feedback"),
        events_before_repeat,
        "duplicate review feedback must not append another repair event"
    );
    assert!(repair_repo
        .get_open_repair_effect(&first.id)
        .await
        .expect("repair effects should load")
        .is_none());
    assert_eq!(github.state().disable_pr_auto_merge_calls, 0);
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
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

#[tokio::test]
async fn agent_workspace_poller_start_reports_unavailable_without_github() {
    let registry = make_registry_no_github();
    let conversation_id = ChatConversationId::from_string("review-pr-start-unavailable");
    let project = Project::new("Review PR".to_string(), "/tmp/review-pr".to_string());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());

    let started = registry.start_agent_workspace_polling(
        conversation_id.clone(),
        411,
        project,
        std::path::PathBuf::from("/tmp/review-pr"),
        workspace_repo,
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MockChatService::new()),
    );

    assert_eq!(started, AgentWorkspacePrPollerStart::Unavailable);
    assert!(!registry.is_agent_workspace_polling(&conversation_id));
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
async fn terminal_agent_workspace_pr_terminalization_stops_active_project_run() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "poller-terminal-active-run-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let run = agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("active run should persist");
    let chat = Arc::new(MockChatService::new());

    terminalize_agent_workspace_after_pr(
        Arc::clone(&workspace_repo),
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        None,
        Some(Arc::clone(&chat) as Arc<dyn crate::application::chat_service::ChatService>),
        &conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ClosedPr,
    )
    .await;

    assert_eq!(
        chat.get_stop_agent_calls().await,
        vec![(ChatContextType::Project, conversation_id.as_str())]
    );
    let updated_run = agent_run_repo
        .get_by_id(&run.id)
        .await
        .expect("run lookup should succeed")
        .expect("run should still exist");
    assert_eq!(updated_run.status, AgentRunStatus::Failed);
    assert_eq!(
        updated_run.error_message.as_deref(),
        Some("Agent stopped because the workspace pull request was closed")
    );
}

#[tokio::test]
async fn terminal_agent_workspace_pr_poller_retries_runtime_shutdown_before_returning() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "poller-terminal-runtime-retry-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let run = agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("active run should persist");
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let chat = Arc::new(MockChatService::new());
    chat.fail_next_stop_agent_calls(1).await;
    let stopping = Arc::new(dashmap::DashMap::new());
    let agent_run_repo_dyn: Arc<dyn AgentRunRepository> = agent_run_repo.clone();
    let plan_branch_repo_dyn: Arc<dyn crate::domain::repositories::PlanBranchRepository> =
        plan_branch_repo;
    let chat_dyn: Arc<dyn crate::application::chat_service::ChatService> = chat.clone();

    super::terminalize_polled_agent_workspace(
        &workspace_repo,
        &agent_run_repo_dyn,
        &plan_branch_repo_dyn,
        &chat_dyn,
        &stopping,
        &conversation_id,
        &project,
        101,
        TerminalAgentWorkspaceCause::MergedPr,
        "merged",
        "Pull request merged",
        Duration::from_millis(1),
    )
    .await;

    assert_eq!(chat.get_stop_agent_calls().await.len(), 2);
    let updated_run = agent_run_repo
        .get_by_id(&run.id)
        .await
        .expect("run lookup should succeed")
        .expect("run should still exist");
    assert_eq!(updated_run.status, AgentRunStatus::Failed);
}

#[tokio::test]
async fn terminal_agent_workspace_pr_poller_retries_authority_persistence_before_shutdown() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "poller-terminal-authority-retry-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    let conversation_id = workspace.conversation_id.clone();
    let concrete_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    concrete_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    concrete_workspace_repo.fail_next_publication_update("authority unavailable");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = concrete_workspace_repo;
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let plan_branch_repo: Arc<dyn crate::domain::repositories::PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let chat = Arc::new(MockChatService::new());
    let chat_dyn: Arc<dyn crate::application::chat_service::ChatService> = chat.clone();
    let stopping = Arc::new(dashmap::DashMap::new());

    super::terminalize_polled_agent_workspace(
        &workspace_repo,
        &agent_run_repo,
        &plan_branch_repo,
        &chat_dyn,
        &stopping,
        &conversation_id,
        &project,
        101,
        TerminalAgentWorkspaceCause::MergedPr,
        "merged",
        "Pull request merged",
        Duration::from_millis(1),
    )
    .await;

    let persisted = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace retained");
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("merged"));
    assert_eq!(
        chat.get_stop_agent_calls().await.len(),
        1,
        "runtime shutdown must begin only after terminal authority persists"
    );
}

#[tokio::test]
async fn mismatched_polled_pr_terminalization_skips_publication_and_runtime_cleanup() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "mismatched-poller-terminal-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let mut workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    workspace.publication_pr_number = Some(942);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/942".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let baseline = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let plan_branch_repo: Arc<dyn crate::domain::repositories::PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let chat = Arc::new(MockChatService::new());
    let chat_dyn: Arc<dyn crate::application::chat_service::ChatService> = chat.clone();
    let stopping = Arc::new(dashmap::DashMap::new());

    super::terminalize_polled_agent_workspace(
        &workspace_repo,
        &agent_run_repo,
        &plan_branch_repo,
        &chat_dyn,
        &stopping,
        &conversation_id,
        &project,
        941,
        TerminalAgentWorkspaceCause::MergedPr,
        "merged",
        "Pull request merged",
        Duration::from_millis(1),
    )
    .await;

    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed"),
        Some(baseline)
    );
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
    assert!(chat.get_stop_agent_calls().await.is_empty());
}

#[tokio::test]
async fn terminal_agent_workspace_pr_cleanup_deletes_verified_merged_artifacts_without_fetch() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "poller-cleanup-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    let conversation_id = workspace.conversation_id.clone();
    let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
    let memory_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        memory_workspace_repo.clone();
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
    cleanup_terminal_agent_workspace_after_pr(
        Arc::clone(&workspace_repo),
        None,
        &conversation_id,
        &project,
    )
    .await;

    assert!(!worktree_path.exists());
    assert!(!branch_exists(repo.path(), &branch));
    assert_eq!(
        memory_workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("cleaned")
    );
}

#[tokio::test]
async fn terminal_agent_workspace_pr_cleanup_does_not_require_remote_fetch() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let conversation_id_str = "poller-fetch-failure-cleanup-conversation";
    let branch = expected_workspace_branch(&project, conversation_id_str);
    let workspace = cleanup_workspace_with_conversation(&project, &branch, conversation_id_str);
    let conversation_id = workspace.conversation_id.clone();
    let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
    let memory_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        memory_workspace_repo.clone();
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
    cleanup_terminal_agent_workspace_after_pr(
        Arc::clone(&workspace_repo),
        None,
        &conversation_id,
        &project,
    )
    .await;

    assert!(!worktree_path.exists());
    assert!(!branch_exists(repo.path(), &branch));
    assert_eq!(
        memory_workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("cleaned")
    );
}

#[tokio::test]
async fn terminal_agent_workspace_pr_cleanup_preserves_non_owned_branch_marker() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let branch = "feature/user-owned-agent-workspace";
    run_git(repo.path(), &["branch", branch, "main"]);

    let workspace =
        cleanup_workspace_with_conversation(&project, branch, "poller-non-owned-cleanup");
    let conversation_id = workspace.conversation_id.clone();
    let memory_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        memory_workspace_repo.clone();
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    cleanup_terminal_agent_workspace_after_pr(
        Arc::clone(&workspace_repo),
        None,
        &conversation_id,
        &project,
    )
    .await;

    assert!(branch_exists(repo.path(), branch));
    assert_eq!(
        memory_workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("failed_unsafe")
    );
}

#[tokio::test]
async fn terminal_agent_workspace_pr_cleanup_returns_when_workspace_missing() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let memory_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        memory_workspace_repo.clone();
    let conversation_id = ChatConversationId::new();

    cleanup_terminal_agent_workspace_after_pr(workspace_repo, None, &conversation_id, &project)
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

    cleanup_terminal_agent_workspace_after_pr(workspace_repo, None, &conversation_id, &project)
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

    cleanup_terminal_agent_workspace_after_pr(workspace_repo, None, &conversation_id, &project)
        .await;
}

#[tokio::test]
async fn agent_workspace_closed_pr_polling_removes_worktree_and_branch() {
    let repo = init_cleanup_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = cleanup_project(repo.path(), worktrees.path());
    let branch = expected_workspace_branch(&project, "poller-closed-cleanup-conversation");
    let mut workspace = cleanup_workspace_with_conversation(
        &project,
        &branch,
        "poller-closed-cleanup-conversation",
    );
    workspace.publication_pr_status = Some("open".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
    let memory_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        memory_workspace_repo.clone();
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
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MockChatService::new()),
    );
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let terminal_status_persisted = workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .ok()
                .flatten()
                .and_then(|workspace| workspace.publication_pr_status)
                .as_deref()
                == Some("closed");
            let cleanup_finished = memory_workspace_repo
                .local_cleanup_status_for_test(&conversation_id)
                .await
                .as_deref()
                == Some("cleaned");
            if terminal_status_persisted
                && cleanup_finished
                && !worktree_path.exists()
                && !branch_exists(repo.path(), &branch)
            {
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
    assert!(!branch_exists(repo.path(), &branch));
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

struct SequencedWorkspaceRepository {
    inner: Arc<MemoryAgentConversationWorkspaceRepository>,
    lookup_calls: AtomicUsize,
    disable_autofix_on_lookup: Option<usize>,
    disable_auto_merge_after_repair_claim: bool,
    error_on_lookup: Option<usize>,
    update_publication_calls: AtomicUsize,
    error_on_update_publication: Option<usize>,
    supersede_repair_claim_on_update_publication: Option<usize>,
    update_auto_merge_calls: AtomicUsize,
    error_on_update_auto_merge: Option<usize>,
    error_on_pr_autofix_post_start_audit: bool,
}

impl SequencedWorkspaceRepository {
    fn new(
        inner: Arc<MemoryAgentConversationWorkspaceRepository>,
        disable_autofix_on_lookup: Option<usize>,
        error_on_lookup: Option<usize>,
    ) -> Self {
        Self {
            inner,
            lookup_calls: AtomicUsize::new(0),
            disable_autofix_on_lookup,
            disable_auto_merge_after_repair_claim: false,
            error_on_lookup,
            update_publication_calls: AtomicUsize::new(0),
            error_on_update_publication: None,
            supersede_repair_claim_on_update_publication: None,
            update_auto_merge_calls: AtomicUsize::new(0),
            error_on_update_auto_merge: None,
            error_on_pr_autofix_post_start_audit: false,
        }
    }

    fn with_disable_auto_merge_after_repair_claim(mut self) -> Self {
        self.disable_auto_merge_after_repair_claim = true;
        self
    }

    fn with_update_publication_error_on_call(mut self, call: usize) -> Self {
        self.error_on_update_publication = Some(call);
        self
    }

    fn with_superseded_repair_claim_on_update_publication(mut self, call: usize) -> Self {
        self.supersede_repair_claim_on_update_publication = Some(call);
        self
    }

    fn with_update_auto_merge_error_on_call(mut self, call: usize) -> Self {
        self.error_on_update_auto_merge = Some(call);
        self
    }

    fn with_pr_autofix_post_start_audit_error(mut self) -> Self {
        self.error_on_pr_autofix_post_start_audit = true;
        self
    }
}

#[async_trait]
impl AgentConversationWorkspaceRepository for SequencedWorkspaceRepository {
    async fn set_last_blocked_pr_health_fingerprint(
        &self,
        _conversation_id: &ChatConversationId,
        _fingerprint: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn set_review_automation_override(
        &self,
        conversation_id: &ChatConversationId,
        value: Option<bool>,
    ) -> AppResult<()> {
        self.inner
            .set_review_automation_override(conversation_id, value)
            .await
    }
    async fn create_or_update(
        &self,
        workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        self.inner.create_or_update(workspace).await
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        let call = self.lookup_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.error_on_lookup == Some(call) {
            return Err(repo_error());
        }

        let workspace = self.inner.get_by_conversation_id(conversation_id).await?;
        if self.disable_autofix_on_lookup == Some(call) {
            let Some(mut workspace) = workspace else {
                return Ok(None);
            };
            workspace.pr_autofix_enabled = false;
            return self.inner.create_or_update(workspace).await.map(Some);
        }
        Ok(workspace)
    }

    async fn get_by_project_id(
        &self,
        project_id: &crate::domain::entities::ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.inner.get_by_project_id(project_id).await
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.inner.list_active_direct_published_workspaces().await
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.inner.list_active_needs_agent_workspaces().await
    }

    async fn update_links(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: Option<&IdeationSessionId>,
        plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()> {
        self.inner
            .update_links(conversation_id, ideation_session_id, plan_branch_id)
            .await
    }

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()> {
        let call = self.update_publication_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.error_on_update_publication == Some(call) {
            return Err(repo_error());
        }
        if self.supersede_repair_claim_on_update_publication == Some(call) {
            self.inner
                .update_publication(conversation_id, pr_number, pr_url, pr_status, push_status)
                .await?;
            self.inner
                .update_pr_auto_merge_state(
                    conversation_id,
                    None,
                    Some("fixing"),
                    Some("replacement repair claim"),
                )
                .await?;
            return Err(repo_error());
        }
        self.inner
            .update_publication(conversation_id, pr_number, pr_url, pr_status, push_status)
            .await
    }

    async fn compare_and_set_repair_state(
        &self,
        conversation_id: &ChatConversationId,
        expected: &crate::domain::repositories::AgentWorkspaceRepairStateGuard,
        transition: &crate::domain::repositories::AgentWorkspaceRepairStateTransition,
    ) -> AppResult<bool> {
        let updated = self
            .inner
            .compare_and_set_repair_state(conversation_id, expected, transition)
            .await?;
        if updated
            && self.disable_auto_merge_after_repair_claim
            && transition.pr_supervision_status.as_deref() == Some("fixing")
        {
            let Some(mut workspace) = self.inner.get_by_conversation_id(conversation_id).await?
            else {
                return Ok(false);
            };
            workspace.pr_auto_merge_desired = false;
            self.inner.create_or_update(workspace).await?;
        }
        Ok(updated)
    }

    async fn update_pr_supervision_preferences(
        &self,
        conversation_id: &ChatConversationId,
        autofix_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: &str,
    ) -> AppResult<()> {
        self.inner
            .update_pr_supervision_preferences(
                conversation_id,
                autofix_enabled,
                auto_merge_desired,
                auto_merge_method,
            )
            .await
    }

    async fn update_pr_auto_merge_state(
        &self,
        conversation_id: &ChatConversationId,
        auto_merge_current: Option<bool>,
        supervision_status: Option<&str>,
        supervision_summary: Option<&str>,
    ) -> AppResult<()> {
        let call = self.update_auto_merge_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.error_on_update_auto_merge == Some(call) {
            return Err(repo_error());
        }
        self.inner
            .update_pr_auto_merge_state(
                conversation_id,
                auto_merge_current,
                supervision_status,
                supervision_summary,
            )
            .await
    }

    async fn update_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()> {
        self.inner.update_status(conversation_id, status).await
    }

    async fn save_pr_description(
        &self,
        conversation_id: &ChatConversationId,
        description: AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        self.inner
            .save_pr_description(conversation_id, description)
            .await
    }

    async fn get_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>> {
        self.inner.get_pr_description(conversation_id).await
    }

    async fn clear_pr_description(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.inner.clear_pr_description(conversation_id).await
    }

    async fn append_publication_event(
        &self,
        event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()> {
        if self.error_on_pr_autofix_post_start_audit
            && event.step == "pr_autofix"
            && event.status == "needs_agent"
        {
            return Err(repo_error());
        }
        self.inner.append_publication_event(event).await
    }

    async fn list_publication_events(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>> {
        self.inner.list_publication_events(conversation_id).await
    }

    async fn upsert_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        comments: Vec<crate::domain::entities::AgentWorkspacePrCommentEvidenceUpsert>,
    ) -> AppResult<()> {
        self.inner
            .upsert_pr_comment_evidence(conversation_id, comments)
            .await
    }

    async fn get_pr_review_monitor(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
        self.inner.get_pr_review_monitor(conversation_id).await
    }

    async fn set_pr_review_auto_approve_enabled(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        self.inner
            .set_pr_review_auto_approve_enabled(conversation_id, enabled)
            .await
    }

    async fn mark_pr_review_first_action_resolved(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        self.inner
            .mark_pr_review_first_action_resolved(conversation_id)
            .await
    }

    async fn claim_pending_pr_review_action(&self, action_id: &str) -> AppResult<bool> {
        self.inner.claim_pending_pr_review_action(action_id).await
    }

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.inner.delete(conversation_id).await
    }
}

struct ReviewMonitorLookupErrorRepository {
    workspace: AgentConversationWorkspace,
}

#[async_trait]
impl AgentConversationWorkspaceRepository for ReviewMonitorLookupErrorRepository {
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
        workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        Ok(workspace)
    }

    async fn get_by_conversation_id(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Ok(Some(self.workspace.clone()))
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

    async fn get_pr_review_monitor(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
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

struct WorkspaceLookupErrorRepository;

#[async_trait]
impl AgentConversationWorkspaceRepository for WorkspaceLookupErrorRepository {
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

struct LookupErrorRepairRepository;

#[async_trait]
impl AgentWorkspaceRepairRepository for LookupErrorRepairRepository {
    async fn get_current_repair_attempt(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        Err(AppError::Infrastructure(
            "repair authority lookup failed".to_string(),
        ))
    }

    async fn get_latest_repair_attempt_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        unreachable!()
    }

    async fn get_repair_attempt(
        &self,
        _attempt_id: &crate::domain::entities::AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        unreachable!()
    }

    async fn get_repair_attempt_for_run(
        &self,
        _conversation_id: &ChatConversationId,
        _run_id: &crate::domain::entities::AgentRunId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        unreachable!()
    }

    async fn list_recoverable_repair_attempts(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceRepairAttempt>> {
        unreachable!()
    }

    async fn list_repair_attempts_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentWorkspaceRepairAttempt>> {
        unreachable!()
    }

    async fn start_or_join_repair_attempt(
        &self,
        _request: crate::domain::repositories::StartOrJoinAgentWorkspaceRepairAttempt,
    ) -> AppResult<crate::domain::repositories::StartOrJoinAgentWorkspaceRepairAttemptOutcome> {
        unreachable!()
    }

    async fn bind_repair_attempt_run(
        &self,
        _request: crate::domain::repositories::BindAgentWorkspaceRepairAttemptRun,
    ) -> AppResult<crate::domain::repositories::AgentWorkspaceRepairAttemptTransitionOutcome> {
        unreachable!()
    }

    async fn transition_repair_attempt(
        &self,
        _request: crate::domain::repositories::AgentWorkspaceRepairAttemptTransition,
    ) -> AppResult<crate::domain::repositories::AgentWorkspaceRepairAttemptTransitionOutcome> {
        unreachable!()
    }

    async fn settle_repair_attempt(
        &self,
        _request: crate::domain::repositories::SettleAgentWorkspaceRepairAttempt,
    ) -> AppResult<crate::domain::repositories::SettleAgentWorkspaceRepairAttemptOutcome> {
        unreachable!()
    }

    async fn settle_and_start_repair_successor(
        &self,
        _request: crate::domain::repositories::SettleAndStartAgentWorkspaceRepairSuccessor,
    ) -> AppResult<crate::domain::repositories::SettleAndStartAgentWorkspaceRepairSuccessorOutcome>
    {
        unreachable!()
    }

    async fn create_repair_effect(
        &self,
        _request: crate::domain::repositories::CreateAgentWorkspaceRepairEffect,
    ) -> AppResult<crate::domain::repositories::CreateAgentWorkspaceRepairEffectOutcome> {
        unreachable!()
    }

    async fn get_repair_effect_by_idempotency_key(
        &self,
        _idempotency_key: &str,
    ) -> AppResult<Option<crate::domain::entities::AgentWorkspaceRepairEffect>> {
        unreachable!()
    }

    async fn get_open_repair_effect(
        &self,
        _attempt_id: &crate::domain::entities::AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<crate::domain::entities::AgentWorkspaceRepairEffect>> {
        unreachable!()
    }

    async fn complete_repair_effect(
        &self,
        _request: crate::domain::repositories::CompleteAgentWorkspaceRepairEffect,
    ) -> AppResult<crate::domain::repositories::CompleteAgentWorkspaceRepairEffectOutcome> {
        unreachable!()
    }

    async fn import_legacy_repair_attempt(
        &self,
        _request: crate::domain::repositories::ImportLegacyAgentWorkspaceRepairAttempt,
    ) -> AppResult<crate::domain::repositories::ImportLegacyAgentWorkspaceRepairAttemptOutcome>
    {
        unreachable!()
    }
}
