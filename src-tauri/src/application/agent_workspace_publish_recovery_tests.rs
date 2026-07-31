use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_recovery::{
    evaluate_pr_autofix_successor, is_blocked_and_not_auto_retryable,
    recover_agent_workspace_repair_after_terminal_run,
    recover_agent_workspace_repair_attempts_for_state,
    recover_stale_agent_workspace_publish_repairs,
    recover_stale_agent_workspace_publish_repairs_for_state,
    recover_stale_agent_workspace_publish_repairs_on_startup,
    recover_stale_agent_workspace_publish_repairs_on_startup_for_state,
    recover_stale_publish_repair_for_workspace,
    recover_stale_publish_repair_for_workspace_and_reload,
    recover_stale_publish_repair_for_workspace_and_reload_with_review_target,
    recover_stale_publish_repair_for_workspace_in_state,
    recover_stale_publish_repair_for_workspace_with_project_repo_outcome,
    recover_stale_transient_publish_statuses, PrAutofixSuccessorDecision,
    StalePublishRepairRecoveryOutcome, STALE_NEEDS_AGENT_CLASSIFICATION,
    STALE_REPAIR_BLOCKED_SUMMARY, STALE_REPAIR_RECOVERED_STEP, STALE_TRANSIENT_CLASSIFICATION,
    STALE_TRANSIENT_RECOVERED_STEP,
};
use crate::application::agent_workspace_publish_repair_state::{
    reserve_agent_workspace_repair_dispatch, start_or_join_agent_workspace_repair,
    AgentWorkspaceRepairDispatchOutcome, AgentWorkspaceRepairStartOutcome,
    AgentWorkspaceRepairStartRequest, MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES,
};
use crate::application::agent_workspace_review::{
    AgentWorkspaceReviewPacket, AgentWorkspaceReviewTarget,
};
use crate::application::publish_resilience::try_acquire_agent_workspace_repair_publish_continuation_guard;
use crate::application::{AppState, GitService};
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, AgentRunActionKind, AgentRunId,
    AgentRunStatus, AgentWorkspaceRepairAttempt, AgentWorkspaceRepairContinuation,
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairOutcome,
    AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversation, ChatConversationId,
    GitTargetIdentity, GitTargetLeaseOwner, IdeationAnalysisBaseRefKind, IdeationSessionId,
    PlanBranch, Project, ProjectId,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentConversationWorkspaceRepository,
    AgentRunRepository, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, CreateAgentWorkspaceRepairEffect,
    CreateAgentWorkspaceRepairEffectOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentProviderSettingsRepository,
    MemoryAgentRunRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn conversation_id(suffix: u8) -> ChatConversationId {
    ChatConversationId::from_string(format!("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbb{suffix:02}"))
}

fn project_id() -> ProjectId {
    ProjectId::from_string("project-publish-recovery".to_string())
}

#[test]
fn blocked_repair_is_exhausted_only_for_spent_delivery_or_automatic_successor_budget() {
    let now = chrono::Utc::now();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(91),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        now,
    );
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.dispatch_count = MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES;
    attempt.blocker = Some("delivery retries exhausted".to_string());
    assert!(is_blocked_and_not_auto_retryable(&attempt));

    attempt.dispatch_count = 0;
    attempt.next_dispatch_at = Some(now + chrono::Duration::seconds(60));
    assert!(!is_blocked_and_not_auto_retryable(&attempt));

    attempt.next_dispatch_at = None;
    attempt.pending_reasons = vec!["auto_retry_blocked_repair:3".to_string()];
    assert!(is_blocked_and_not_auto_retryable(&attempt));

    attempt.phase = AgentWorkspaceRepairPhase::Requested;
    assert!(!is_blocked_and_not_auto_retryable(&attempt));
}

#[cfg(unix)]
#[tokio::test]
async fn needs_human_blocker_is_exempt_from_automatic_repair_reconciliation() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(119, "#!/bin/sh\nexit 1\n").await;
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load seeded repair")
        .expect("seeded repair exists");
    let mut needs_human = current.clone();
    needs_human.source = AgentWorkspaceRepairSource::PrAutofix;
    needs_human.phase = AgentWorkspaceRepairPhase::Blocked;
    needs_human.blocker = Some("A maintainer must approve this change.".to_string());
    needs_human.pending_reasons = vec![
        crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON
            .to_string(),
    ];
    needs_human.updated_at = chrono::Utc::now() - chrono::Duration::seconds(61);
    let needs_human = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: needs_human,
            expected_phase: current.phase,
            expected_updated_at: current.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist needs-human completion marker")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("needs-human marker must apply, got {outcome:?}"),
    };

    assert!(is_blocked_and_not_auto_retryable(&needs_human));
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("needs-human recovery sweep"),
        0,
        "needs-human repairs must never redispatch automatically"
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load post-recovery repair")
        .expect("needs-human repair remains current");
    assert_eq!(current.id, needs_human.id);
    assert_eq!(current.generation, needs_human.generation);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
}

#[cfg(unix)]
struct TestEnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

#[cfg(unix)]
impl TestEnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

#[cfg(unix)]
impl Drop for TestEnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

/// A timestamp old enough that a reservation without a run row is a genuine interrupted delivery
/// rather than a dispatch whose run row has not been written yet.
fn aged_past_spawn_grace() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
        - chrono::Duration::seconds(
            crate::application::agent_workspace_publish_repair_state::ORPHANED_REPAIR_DISPATCH_RESCUE_GRACE_SECS
                + 60,
        )
}

/// Ages the current attempt past the spawn-grace window without changing anything else about it.
async fn age_current_repair_attempt_past_spawn_grace(
    state: &AppState,
    conversation_id: &ChatConversationId,
) {
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load attempt to age")
        .expect("attempt exists to age");
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.updated_at = aged_past_spawn_grace();
    let outcome = state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("age repair attempt");
    assert!(matches!(
        outcome,
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
}

fn needs_agent_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/publish-recovery".to_string(),
        "/tmp/ralphx-test-publish-recovery".to_string(),
    );
    workspace.publication_pr_number = Some(684);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/684".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace
}

fn recovery_git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
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

#[cfg(unix)]
async fn seed_orphaned_repair_dispatch(
    suffix: u8,
    cli_script: &str,
) -> (
    AppState,
    ChatConversationId,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let mut state = AppState::new_test();
    let worktree_parent = tempfile::tempdir().expect("create orphaned repair worktree parent");
    let project_dir = tempfile::tempdir().expect("create orphaned repair project directory");
    recovery_git(project_dir.path(), &["init", "-b", "main"]);
    recovery_git(
        project_dir.path(),
        &["config", "user.email", "recovery@example.com"],
    );
    recovery_git(
        project_dir.path(),
        &["config", "user.name", "Recovery Test"],
    );
    std::fs::write(project_dir.path().join("README.md"), "base\n").expect("write base file");
    recovery_git(project_dir.path(), &["add", "README.md"]);
    recovery_git(project_dir.path(), &["commit", "-m", "base"]);
    let cli_path = project_dir.path().join("fake-claude");
    std::fs::write(&cli_path, cli_script).expect("write fake repair CLI");
    std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))
        .expect("make fake repair CLI executable");
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut provider = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    provider.enabled = true;
    provider.is_default = true;
    provider.custom_binary_enabled = true;
    provider.custom_binary_path = Some(cli_path.display().to_string());
    state
        .agent_provider_settings_repo
        .upsert(&provider)
        .await
        .expect("enable fake Claude provider");
    let conversation_id = conversation_id(suffix);
    let mut project = Project::new(
        "orphaned repair recovery project".to_string(),
        project_dir.path().display().to_string(),
    );
    project.id = project_id();
    project.worktree_parent_directory = Some(worktree_parent.path().display().to_string());
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("derive exact orphaned workspace path");
    recovery_git(
        project_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            "ralphx/test/publish-recovery",
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    state
        .project_repo
        .create(project)
        .await
        .expect("seed orphaned repair project");
    let mut conversation = ChatConversation::new_project(project_id());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed orphaned repair conversation");
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.worktree_path = workspace_path.display().to_string();
    workspace.base_commit = Some(recovery_git(project_dir.path(), &["rev-parse", "HEAD"]));
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed orphaned repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "orphaned first dispatch".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start orphaned repair");
    assert!(matches!(
        started,
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(_)
    ));
    (state, conversation_id, worktree_parent, project_dir)
}

async fn age_requested_repair_attempt(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> AgentWorkspaceRepairAttempt {
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load requested orphan")
        .expect("requested orphan exists");
    let expected_updated_at = attempt.updated_at;
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(61);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Requested,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("age requested orphan")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("requested orphan aging must apply, got {outcome:?}"),
    }
}

async fn block_repair_attempt_after(
    state: &AppState,
    conversation_id: &ChatConversationId,
    expected_phase: AgentWorkspaceRepairPhase,
    elapsed_secs: i64,
) -> AgentWorkspaceRepairAttempt {
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load repair attempt to block")
        .expect("repair attempt exists to block");
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.blocker = Some("automatic blocked-repair recovery fixture".to_string());
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(elapsed_secs);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("block repair attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("blocking repair attempt must apply, got {outcome:?}"),
    }
}

#[cfg(unix)]
async fn block_push_handoff_base_advanced_repair(
    state: &AppState,
    conversation_id: &ChatConversationId,
    project_dir: &std::path::Path,
    retry_streak: u32,
) -> (AgentWorkspaceRepairAttempt, String, String) {
    let stale_base_commit = recovery_git(project_dir, &["rev-parse", "main"]);
    std::fs::write(project_dir.join("base-advanced.md"), "fresh base\n")
        .expect("write fresh base fixture");
    recovery_git(project_dir, &["add", "base-advanced.md"]);
    recovery_git(project_dir, &["commit", "-m", "advance repair base"]);
    let fresh_base_commit = recovery_git(project_dir, &["rev-parse", "main"]);
    assert_ne!(fresh_base_commit, stale_base_commit);

    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .expect("load workspace whose base advanced")
        .expect("workspace whose base advanced exists");
    workspace.base_commit = Some(fresh_base_commit.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist fresh workspace base");

    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load repair attempt to block at push handoff")
        .expect("repair attempt exists to block at push handoff");
    let expected_updated_at = attempt.updated_at;
    let blocker = format!(
        "workspace repair push handoff base advanced from '{stale_base_commit}' to '{fresh_base_commit}'"
    );
    attempt.source = AgentWorkspaceRepairSource::PrAutofix;
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.target_base_commit = Some(stale_base_commit.clone());
    attempt.summary = Some(blocker.clone());
    attempt.blocker = Some(blocker);
    attempt.pending_reasons = (retry_streak > 0)
        .then(|| format!("auto_retry_blocked_repair:{retry_streak}"))
        .into_iter()
        .collect();
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1_000);
    let blocked = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("record push-handoff base-advanced blocker")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("push-handoff blocker transition must apply, got {outcome:?}"),
    };
    (blocked, stale_base_commit, fresh_base_commit)
}

async fn park_repair_attempt_ready_after(
    state: &AppState,
    conversation_id: &ChatConversationId,
    expected_phase: AgentWorkspaceRepairPhase,
    elapsed_secs: i64,
) -> AgentWorkspaceRepairAttempt {
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load repair attempt to park")
        .expect("repair attempt exists to park");
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Ready;
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(elapsed_secs);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("park repair attempt at ready")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("parking repair attempt must apply, got {outcome:?}"),
    }
}

fn review_target() -> AgentWorkspaceReviewTarget {
    AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        base_ref: "main".to_string(),
        base_sha: Some("base-sha".to_string()),
        head_ref: "ralphx/test/publish-recovery".to_string(),
        head_sha: Some("head-current".to_string()),
        diff_fingerprint: "diff-current".to_string(),
        working_directory: PathBuf::from("/tmp/ralphx-test-publish-recovery"),
        source_pull_request_number: None,
        review_packet: AgentWorkspaceReviewPacket::default(),
    }
}

fn reviewing_monitor(
    conversation_id: ChatConversationId,
    target: &AgentWorkspaceReviewTarget,
) -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor
}

fn stale_passed_monitor(
    conversation_id: ChatConversationId,
    target: &AgentWorkspaceReviewTarget,
) -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-stale"));
    monitor.reviewed_target_scope = Some(target.scope);
    monitor.reviewed_head_sha = Some("old-head".to_string());
    monitor.reviewed_diff_fingerprint = Some("old-diff".to_string());
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor
}

async fn seed_terminal_run(
    agent_run_repo: &dyn AgentRunRepository,
    conversation_id: ChatConversationId,
) {
    let run = agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed run");
    agent_run_repo
        .fail(&run.id, "agent repair exited")
        .await
        .expect("mark run failed");
}

async fn seed_failed_pr_autofix_run(
    agent_run_repo: &dyn AgentRunRepository,
    conversation_id: ChatConversationId,
    fingerprint: &str,
) {
    let mut run = AgentRun::new(conversation_id);
    run.action_kind = Some(AgentRunActionKind::PrAutofix);
    run.action_context_id = Some("684".to_string());
    run.action_target_id = Some(fingerprint.to_string());
    let run = agent_run_repo.create(run).await.expect("seed autofix run");
    agent_run_repo
        .fail(&run.id, "autofix interrupted")
        .await
        .expect("mark autofix failed");
}

#[tokio::test]
async fn startup_recovery_wrappers_finish_on_empty_repositories() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());

    recover_stale_agent_workspace_publish_repairs_on_startup(
        workspace_repo as Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo as Arc<dyn AgentRunRepository>,
    )
    .await;
    recover_stale_agent_workspace_publish_repairs_on_startup_for_state(&AppState::new_test()).await;
}

#[tokio::test]
async fn terminal_run_hints_without_an_exact_repair_reservation_are_ignored() {
    let state = AppState::new_test();

    assert!(!recover_agent_workspace_repair_after_terminal_run(
        &state,
        &conversation_id(97),
        &AgentRunId::from_string("unreserved-terminal-run"),
    )
    .await
    .expect("an unreserved terminal hint is a harmless no-op"));
}

#[tokio::test]
async fn recovery_ignores_nonterminal_run_hints_and_blocks_exhausted_ownerless_dispatches() {
    let state = AppState::new_test();
    let live_conversation = conversation_id(87);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(live_conversation.clone()))
        .await
        .expect("seed live-hint workspace");
    let live_run = state
        .agent_run_repo
        .create(AgentRun::new(live_conversation.clone()))
        .await
        .expect("seed nonterminal run");
    let live_attempt = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                live_conversation.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "nonterminal hint".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start live-hint repair");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(live_attempt) = live_attempt else {
        panic!("first live-hint attempt must start");
    };
    let bound = state
        .agent_workspace_repair_repo
        .bind_repair_attempt_run(
            crate::domain::repositories::BindAgentWorkspaceRepairAttemptRun {
                attempt_id: live_attempt.id,
                generation: live_attempt.generation,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at: live_attempt.updated_at,
                run_id: live_run.id.clone(),
                runtime_conversation_id: None,
                updated_at: live_attempt.updated_at + chrono::Duration::microseconds(1),
            },
        )
        .await
        .expect("bind nonterminal run");
    assert!(matches!(
        bound,
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
    assert!(!recover_agent_workspace_repair_after_terminal_run(
        &state,
        &live_conversation,
        &live_run.id,
    )
    .await
    .expect("nonterminal notification is ignored"));

    let exhausted_conversation = conversation_id(88);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(exhausted_conversation.clone()))
        .await
        .expect("seed exhausted-dispatch workspace");
    let exhausted = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                exhausted_conversation.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "exhausted dispatch".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start exhausted dispatch");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut exhausted) = exhausted else {
        panic!("first exhausted dispatch must start");
    };
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/exhausted-ownerless-dispatch"),
        "refs/heads/ralphx/exhausted-ownerless-dispatch",
    )
    .expect("canonical exhausted dispatch target");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(exhausted.id.as_str());
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner,
        })
        .await
        .expect("acquire exhausted dispatch target")
    else {
        panic!("exhausted dispatch should acquire a new target lease");
    };
    let expected_updated_at = exhausted.updated_at;
    exhausted.phase = AgentWorkspaceRepairPhase::Dispatching;
    exhausted.dispatch_count = MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES;
    exhausted.git_common_dir = Some(
        target_identity
            .git_common_dir()
            .to_string_lossy()
            .into_owned(),
    );
    exhausted.target_ref = Some(target_identity.full_ref().to_string());
    exhausted.target_identity_version = Some(
        crate::application::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION,
    );
    exhausted.target_lease_epoch = Some(fencing_epoch);
    // Aged past the spawn-grace window: this fixture is an ownerless dispatch left behind by a
    // dead process, not a reservation whose run row has simply not been written yet.
    exhausted.updated_at = aged_past_spawn_grace();
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt: exhausted,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at,
                next_phase: AgentWorkspaceRepairPhase::Dispatching,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("persist exhausted ownerless dispatch"),
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recover ownerless exhausted dispatch"),
        1
    );
    let blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&exhausted_conversation)
        .await
        .expect("load exhausted repair")
        .expect("exhausted repair remains actionable");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(blocked
        .blocker
        .as_deref()
        .is_some_and(|blocker| blocker.contains("retries are exhausted")));
}

#[tokio::test]
async fn startup_recovery_blocks_unprovable_validation_and_manual_continuation() {
    let state = AppState::new_test();

    for (suffix, phase, continuation, expected_blocker) in [
        (
            97,
            AgentWorkspaceRepairPhase::Validating,
            AgentWorkspaceRepairContinuation::Publish,
            "lost canonical Git target authority",
        ),
        (
            98,
            AgentWorkspaceRepairPhase::ContinuationPending,
            AgentWorkspaceRepairContinuation::Manual,
            "could not prove a publish runtime",
        ),
    ] {
        let conversation_id = conversation_id(suffix);
        state
            .agent_conversation_workspace_repo
            .create_or_update(needs_agent_workspace(conversation_id.clone()))
            .await
            .expect("seed canonical recovery workspace");
        let started = state
            .agent_workspace_repair_repo
            .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: AgentWorkspaceRepairAttempt::new(
                    conversation_id.clone(),
                    AgentWorkspaceRepairSource::Publish,
                    continuation,
                    "main",
                    false,
                    true,
                    false,
                    None,
                    chrono::Utc::now(),
                ),
                reason: "recover an interrupted durable phase".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("start durable recovery attempt");
        let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut attempt) = started else {
            panic!("first durable repair attempt must start");
        };
        let expected_updated_at = attempt.updated_at;
        attempt.phase = phase;
        attempt.updated_at += chrono::Duration::microseconds(1);
        let transitioned = state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at,
                next_phase: phase,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("persist interrupted recovery phase");
        assert!(matches!(
            transitioned,
            AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
        ));

        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("reconcile interrupted durable phase"),
            1
        );
        let blocked = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load reconciled repair")
            .expect("blocked repair remains actionable");
        assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
        assert!(
            blocked
                .blocker
                .as_deref()
                .is_some_and(|blocker| blocker.contains(expected_blocker)),
            "unexpected blocker: {:?}",
            blocked.blocker
        );
    }
}

#[tokio::test]
async fn startup_recovery_leaves_validating_attempt_owned_by_an_active_run_unchanged() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(99);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed validating workspace");
    let run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("create active validation owner");
    state
        .agent_run_repo
        .update_status(&run.id, AgentRunStatus::Running)
        .await
        .expect("mark validation owner active");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "active validating repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start validating repair");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut attempt) = started else {
        panic!("first repair attempt must start");
    };
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Validating;
    attempt.reserved_agent_run_id = Some(run.id);
    attempt.updated_at += chrono::Duration::microseconds(1);
    let validating = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Validating,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist active validating repair")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected validating repair, got {outcome:?}"),
    };

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("active validation recovery is a no-op"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load active validating repair")
        .expect("attempt remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Validating);
    assert_eq!(current.updated_at, validating.updated_at);
}

#[tokio::test]
async fn startup_recovery_revalidates_a_clean_committed_validating_repair() {
    let state = AppState::new_test();
    let repo = tempfile::tempdir().expect("create recovery repository");
    let worktrees = tempfile::tempdir().expect("create recovery worktree parent");
    recovery_git(repo.path(), &["init", "-b", "main"]);
    recovery_git(repo.path(), &["config", "user.email", "test@example.com"]);
    recovery_git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    recovery_git(repo.path(), &["add", "README.md"]);
    recovery_git(repo.path(), &["commit", "-m", "base"]);
    let base_commit = recovery_git(repo.path(), &["rev-parse", "HEAD"]);

    let mut project = Project::new(
        "Recovery validation".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = project_id();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let conversation_id = conversation_id(100);
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("resolve canonical recovery workspace path");
    recovery_git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            "ralphx/recovery-validation",
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    std::fs::write(workspace_path.join("repair.md"), "clean repair\n").expect("write repair file");
    recovery_git(&workspace_path, &["add", "repair.md"]);
    recovery_git(&workspace_path, &["commit", "-m", "repair"]);

    state
        .review_settings_repo
        .update_settings(&crate::domain::review::ReviewSettings {
            require_workspace_review: false,
            ..crate::domain::review::ReviewSettings::default()
        })
        .await
        .expect("disable workspace review policy for revalidation fixture");
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.project_id = project.id;
    workspace.worktree_path = workspace_path.to_string_lossy().to_string();
    workspace.branch_name = "ralphx/recovery-validation".to_string();
    workspace.auto_publish_enabled = true;
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.pr_auto_merge_current = None;
    workspace.pr_autofix_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed recovery workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::BaseUpdate,
                AgentWorkspaceRepairContinuation::UpdateOnly,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "recover clean validating repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start recovery attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut attempt) = started else {
        panic!("first recovery attempt must start");
    };
    let target_identity =
        GitService::canonical_target_identity(repo.path(), "ralphx/recovery-validation")
            .await
            .expect("resolve canonical recovery target");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner,
        })
        .await
        .expect("acquire recovery target lease")
    else {
        panic!("recovery target lease must be newly acquired");
    };
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Validating;
    attempt.target_base_commit = Some(base_commit);
    attempt.git_common_dir = Some(
        target_identity
            .git_common_dir()
            .to_string_lossy()
            .into_owned(),
    );
    attempt.target_ref = Some(target_identity.full_ref().to_string());
    attempt.target_identity_version = Some(
        crate::application::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION,
    );
    attempt.target_lease_epoch = Some(fencing_epoch);
    attempt.updated_at += chrono::Duration::microseconds(1);
    let validating = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Validating,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint interrupted validation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected validating repair, got {outcome:?}"),
    };
    let _ = validating;

    let repair_head = recovery_git(&workspace_path, &["rev-parse", "HEAD"]);
    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("recover clean committed validation");
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load recovered repair")
        .expect("recovered repair stays current for continuation reconciliation");
    assert_ne!(
        current.phase,
        AgentWorkspaceRepairPhase::Blocked,
        "clean committed validating repair must not re-block: {current:?}"
    );
    assert_eq!(
        current.phase,
        AgentWorkspaceRepairPhase::Ready,
        "clean update-only revalidation parks the repaired workspace at Ready"
    );
    assert_eq!(
        current.repair_head_commit.as_deref(),
        Some(repair_head.as_str()),
        "revalidation records the exact committed repair head"
    );
    assert!(
        current.blocker.is_none(),
        "no blocker after clean revalidation: {current:?}"
    );
    assert!(current.settled_at.is_none());
}

/// Production incident 2026-07-31: a live PR-fixer dispatch was settled `interrupted` 43 ms after
/// spawn because the reservation is written before the agent run row exists. The reservation alone
/// is not evidence of a dead worker, so a just-reserved dispatch must survive an immediate pass.
#[tokio::test]
async fn fresh_dispatch_reservation_is_not_settled_as_interrupted_before_its_run_row_exists() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(124);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed fresh dispatch workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "fresh dispatch".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start fresh repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("fresh durable repair attempt must start");
    };
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-fresh-dispatch"),
        "refs/heads/ralphx/test/publish-recovery",
    )
    .expect("valid canonical target identity");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        started,
        AgentRunId::from_string("fresh-dispatch-run"),
        None,
        "dispatch fresh repair",
        None,
    )
    .await
    .expect("reserve fresh repair delivery");
    assert!(matches!(
        dispatch,
        AgentWorkspaceRepairDispatchOutcome::Reserved(_)
    ));

    // The agent is spawning right now; its run row has not been written yet.
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recovery pass over a just-reserved dispatch"),
        0
    );
    let held = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load fresh dispatch")
        .expect("fresh dispatch remains current");
    assert_eq!(
        held.phase,
        AgentWorkspaceRepairPhase::Dispatching,
        "a spawning dispatch must not be settled as interrupted"
    );
    assert_eq!(held.dispatch_count, 0, "no retry was consumed");
    assert!(
        held.next_dispatch_at.is_none(),
        "no duplicate delivery queued"
    );
    assert_eq!(
        held.reserved_agent_run_id,
        Some(AgentRunId::from_string("fresh-dispatch-run")),
        "the original reservation still owns the dispatch"
    );
    assert!(
        !state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("load publication events")
            .iter()
            .any(|event| event.step == "repair_sent" && event.status == "retrying"),
        "no interrupted-retry event may be emitted inside the spawn grace window"
    );

    // Past the grace window the same reservation is a genuine orphan and settles as before.
    age_current_repair_attempt_past_spawn_grace(&state, &conversation_id).await;
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recover the aged orphaned dispatch"),
        1
    );
    let retried = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load retried dispatch")
        .expect("retried dispatch remains current");
    assert_eq!(retried.phase, AgentWorkspaceRepairPhase::Requested);
    assert_eq!(retried.dispatch_count, 1);
    assert!(retried.reserved_agent_run_id.is_none());
}

#[tokio::test]
async fn startup_recovery_schedules_one_due_retry_for_an_interrupted_repair_delivery() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(93);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed durable repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "interrupted delivery".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair attempt must start");
    };
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-recovery-dispatch"),
        "refs/heads/ralphx/test/publish-recovery",
    )
    .expect("valid canonical target identity");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        started,
        AgentRunId::from_string("interrupted-repair-delivery-run"),
        None,
        "dispatch durable repair",
        None,
    )
    .await
    .expect("reserve interrupted repair delivery");
    assert!(matches!(
        dispatch,
        AgentWorkspaceRepairDispatchOutcome::Reserved(_)
    ));
    // Startup recovery runs after the process that owned this dispatch died, so the reservation is
    // well past the spawn-grace window that protects a just-reserved delivery.
    age_current_repair_attempt_past_spawn_grace(&state, &conversation_id).await;

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("startup recovery schedules due repair retry"),
        1
    );
    let scheduled = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load scheduled repair retry")
        .expect("repair remains current");
    assert_eq!(scheduled.phase, AgentWorkspaceRepairPhase::Requested);
    assert_eq!(scheduled.dispatch_count, 1);
    assert!(scheduled.next_dispatch_at.is_some());
    assert!(scheduled.reserved_agent_run_id.is_none());
    let retry_events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load recovery retry events");
    assert_eq!(
        retry_events
            .iter()
            .filter(|event| event.step == "repair_sent" && event.status == "retrying")
            .count(),
        1
    );

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("not-due startup replay is harmless"),
        0
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("reload retry events"),
        retry_events,
        "not-due restart recovery must not dispatch or emit a duplicate repair message"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn due_startup_recovery_redelivers_once_and_binds_the_replacement_run() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let mut state = AppState::new_test();
    let worktree_parent = tempfile::tempdir().expect("create repair worktree parent");
    let project_dir = tempfile::tempdir().expect("create repair project directory");
    let cli_path = project_dir.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"durable-retry-session"}'
printf '%s\n' '{"type":"result","session_id":"durable-retry-session","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .expect("write fake repair CLI");
    std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))
        .expect("make fake repair CLI executable");
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut provider = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    provider.enabled = true;
    provider.is_default = true;
    provider.custom_binary_enabled = true;
    provider.custom_binary_path = Some(cli_path.display().to_string());
    state
        .agent_provider_settings_repo
        .upsert(&provider)
        .await
        .expect("enable fake Claude provider");
    let conversation_id = conversation_id(95);
    let mut project = Project::new(
        "repair recovery project".to_string(),
        project_dir.path().display().to_string(),
    );
    project.id = project_id();
    project.worktree_parent_directory = Some(worktree_parent.path().display().to_string());
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("derive exact workspace path");
    std::fs::create_dir_all(workspace_path.join(".git")).expect("seed test workspace marker");
    state
        .project_repo
        .create(project)
        .await
        .expect("seed retry project");
    let mut conversation = ChatConversation::new_project(project_id());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed retry conversation");
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.worktree_path = workspace_path.display().to_string();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed durable retry workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "retry delivery".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair attempt must start");
    };
    let target_identity =
        GitTargetIdentity::new(workspace_path, "refs/heads/ralphx/test/publish-recovery")
            .expect("valid canonical target identity");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        started,
        AgentRunId::from_string("due-retry-initial-run"),
        None,
        "reserve retry delivery",
        None,
    )
    .await
    .expect("reserve retry delivery");
    let AgentWorkspaceRepairDispatchOutcome::Reserved(dispatch) = dispatch else {
        panic!("first delivery must reserve its run");
    };
    let scheduled = crate::application::agent_workspace_publish_repair_state::settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        dispatch,
        crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        "retryable delivery failure",
        None,
    )
    .await
    .expect("schedule durable retry");
    let crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairTransitionOutcome::Applied(mut scheduled) = scheduled else {
        panic!("first delivery failure must schedule a retry");
    };
    let expected_updated_at = scheduled.updated_at;
    scheduled.next_dispatch_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    scheduled.updated_at += chrono::Duration::microseconds(1);
    let due = state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: scheduled,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Requested,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("make durable retry due");
    assert!(matches!(
        due,
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("due recovery redelivers repair"),
        1
    );
    let delivered = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load repaired attempt")
        .expect("repair remains current");
    assert_eq!(
        delivered.phase,
        AgentWorkspaceRepairPhase::Repairing,
        "due retry should bind a replacement run instead of rescheduling: {delivered:?}"
    );
    assert_eq!(delivered.dispatch_count, 1);
    assert!(delivered.next_dispatch_at.is_none());
    let replacement_run = delivered
        .reserved_agent_run_id
        .clone()
        .expect("successful due delivery binds exactly one replacement run");
    assert!(
        state
            .agent_run_repo
            .get_by_id(&replacement_run)
            .await
            .expect("load replacement run")
            .is_some(),
        "due recovery must create the bound replacement run through the chat service"
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load retry events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "repair_sent" && event.status == "succeeded")
            .count(),
        1,
        "due recovery must settle exactly one delivery event"
    );

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("duplicate recovery is suppressed"),
        0
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("reload retry events"),
        events,
        "duplicate recovery must not send another repair message or append another event"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn orphaned_requested_dispatch_is_rescued_through_the_delivery_lane() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        101,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"orphaned-retry-session"}'
printf '%s\n' '{"type":"result","session_id":"orphaned-retry-session","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .await;
    let orphan = age_requested_repair_attempt(&state, &conversation_id).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("rescue orphaned requested dispatch"),
        1
    );
    let recovered = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load rescued attempt")
        .expect("rescued attempt remains current");
    assert_eq!(recovered.phase, AgentWorkspaceRepairPhase::Repairing);
    assert!(recovered.reserved_agent_run_id.is_some());
    assert!(recovered.git_common_dir.is_some());
    assert!(recovered.target_ref.is_some());
    assert!(recovered.target_lease_epoch.is_some());
    assert!(recovered.updated_at > orphan.updated_at);
}

#[cfg(unix)]
#[tokio::test]
async fn fresh_orphaned_requested_dispatch_remains_untouched_during_grace_period() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(102, "#!/bin/sh\nexit 1\n").await;
    let before = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load fresh orphan")
        .expect("fresh orphan exists");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("fresh orphan recovery is harmless"),
        0
    );
    let after = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload fresh orphan")
        .expect("fresh orphan remains current");
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.phase, AgentWorkspaceRepairPhase::Requested);
    assert!(after.reserved_agent_run_id.is_none());
    assert!(after.target_lease_epoch.is_none());
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn orphaned_requested_delivery_failure_schedules_the_normal_retry() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(103, "#!/bin/sh\nexit 1\n").await;
    age_requested_repair_attempt(&state, &conversation_id).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("classify orphaned delivery failure"),
        1
    );
    let scheduled = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load scheduled orphan retry")
        .expect("scheduled orphan retry remains current");
    assert_eq!(scheduled.phase, AgentWorkspaceRepairPhase::Requested);
    assert_eq!(scheduled.dispatch_count, 1);
    assert!(scheduled.next_dispatch_at.is_some());
    assert!(scheduled.reserved_agent_run_id.is_none());
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn orphaned_successor_from_retry_blocked_is_rescued() {
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        105,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"orphaned-successor-session"}'
printf '%s\n' '{"type":"result","session_id":"orphaned-successor-session","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .await;
    let mut blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load blocked predecessor")
        .expect("blocked predecessor exists");
    let expected_updated_at = blocked.updated_at;
    blocked.phase = AgentWorkspaceRepairPhase::Blocked;
    blocked.blocker = Some("retry blocked predecessor".to_string());
    blocked.updated_at += chrono::Duration::microseconds(1);
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt: blocked,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at,
                next_phase: AgentWorkspaceRepairPhase::Blocked,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("block predecessor"),
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
    let successor = start_or_join_agent_workspace_repair(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        AgentWorkspaceRepairStartRequest {
            conversation_id: conversation_id.clone(),
            source: AgentWorkspaceRepairSource::Publish,
            continuation: AgentWorkspaceRepairContinuation::Publish,
            target_base_ref: "main".to_string(),
            target_base_commit: None,
            verified_newer_base: false,
            reason: "retry blocked repair".to_string(),
            summary: "Retry blocked repair.".to_string(),
            auto_merge_current: None,
            retry_blocked: true,
            carryover_pr_autofix_evidence: None,
        },
    )
    .await
    .expect("start orphaned successor");
    assert!(matches!(
        successor,
        AgentWorkspaceRepairStartOutcome::SuccessorStarted(_)
    ));
    age_requested_repair_attempt(&state, &conversation_id).await;

    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("rescue orphaned successor"),
        1
    );
    let rescued = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load rescued successor")
        .expect("rescued successor remains current");
    assert_eq!(rescued.phase, AgentWorkspaceRepairPhase::Repairing);
    assert!(rescued.reserved_agent_run_id.is_some());
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn blocked_automatic_repair_is_superseded_and_dispatched_without_user_action() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        107,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"automatic-blocked-session"}'
printf '%s\n' '{"type":"result","session_id":"automatic-blocked-session","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .await;
    let blocked = block_repair_attempt_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("automatically retry blocked repair"),
        1
    );
    let predecessor = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&blocked.id)
        .await
        .expect("load superseded predecessor")
        .expect("blocked predecessor persists");
    assert_eq!(
        predecessor.outcome,
        Some(AgentWorkspaceRepairOutcome::Superseded)
    );
    let successor = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load automatic successor")
        .expect("automatic successor remains current");
    assert_eq!(successor.generation, blocked.generation + 1);
    assert_eq!(successor.phase, AgentWorkspaceRepairPhase::Repairing);
    assert!(successor
        .pending_reasons
        .iter()
        .any(|reason| reason == "auto_retry_blocked_repair:1"));
    assert!(successor.reserved_agent_run_id.is_some());

    // The retry marker is internal scheduling bookkeeping. Rendering it as the assignment's
    // "Context:" told the recipient nothing about what needed repairing.
    let delivered = latest_sent_repair_message(&state, &conversation_id).await;
    assert!(
        !delivered.contains("auto_retry_blocked_repair"),
        "internal retry markers must never reach an agent assignment: {delivered}"
    );
    assert!(
        delivered.contains("The current durable workspace repair still needs attention."),
        "a marker-only reason list must fall back to human context: {delivered}"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn push_handoff_base_advanced_blocker_retries_with_the_fresh_base() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, project_dir) = seed_orphaned_repair_dispatch(
        117,
        r#"#!/bin/sh
cat >/dev/null &
sleep 1
"#,
    )
    .await;
    let (blocked, stale_base_commit, fresh_base_commit) =
        block_push_handoff_base_advanced_repair(&state, &conversation_id, project_dir.path(), 0)
            .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recover push-handoff base-advanced repair"),
        1
    );
    let predecessor = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&blocked.id)
        .await
        .expect("load superseded push-handoff predecessor")
        .expect("push-handoff predecessor persists");
    assert_eq!(
        predecessor.outcome,
        Some(AgentWorkspaceRepairOutcome::Superseded)
    );
    let successor = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load push-handoff automatic successor")
        .expect("push-handoff automatic successor remains current");
    assert_eq!(successor.generation, blocked.generation + 1);
    assert_eq!(successor.source, AgentWorkspaceRepairSource::PrAutofix);
    assert_eq!(successor.target_base_ref, "main");
    assert_eq!(
        successor.target_base_commit.as_deref(),
        Some(fresh_base_commit.as_str())
    );
    assert_ne!(
        successor.target_base_commit.as_deref(),
        Some(stale_base_commit.as_str())
    );
    assert!(successor
        .pending_reasons
        .iter()
        .any(|reason| reason == "auto_retry_blocked_repair:1"));
    assert_ne!(successor.phase, AgentWorkspaceRepairPhase::Blocked);
}

#[cfg(unix)]
#[tokio::test]
async fn push_handoff_base_advanced_blocker_stays_blocked_after_auto_retry_cap() {
    let (state, conversation_id, _worktree_parent, project_dir) =
        seed_orphaned_repair_dispatch(118, "#!/bin/sh\nexit 1\n").await;
    let (blocked, _stale_base_commit, _fresh_base_commit) =
        block_push_handoff_base_advanced_repair(&state, &conversation_id, project_dir.path(), 3)
            .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("capped push-handoff recovery is harmless"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load capped push-handoff repair")
        .expect("capped push-handoff repair remains current");
    assert_eq!(current.id, blocked.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert_eq!(current.target_base_commit, blocked.target_base_commit);
    assert_eq!(
        current.blocker.as_deref(),
        blocked.blocker.as_deref(),
        "the capped attempt remains actionable with its original push-handoff blocker"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn ready_automatic_repair_past_grace_re_drives_its_publish_continuation() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        114,
        r#"#!/bin/sh
cat >/dev/null
"#,
    )
    .await;
    let ready = park_repair_attempt_ready_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("re-drive parked ready continuation");

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload re-driven ready repair")
        .expect("re-driven repair remains current");
    assert_ne!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == "auto_retry_ready_repair:1"));
    assert_eq!(
        current.id, ready.id,
        "the continuation owns the same generation"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn ready_automatic_repair_busy_publish_guard_remains_re_drivable() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        119,
        r#"#!/bin/sh
cat >/dev/null
"#,
    )
    .await;
    let ready = park_repair_attempt_ready_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;
    let _busy_guard =
        try_acquire_agent_workspace_repair_publish_continuation_guard(&conversation_id)
            .expect("reserve publish continuation guard");

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("busy publish continuation is retryable");

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload busy continuation")
        .expect("busy continuation remains current");
    assert_eq!(current.id, ready.id);
    assert!(matches!(
        current.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ));
    assert!(current.settled_at.is_none());
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn concurrent_ready_recovery_sweeps_re_drive_one_current_generation() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        120,
        r#"#!/bin/sh
cat >/dev/null
"#,
    )
    .await;
    let ready = park_repair_attempt_ready_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;

    let (first, second) = tokio::join!(
        recover_agent_workspace_repair_attempts_for_state(&state),
        recover_agent_workspace_repair_attempts_for_state(&state)
    );
    first.expect("first ready recovery sweep");
    second.expect("second ready recovery sweep");
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload concurrently re-driven ready repair")
        .expect("ready repair remains current");
    assert_eq!(current.id, ready.id);
    assert_eq!(
        current
            .pending_reasons
            .iter()
            .filter(|reason| reason.as_str() == "auto_retry_ready_repair:1")
            .count(),
        1,
        "the Ready timestamp CAS rejects the stale recovery snapshot"
    );
}

#[tokio::test]
async fn ready_automatic_repair_within_grace_remains_untouched() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(115);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed ready repair workspace");
    state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "ready grace repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start ready repair");
    let ready = park_repair_attempt_ready_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        59,
    )
    .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("respect ready repair grace"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload ready repair")
        .expect("ready repair remains current");
    assert_eq!(current.id, ready.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(!current
        .pending_reasons
        .iter()
        .any(|reason| reason.starts_with("auto_retry_ready_repair:")));
}

#[tokio::test]
async fn ready_manual_repair_remains_untouched_by_automatic_recovery() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(116);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed manual ready workspace");
    state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Manual,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "manual ready repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start manual ready repair");
    let ready = park_repair_attempt_ready_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("skip manual ready recovery");
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload manual ready repair")
        .expect("manual ready repair remains current");
    assert_eq!(current.id, ready.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
}

#[tokio::test]
async fn ready_automatic_repair_with_open_effect_remains_untouched() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(117);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed effect-owned ready workspace");
    state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "effect-owned ready repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start effect-owned ready repair");
    let ready = park_repair_attempt_ready_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: ready.id.clone(),
                generation: ready.generation,
                expected_phase: AgentWorkspaceRepairPhase::Ready,
                expected_attempt_updated_at: ready.updated_at,
                effect: AgentWorkspaceRepairEffect::new(
                    ready.id.clone(),
                    AgentWorkspaceRepairEffectKind::PushBranch,
                    "ready-repair-open-effect",
                    chrono::Utc::now(),
                ),
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("record ready repair effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("respect ready repair effect owner");
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload effect-owned ready repair")
        .expect("effect-owned ready repair remains current");
    assert_eq!(current.id, ready.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
}

#[tokio::test]
async fn ready_automatic_repair_at_streak_cap_is_settled() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(118);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed capped ready workspace");
    state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "auto_retry_ready_repair:3".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start capped ready repair");
    let ready = park_repair_attempt_ready_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("settle capped ready repair"),
        1
    );
    assert!(state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load current capped ready repair")
        .is_none());
    let settled = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&ready.id)
        .await
        .expect("load settled ready repair")
        .expect("capped ready repair persists");
    assert_eq!(settled.outcome, Some(AgentWorkspaceRepairOutcome::Failed));
    assert!(settled.settled_at.is_some());
}

#[tokio::test]
async fn blocked_manual_repair_remains_untouched_by_automatic_recovery() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(108);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed manual repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Manual,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "manual blocked repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start manual repair");
    assert!(matches!(
        started,
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(_)
    ));
    let blocked = block_repair_attempt_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("manual blocked recovery is harmless"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload manual blocked repair")
        .expect("manual repair remains current");
    assert_eq!(current.id, blocked.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert_eq!(current.updated_at, blocked.updated_at);
}

#[tokio::test]
async fn blocked_automatic_repair_at_streak_cap_remains_untouched() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(109);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed capped repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "auto_retry_blocked_repair:3".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start capped repair");
    assert!(matches!(
        started,
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(_)
    ));
    let blocked = block_repair_attempt_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        1_000,
    )
    .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("capped automatic recovery is harmless"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload capped repair")
        .expect("capped repair remains current");
    assert_eq!(current.id, blocked.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
}

#[tokio::test]
async fn blocked_automatic_repair_waits_for_backoff() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(110);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed backoff repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "backoff blocked repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start backoff repair");
    assert!(matches!(
        started,
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(_)
    ));
    let blocked = block_repair_attempt_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        59,
    )
    .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("backoff recovery is harmless"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload backoff repair")
        .expect("backoff repair remains current");
    assert_eq!(current.id, blocked.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
}

#[tokio::test]
async fn blocked_automatic_repair_with_an_open_effect_remains_untouched() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(111);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed effect-owned blocked repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "effect-owned blocked repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start effect-owned blocked repair");
    assert!(matches!(
        started,
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(_)
    ));
    let blocked = block_repair_attempt_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: blocked.id.clone(),
                generation: blocked.generation,
                expected_phase: AgentWorkspaceRepairPhase::Blocked,
                expected_attempt_updated_at: blocked.updated_at,
                effect: AgentWorkspaceRepairEffect::new(
                    blocked.id.clone(),
                    AgentWorkspaceRepairEffectKind::PushBranch,
                    "blocked-repair-open-effect",
                    chrono::Utc::now(),
                ),
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("record blocked repair effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("effect-owned blocked recovery is harmless"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload effect-owned blocked repair")
        .expect("effect-owned repair remains current");
    assert_eq!(current.id, blocked.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn concurrent_blocked_recovery_sweeps_start_and_dispatch_one_successor() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        112,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"automatic-blocked-race-session"}'
printf '%s\n' '{"type":"result","session_id":"automatic-blocked-race-session","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .await;
    let blocked = block_repair_attempt_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        61,
    )
    .await;

    let (first, second) = tokio::join!(
        recover_agent_workspace_repair_attempts_for_state(&state),
        recover_agent_workspace_repair_attempts_for_state(&state)
    );
    assert_eq!(
        first.expect("first blocked recovery sweep")
            + second.expect("second blocked recovery sweep"),
        1,
        "the blocked-attempt timestamp CAS must allow one automatic successor"
    );
    let predecessor = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&blocked.id)
        .await
        .expect("load raced predecessor")
        .expect("raced predecessor persists");
    assert_eq!(
        predecessor.outcome,
        Some(AgentWorkspaceRepairOutcome::Superseded)
    );
    let successor = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load raced successor")
        .expect("one successor remains current");
    assert_eq!(successor.generation, blocked.generation + 1);
    assert_eq!(successor.phase, AgentWorkspaceRepairPhase::Repairing);
    assert!(successor.reserved_agent_run_id.is_some());
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn blocked_automatic_repair_streak_escalates_then_stops_at_the_cap() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        113,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"automatic-blocked-streak-session"}'
printf '%s\n' '{"type":"result","session_id":"automatic-blocked-streak-session","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .await;
    let mut blocked = block_repair_attempt_after(
        &state,
        &conversation_id,
        AgentWorkspaceRepairPhase::Requested,
        1_000,
    )
    .await;

    for expected_streak in 1..=3 {
        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("advance automatic blocked-repair streak"),
            1
        );
        let successor = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load automatic streak successor")
            .expect("automatic streak successor remains current");
        assert!(matches!(
            successor.phase,
            AgentWorkspaceRepairPhase::Requested | AgentWorkspaceRepairPhase::Repairing
        ));
        assert!(
            successor.reserved_agent_run_id.is_some() || successor.next_dispatch_at.is_some(),
            "automatic successor must be active or durably scheduled: {successor:?}"
        );
        assert!(successor
            .pending_reasons
            .iter()
            .any(|reason| { reason == &format!("auto_retry_blocked_repair:{expected_streak}") }));
        let successor_phase = successor.phase;
        blocked =
            block_repair_attempt_after(&state, &conversation_id, successor_phase, 1_000).await;
    }

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("capped streak recovery is harmless"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load capped automatic repair")
        .expect("capped automatic repair remains current");
    assert_eq!(current.id, blocked.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == "auto_retry_blocked_repair:3"));
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn concurrent_orphaned_recovery_sweeps_dispatch_only_one_run() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        106,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"orphaned-concurrent-session"}'
printf '%s\n' '{"type":"result","session_id":"orphaned-concurrent-session","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .await;
    age_requested_repair_attempt(&state, &conversation_id).await;

    let (first, second) = tokio::join!(
        recover_agent_workspace_repair_attempts_for_state(&state),
        recover_agent_workspace_repair_attempts_for_state(&state)
    );
    assert_eq!(
        first.expect("first orphan sweep") + second.expect("second orphan sweep"),
        1,
        "the Requested timestamp CAS must prevent a duplicate rescue delivery"
    );
    let repaired = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load concurrently rescued attempt")
        .expect("attempt remains current");
    assert_eq!(repaired.phase, AgentWorkspaceRepairPhase::Repairing);
    let run_id = repaired
        .reserved_agent_run_id
        .expect("exactly one run is reserved");
    assert!(state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .expect("load reserved run")
        .is_some());
}

#[tokio::test]
async fn orphaned_requested_dispatch_without_a_workspace_is_blocked_actionably() {
    let mut state = AppState::new_test();
    let conversation_id = conversation_id(104);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed workspace before its durable attempt");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "workspace was removed before dispatch".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed missing-workspace orphan");
    assert!(matches!(
        started,
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(_)
    ));
    age_requested_repair_attempt(&state, &conversation_id).await;
    state.agent_conversation_workspace_repo =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("block missing-workspace orphan"),
        1
    );
    let blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load blocked orphan")
        .expect("blocked orphan remains current");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(blocked
        .blocker
        .as_deref()
        .is_some_and(|blocker| blocker.contains("cannot find its canonical workspace")));
    assert!(blocked.reserved_agent_run_id.is_none());
}

#[tokio::test]
async fn due_recovery_with_an_open_repair_effect_does_not_dispatch_or_append_events() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(96);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed durable retry workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "effect-owned retry".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start durable repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair attempt must start");
    };
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-recovery-effect"),
        "refs/heads/ralphx/test/publish-recovery",
    )
    .expect("valid canonical target identity");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        started,
        AgentRunId::from_string("effect-owned-retry-initial-run"),
        None,
        "reserve retry delivery",
        None,
    )
    .await
    .expect("reserve retry delivery");
    let AgentWorkspaceRepairDispatchOutcome::Reserved(dispatch) = dispatch else {
        panic!("first delivery must reserve its run");
    };
    let scheduled = crate::application::agent_workspace_publish_repair_state::settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        dispatch,
        crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        "retryable delivery failure",
        None,
    )
    .await
    .expect("schedule durable retry");
    let crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairTransitionOutcome::Applied(mut scheduled) = scheduled else {
        panic!("first delivery failure must schedule a retry");
    };
    let expected_updated_at = scheduled.updated_at;
    scheduled.next_dispatch_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    scheduled.updated_at += chrono::Duration::microseconds(1);
    let due = state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: scheduled,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Requested,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("make retry due");
    let AgentWorkspaceRepairAttemptTransitionOutcome::Applied(due) = due else {
        panic!("due checkpoint must preserve retry authority");
    };
    let effect = AgentWorkspaceRepairEffect::new(
        due.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "effect-owned-retry",
        chrono::Utc::now(),
    );
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: due.id.clone(),
                generation: due.generation,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_attempt_updated_at: due.updated_at,
                effect,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("record active repair effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));
    let events_before = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load events before suppressed retry");

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("effect-owned retry recovery is harmless"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load suppressed retry")
        .expect("repair remains current");
    assert_eq!(current.id, due.id);
    assert_eq!(current.updated_at, due.updated_at);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Requested);
    assert!(current.reserved_agent_run_id.is_none());
    assert!(
        state
            .agent_run_repo
            .get_latest_for_conversation(&conversation_id)
            .await
            .expect("load replacement run")
            .is_none(),
        "effect ownership must suppress replacement agent-run creation"
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("reload events after suppressed retry"),
        events_before,
        "effect ownership must not append retry delivery events"
    );
}

#[tokio::test]
async fn startup_recovery_keeps_a_live_reserved_repair_run_authoritative() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(94);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed durable repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "live delivery".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair attempt must start");
    };
    let run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed live reserved repair run");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        GitTargetIdentity::new(
            PathBuf::from("/tmp/ralphx-repair-recovery-live-run"),
            "refs/heads/ralphx/test/publish-recovery",
        )
        .expect("valid canonical target identity"),
        started,
        run.id,
        None,
        "dispatch durable repair",
        None,
    )
    .await
    .expect("reserve live repair delivery");
    assert!(matches!(
        dispatch,
        AgentWorkspaceRepairDispatchOutcome::Reserved(_)
    ));

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("live repair recovery is a no-op"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load live repair")
        .expect("repair remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Dispatching);
    assert_eq!(current.dispatch_count, 0);
    assert_eq!(current.reserved_agent_run_id, Some(run.id));
}

#[tokio::test]
async fn failed_exact_pr_autofix_is_classified_as_retry_eligible() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(31);
    let workspace = needs_agent_workspace(conversation_id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let fingerprint = "github_pr_autofix:684:head:checks";
    seed_failed_pr_autofix_run(state.agent_run_repo.as_ref(), conversation_id, fingerprint).await;

    let (_workspace, outcome) =
        recover_stale_publish_repair_for_workspace_with_project_repo_outcome(
            Arc::clone(&state.agent_conversation_workspace_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.project_repo),
            workspace,
        )
        .await
        .expect("recover retry-eligible repair");

    assert_eq!(outcome, StalePublishRepairRecoveryOutcome::RetryEligible);
}

#[tokio::test]
async fn state_recovery_recovers_terminal_needs_agent_workspace_and_reloads_it() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(1);
    let workspace = needs_agent_workspace(conversation_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_terminal_run(state.agent_run_repo.as_ref(), conversation_id).await;

    let recovered = recover_stale_agent_workspace_publish_repairs_for_state(&state)
        .await
        .expect("recover stale publish repair");

    assert_eq!(recovered, 1);
    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(events.iter().any(|event| {
        event.step == "legacy_repair_import_blocked"
            && event.classification.as_deref() == Some("legacy_repair_import_ambiguous")
    }));
}

#[tokio::test]
async fn state_recovery_preserves_an_active_exact_legacy_pr_autofix() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(32);
    let workspace = needs_agent_workspace(conversation_id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let mut run = AgentRun::new(conversation_id.clone());
    run.action_kind = Some(AgentRunActionKind::PrAutofix);
    run.action_context_id = Some("684".to_string());
    run.action_target_id = Some("github_pr_autofix:684:head:checks".to_string());
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed exact active PR autofix");

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("active PR autofix recovery should defer"),
        0
    );
    assert!(state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load durable repair authority")
        .is_none());
    let preserved = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(
        preserved.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(preserved.pr_supervision_status.as_deref(), Some("fixing"));
    assert!(state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load publication events")
        .iter()
        .all(|event| event.step != "legacy_repair_import_blocked"));
}

#[tokio::test]
async fn recovery_correlates_the_exact_pr_autofix_attempt_not_a_newer_unrelated_run() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(11);
    let workspace = needs_agent_workspace(conversation_id);
    let fingerprint = "github_pr_autofix:684:head:failing-check";
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix",
            "needs_agent",
            "PR autofix started.",
            Some(fingerprint.to_string()),
        ))
        .await
        .expect("seed autofix event");
    seed_failed_pr_autofix_run(agent_run_repo.as_ref(), conversation_id, fingerprint).await;
    let unrelated = agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed unrelated run");
    agent_run_repo
        .complete(&unrelated.id)
        .await
        .expect("complete unrelated run");

    let (updated, outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
            None,
        )
        .await
        .expect("recover exact autofix attempt");

    assert_eq!(outcome, StalePublishRepairRecoveryOutcome::RetryEligible);
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("single retry is eligible"));
}

#[tokio::test]
async fn recovery_with_review_target_preserves_current_reviewing_handoff() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(2);
    let workspace = needs_agent_workspace(conversation_id);
    let target = review_target();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");
    workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor(conversation_id, &target))
        .await
        .expect("seed review monitor");
    seed_terminal_run(agent_run_repo.as_ref(), conversation_id).await;

    let (refreshed, outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
            Some(&target),
        )
        .await
        .expect("check stale publish repair");

    assert_eq!(outcome, StalePublishRepairRecoveryOutcome::HandoffPreserved);
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(
        !events
            .iter()
            .any(|event| event.step == "stale_repair_recovered"),
        "current Workspace Review handoff must not be downgraded as stale"
    );
}

#[tokio::test]
async fn stale_review_handoff_without_matching_target_is_recovered_and_reloaded() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(3);
    let workspace = needs_agent_workspace(conversation_id);
    let target = review_target();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");
    workspace_repo
        .upsert_workspace_review_monitor(stale_passed_monitor(conversation_id, &target))
        .await
        .expect("seed stale passed review monitor");
    seed_terminal_run(agent_run_repo.as_ref(), conversation_id).await;

    let refreshed = recover_stale_publish_repair_for_workspace_and_reload(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        workspace,
    )
    .await
    .expect("recover stale publish repair");

    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(events
        .iter()
        .any(|event| event.step == "stale_repair_recovered"));
}

#[tokio::test]
async fn batch_recovery_counts_only_recovered_workspaces() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let recoverable_id = conversation_id(4);
    let active_id = conversation_id(5);
    let recoverable = needs_agent_workspace(recoverable_id);
    let active = needs_agent_workspace(active_id);
    workspace_repo
        .create_or_update(recoverable)
        .await
        .expect("seed recoverable workspace");
    workspace_repo
        .create_or_update(active)
        .await
        .expect("seed active workspace");
    seed_terminal_run(agent_run_repo.as_ref(), recoverable_id).await;
    agent_run_repo
        .create(AgentRun::new(active_id))
        .await
        .expect("seed active run");

    let recovered = recover_stale_agent_workspace_publish_repairs(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
    )
    .await
    .expect("recover batch");

    assert_eq!(recovered, 1);
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&active_id)
            .await
            .expect("load active workspace")
            .expect("active workspace exists")
            .publication_push_status
            .as_deref(),
        Some("needs_agent")
    );
}

#[tokio::test]
async fn recovery_heals_only_an_active_current_repair_to_fixing() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(6);
    let mut workspace = needs_agent_workspace(conversation_id);
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed active run");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_sent",
            "succeeded",
            "Sent failure to workspace repair agent",
            Some("agent_fixable".to_string()),
        ))
        .await
        .expect("seed repair evidence");

    let refreshed = recover_stale_publish_repair_for_workspace_and_reload(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        workspace,
    )
    .await
    .expect("reconcile active repair");

    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("fixing"));
}

#[tokio::test]
async fn recovery_restores_blocked_state_only_for_the_current_pr_autofix_replacement() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(7);
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.pr_supervision_status = Some("blocked".to_string());
    let fingerprint = "github_pr_autofix:684:head:replacement";
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed blocked workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix",
            "started",
            "PR autofix replacement started.",
            Some(fingerprint.to_string()),
        ))
        .await
        .expect("seed autofix evidence");
    let mut replacement = AgentRun::new(conversation_id.clone());
    replacement.action_kind = Some(AgentRunActionKind::PrAutofix);
    replacement.action_context_id = Some("684".to_string());
    replacement.action_target_id = Some(fingerprint.to_string());
    agent_run_repo
        .create(replacement)
        .await
        .expect("seed exact active replacement");

    let (refreshed, outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
            None,
        )
        .await
        .expect("recover exact active replacement");

    assert_eq!(
        outcome,
        StalePublishRepairRecoveryOutcome::ActiveReplacement
    );
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("fixing"));
}

#[tokio::test]
async fn recovery_does_not_treat_an_unrelated_active_run_as_a_pr_autofix_replacement() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(8);
    let workspace = needs_agent_workspace(conversation_id.clone());
    let fingerprint = "github_pr_autofix:684:head:retry";
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix",
            "failed",
            "PR autofix failed.",
            Some(fingerprint.to_string()),
        ))
        .await
        .expect("seed exact autofix event");
    seed_failed_pr_autofix_run(
        agent_run_repo.as_ref(),
        conversation_id.clone(),
        fingerprint,
    )
    .await;
    agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed unrelated active run");

    let (refreshed, outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
            None,
        )
        .await
        .expect("recover retry-eligible autofix");

    assert_eq!(outcome, StalePublishRepairRecoveryOutcome::RetryEligible);
    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
}

mod extracted_inline_tests {
    use super::*;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, AgentRun, AgentRunStatus, ChatConversationId,
        IdeationAnalysisBaseRefKind, ProjectId,
    };
    use crate::infrastructure::memory::{
        MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    };

    fn needs_agent_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-1".to_string()),
            "ralphx/test/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.publication_pr_number = Some(42);
        workspace.publication_pr_status = Some("failed".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_supervision_status = Some("fixing".to_string());
        workspace
    }

    async fn create_failed_run(
        agent_run_repo: &MemoryAgentRunRepository,
        conversation_id: ChatConversationId,
    ) {
        let run = agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed run");
        agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("mark run failed");
    }

    #[tokio::test]
    async fn recovers_needs_agent_workspace_when_no_agent_run_is_active() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        let recovered = recover_stale_agent_workspace_publish_repairs(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await
        .expect("recover stale repair");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));

        let events = workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("list events");
        assert!(events.iter().any(|event| {
            event.step == STALE_REPAIR_RECOVERED_STEP
                && event.status == "succeeded"
                && event.classification.as_deref() == Some(STALE_NEEDS_AGENT_CLASSIFICATION)
        }));
    }

    #[tokio::test]
    async fn reloads_recovered_workspace_from_app_state() {
        let state = crate::application::AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
        let workspace = needs_agent_workspace(conversation_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed run");
        state
            .agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("mark run failed");

        let refreshed = recover_stale_publish_repair_for_workspace_in_state(&state, workspace)
            .await
            .expect("recover stale repair");

        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn recovers_stale_supervised_autofix_workspace_as_blocked() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_current = Some(true);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        let recovered = recover_stale_agent_workspace_publish_repairs(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await
        .expect("recover stale repair");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
        assert_eq!(
            refreshed.pr_supervision_summary.as_deref(),
            Some(STALE_REPAIR_BLOCKED_SUMMARY)
        );
        assert_eq!(refreshed.pr_auto_merge_current, Some(true));
    }

    #[tokio::test]
    async fn startup_helper_recovers_stale_repairs() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("77777777-7777-7777-7777-777777777777");

        recover_stale_agent_workspace_publish_repairs_on_startup(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await;

        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        recover_stale_agent_workspace_publish_repairs_on_startup(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await;

        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn keeps_needs_agent_workspace_locked_while_agent_run_is_active() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed active run");

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("needs_agent")
        );
        assert!(
            workspace_repo
                .list_publication_events(&conversation_id)
                .await
                .expect("list events")
                .is_empty(),
            "active repairs must not be downgraded"
        );
    }

    #[tokio::test]
    async fn ignores_workspace_that_is_not_waiting_on_agent_repair() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace.publication_push_status = Some("failed".to_string());

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
    }

    #[tokio::test]
    async fn ignores_workspace_without_terminal_repair_run_evidence() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("55555555-5555-5555-5555-555555555555");
        let workspace = needs_agent_workspace(conversation_id);

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
    }

    #[tokio::test]
    async fn recovers_terminal_run_without_completion_timestamp() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("88888888-8888-8888-8888-888888888888");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let mut run = AgentRun::new(conversation_id);
        run.status = AgentRunStatus::Failed;
        run.completed_at = None;
        agent_run_repo.create(run).await.expect("seed run");

        let recovered =
            recover_stale_publish_repair_for_workspace(workspace_repo, agent_run_repo, workspace)
                .await
                .expect("check repair state");

        assert!(recovered);
    }

    #[tokio::test]
    async fn does_not_recover_a_fresh_claim_from_an_older_terminal_run() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("66666666-6666-6666-6666-666666666666");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;
        workspace.updated_at = chrono::Utc::now() + chrono::Duration::minutes(5);

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
        let current = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            current.publication_push_status.as_deref(),
            Some("needs_agent")
        );
    }

    #[tokio::test]
    async fn stale_recovery_snapshot_cannot_overwrite_newer_workspace_state() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("99999999-9999-9999-9999-999999999999");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;
        workspace_repo
            .update_publication(
                &conversation_id,
                workspace.publication_pr_number,
                workspace.publication_pr_url.as_deref(),
                workspace.publication_pr_status.as_deref(),
                Some("pushed"),
            )
            .await
            .expect("persist newer publication state");
        workspace_repo
            .update_pr_auto_merge_state(
                &conversation_id,
                workspace.pr_auto_merge_current,
                Some("monitoring"),
                Some("Newer state is authoritative"),
            )
            .await
            .expect("persist newer supervision state");

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("stale recovery should be rejected");

        assert!(!recovered);
        let current = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(current.publication_push_status.as_deref(), Some("pushed"));
        assert_eq!(current.pr_supervision_status.as_deref(), Some("monitoring"));
        assert!(workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap()
            .iter()
            .all(|event| event.step != STALE_REPAIR_RECOVERED_STEP));
    }

    fn transient_workspace(
        conversation_id: ChatConversationId,
        status: &str,
    ) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-1".to_string()),
            "ralphx/test/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.publication_push_status = Some(status.to_string());
        workspace
    }

    #[tokio::test]
    async fn recovers_stale_transient_refreshing_workspace() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let conversation_id =
            ChatConversationId::from_string("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        let workspace = transient_workspace(conversation_id.clone(), "refreshing");
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        // stale_older_than_secs=0 means any workspace updated at or before now is stale
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            0,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));

        let events = workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list events");
        assert!(events.iter().any(|e| {
            e.step == STALE_TRANSIENT_RECOVERED_STEP
                && e.status == "succeeded"
                && e.classification.as_deref() == Some(STALE_TRANSIENT_CLASSIFICATION)
        }));
    }

    #[tokio::test]
    async fn skips_recent_transient_workspace_within_staleness_window() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let conversation_id =
            ChatConversationId::from_string("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        let workspace = transient_workspace(conversation_id.clone(), "checking");
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        // stale_older_than_secs=3600 means only workspaces older than 1 hour are stale;
        // a just-created workspace must not be recovered
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            3600,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 0);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("checking")
        );
    }

    #[tokio::test]
    async fn recovers_all_four_stale_transient_statuses() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());

        for (id, status) in [
            ("cccccccc-cccc-cccc-cccc-cccccccccc01", "refreshing"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc02", "checking"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc03", "committing"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc04", "describing"),
        ] {
            let conv_id = ChatConversationId::from_string(id.to_string());
            let workspace = transient_workspace(conv_id, status);
            workspace_repo
                .create_or_update(workspace)
                .await
                .expect("seed workspace");
        }

        // stale_older_than_secs=0 catches all freshly-seeded transient workspaces
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            0,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 4);
    }

    #[tokio::test]
    async fn imports_only_exact_legacy_repair_provenance_then_blocks_its_terminal_run() {
        let state = AppState::new_test();
        let conversation_id = conversation_id(91);
        let workspace = needs_agent_workspace(conversation_id.clone());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id.clone()))
            .await
            .expect("seed exact legacy run");
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_requested",
                "started",
                "legacy publish repair requested",
                Some("agent_fixable:publish".to_string()),
            ))
            .await
            .expect("seed continuation provenance");
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_sent",
                "succeeded",
                "legacy repair dispatched",
                Some(format!("agent_fixable:run:{}", run.id)),
            ))
            .await
            .expect("seed run provenance");

        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&state)
                .await
                .expect("import exact legacy repair"),
            0
        );
        let imported = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load durable attempt")
            .expect("exact provenance should import");
        assert_eq!(imported.source, AgentWorkspaceRepairSource::Legacy);
        assert_eq!(
            imported.continuation,
            AgentWorkspaceRepairContinuation::Publish
        );
        assert_eq!(imported.phase, AgentWorkspaceRepairPhase::Repairing);
        assert_eq!(imported.reserved_agent_run_id, Some(run.id));
        let imported_workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("reload imported compatibility projection")
            .expect("workspace remains present");
        let imported_events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("load import audit events");

        // Startup/recovery may re-enter after a crash. The exact legacy import is one-time:
        // it joins the same durable generation and replays neither projection nor audit events.
        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&state)
                .await
                .expect("repeat exact legacy import"),
            0
        );
        assert_eq!(
            state
                .agent_workspace_repair_repo
                .get_current_repair_attempt(&conversation_id)
                .await
                .expect("reload durable attempt")
                .expect("attempt remains current")
                .id,
            imported.id
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .expect("reload compatibility projection")
                .expect("workspace remains present"),
            imported_workspace
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .list_publication_events(&conversation_id)
                .await
                .expect("reload import audit events"),
            imported_events
        );

        state
            .agent_run_repo
            .fail(&run.id, "repair process stopped")
            .await
            .expect("terminalize run");
        assert!(recover_agent_workspace_repair_after_terminal_run(
            &state,
            &conversation_id,
            &run.id
        )
        .await
        .expect("recover exact terminal run"));
        let blocked = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load blocked attempt")
            .expect("attempt remains visible for retry");
        assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
        assert!(blocked.blocker.is_some());
    }

    #[tokio::test]
    async fn legacy_import_requires_exact_continuation_base_and_run_provenance() {
        async fn append_provenance(
            state: &AppState,
            conversation_id: &ChatConversationId,
            continuation: &str,
            run_id: &AgentRunId,
        ) {
            state
                .agent_conversation_workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id.clone(),
                    "repair_requested",
                    "started",
                    "legacy repair requested",
                    Some(continuation.to_string()),
                ))
                .await
                .expect("seed legacy continuation");
            state
                .agent_conversation_workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id.clone(),
                    "repair_sent",
                    "succeeded",
                    "legacy repair dispatched",
                    Some(format!("agent_fixable:run:{run_id}")),
                ))
                .await
                .expect("seed legacy run");
        }

        let update_state = AppState::new_test();
        let update_conversation = conversation_id(81);
        update_state
            .agent_conversation_workspace_repo
            .create_or_update(needs_agent_workspace(update_conversation.clone()))
            .await
            .expect("seed update-only workspace");
        let update_run = update_state
            .agent_run_repo
            .create(AgentRun::new(update_conversation.clone()))
            .await
            .expect("seed update-only run");
        append_provenance(
            &update_state,
            &update_conversation,
            "agent_fixable:update_only",
            &update_run.id,
        )
        .await;
        recover_stale_agent_workspace_publish_repairs_for_state(&update_state)
            .await
            .expect("import update-only provenance");
        let update_attempt = update_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&update_conversation)
            .await
            .expect("load update-only repair")
            .expect("exact update-only provenance imports");
        assert_eq!(
            update_attempt.continuation,
            AgentWorkspaceRepairContinuation::UpdateOnly
        );
        assert_eq!(update_attempt.phase, AgentWorkspaceRepairPhase::Repairing);

        let terminal_state = AppState::new_test();
        let terminal_conversation = conversation_id(82);
        terminal_state
            .agent_conversation_workspace_repo
            .create_or_update(needs_agent_workspace(terminal_conversation.clone()))
            .await
            .expect("seed terminal legacy workspace");
        let terminal_run = terminal_state
            .agent_run_repo
            .create(AgentRun::new(terminal_conversation.clone()))
            .await
            .expect("seed terminal legacy run");
        terminal_state
            .agent_run_repo
            .fail(&terminal_run.id, "legacy repair stopped")
            .await
            .expect("terminalize legacy run");
        append_provenance(
            &terminal_state,
            &terminal_conversation,
            "agent_fixable:publish",
            &terminal_run.id,
        )
        .await;
        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&terminal_state)
                .await
                .expect("import terminal provenance"),
            1
        );
        let terminal_attempt = terminal_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&terminal_conversation)
            .await
            .expect("load terminal legacy repair")
            .expect("terminal exact provenance remains actionable");
        assert_eq!(terminal_attempt.phase, AgentWorkspaceRepairPhase::Blocked);
        assert!(terminal_attempt
            .blocker
            .as_deref()
            .is_some_and(|blocker| blocker.contains("without a durable completion receipt")));

        for (suffix, continuation, clear_base, run_owner_matches) in [
            (83, "agent_fixable:manual", false, true),
            (84, "agent_fixable:publish", true, true),
            (85, "agent_fixable:publish", false, false),
        ] {
            let state = AppState::new_test();
            let conversation_id = conversation_id(suffix);
            let mut workspace = needs_agent_workspace(conversation_id.clone());
            if clear_base {
                workspace.base_commit = None;
            }
            state
                .agent_conversation_workspace_repo
                .create_or_update(workspace)
                .await
                .expect("seed ambiguous legacy workspace");
            let run_conversation = if run_owner_matches {
                conversation_id.clone()
            } else {
                ChatConversationId::from_string(format!("wrong-owner-{suffix}"))
            };
            let run = state
                .agent_run_repo
                .create(AgentRun::new(run_conversation))
                .await
                .expect("seed ambiguous legacy run");
            append_provenance(&state, &conversation_id, continuation, &run.id).await;

            assert_eq!(
                recover_stale_agent_workspace_publish_repairs_for_state(&state)
                    .await
                    .expect("ambiguous provenance fails closed"),
                1
            );
            let blocked = state
                .agent_workspace_repair_repo
                .get_current_repair_attempt(&conversation_id)
                .await
                .expect("load ambiguous legacy repair")
                .expect("ambiguous provenance remains actionable");
            assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
            assert_eq!(
                blocked.continuation,
                AgentWorkspaceRepairContinuation::Manual
            );
        }

        let missing_run_state = AppState::new_test();
        let missing_run_conversation = conversation_id(86);
        missing_run_state
            .agent_conversation_workspace_repo
            .create_or_update(needs_agent_workspace(missing_run_conversation.clone()))
            .await
            .expect("seed missing-run workspace");
        append_provenance(
            &missing_run_state,
            &missing_run_conversation,
            "agent_fixable:publish",
            &AgentRunId::new(),
        )
        .await;
        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&missing_run_state)
                .await
                .expect("missing exact run fails closed"),
            1
        );
    }

    #[tokio::test]
    async fn startup_recovery_ignores_exact_legacy_provenance_when_a_durable_attempt_exists() {
        let state = AppState::new_test();
        let conversation_id = conversation_id(92);
        let workspace = needs_agent_workspace(conversation_id.clone());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed legacy projection");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id.clone()))
            .await
            .expect("seed active durable run");
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_requested",
                "started",
                "legacy publish repair requested",
                Some("agent_fixable:publish".to_string()),
            ))
            .await
            .expect("seed exact legacy continuation provenance");
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_sent",
                "succeeded",
                "legacy repair dispatched",
                Some(format!("agent_fixable:run:{}", run.id)),
            ))
            .await
            .expect("seed exact legacy run provenance");

        let mut durable_attempt = AgentWorkspaceRepairAttempt::new(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::BaseUpdate,
            AgentWorkspaceRepairContinuation::UpdateOnly,
            "main",
            false,
            false,
            false,
            None,
            chrono::Utc::now(),
        );
        durable_attempt.phase = AgentWorkspaceRepairPhase::Repairing;
        durable_attempt.reserved_agent_run_id = Some(run.id.clone());
        let durable_attempt = match state
            .agent_workspace_repair_repo
            .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: durable_attempt,
                reason: "durable repair already owns this conversation".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("start durable repair")
        {
            StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
            outcome => panic!("expected a new durable repair, got {outcome:?}"),
        };
        let before_workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load compatibility projection")
            .expect("workspace remains present");
        let before_events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("load legacy events");

        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&state)
                .await
                .expect("recover with durable authority"),
            0
        );

        assert_eq!(
            state
                .agent_workspace_repair_repo
                .get_current_repair_attempt(&conversation_id)
                .await
                .expect("reload durable repair")
                .expect("durable repair remains current")
                .id,
            durable_attempt.id
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .expect("reload compatibility projection")
                .expect("workspace remains present"),
            before_workspace
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .list_publication_events(&conversation_id)
                .await
                .expect("reload legacy events"),
            before_events
        );
        assert!(state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&durable_attempt.id)
            .await
            .expect("load durable effects")
            .is_none());
    }

    #[tokio::test]
    async fn ambiguous_legacy_repair_fails_closed_without_guessing_run_or_continuation() {
        let state = AppState::new_test();
        let conversation_id = conversation_id(92);
        state
            .agent_conversation_workspace_repo
            .create_or_update(needs_agent_workspace(conversation_id.clone()))
            .await
            .expect("seed ambiguous legacy workspace");

        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&state)
                .await
                .expect("block ambiguous legacy repair"),
            1
        );
        let attempt = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load blocked legacy attempt")
            .expect("ambiguous legacy state must remain observable");
        assert_eq!(attempt.source, AgentWorkspaceRepairSource::Legacy);
        assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Blocked);
        assert_eq!(
            attempt.continuation,
            AgentWorkspaceRepairContinuation::Manual
        );
        assert!(attempt.reserved_agent_run_id.is_none());
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load compatibility projection")
            .expect("workspace exists");
        assert_eq!(workspace.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(workspace.pr_supervision_status.as_deref(), Some("blocked"));
    }
}

// --- Unattended repair loop regressions -------------------------------------------------------
//
// Production incident 2026-07-31 (PR #934): four Opus generations re-validated a clean workspace
// because durable redelivery addressed the generic repairer, successors carried no failure
// identity, and a live dispatch was settled as "interrupted" 43 ms after spawn.

fn failing_check_pr_health(
    head: &str,
    check_name: &str,
) -> crate::domain::services::github_service::PrHealth {
    crate::domain::services::github_service::PrHealth {
        sync_state: crate::domain::services::PrSyncState {
            status: crate::domain::services::PrStatus::Open,
            merge_state_status: None,
            mergeable: Some(crate::domain::services::github_service::PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "ralphx/test/publish-recovery".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some(head.to_string()),
            base_ref_oid: Some("base-sha".to_string()),
        },
        review_decision: None,
        checks: vec![crate::domain::services::github_service::PrHealthCheck {
            name: check_name.to_string(),
            status: Some("completed".to_string()),
            conclusion: Some("failure".to_string()),
            details_url: Some("https://github.com/owner/repo/actions/runs/1".to_string()),
        }],
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }
}

fn health_fingerprint(
    pr_number: i64,
    health: &crate::domain::services::github_service::PrHealth,
) -> String {
    crate::application::services::pr_merge_poller::classify_agent_workspace_pr_autofix_issue(
        pr_number, health,
    )
    .expect("failing check classifies as a PR autofix issue")
    .classification
}

/// Rewrites the current attempt into a blocked PR autofix generation carrying an exact failure
/// identity, aged past the automatic blocked-retry backoff.
async fn block_pr_autofix_attempt_with_fingerprint(
    state: &AppState,
    conversation_id: &ChatConversationId,
    fingerprint: Option<String>,
) -> AgentWorkspaceRepairAttempt {
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load attempt to block")
        .expect("attempt exists to block");
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .expect("load workspace for blocked fixture")
        .expect("workspace exists for blocked fixture");
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.source = AgentWorkspaceRepairSource::PrAutofix;
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.pr_autofix_health_fingerprint = fingerprint;
    attempt.pr_autofix_dispatch_head_commit = Some("dispatch-head".to_string());
    // Base parity keeps the base-advance escape hatch out of the way; these fixtures exercise the
    // health comparison itself.
    attempt.target_base_commit = workspace.base_commit.clone();
    attempt.blocker = Some("transient_ci".to_string());
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1_000);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("block PR autofix attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("blocking PR autofix attempt must apply, got {outcome:?}"),
    }
}

async fn latest_sent_repair_message(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> String {
    let messages = state
        .chat_message_repo
        .get_by_conversation(conversation_id)
        .await
        .expect("load delivered repair messages");
    messages
        .iter()
        .rev()
        .find(|message| message.role == crate::domain::entities::MessageRole::User)
        .map(|message| message.content.clone())
        .expect("a repair assignment was delivered")
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn pr_autofix_redelivery_addresses_the_pr_fixer_with_pr_context() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        120,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"result","session_id":"pr-fixer-redelivery","is_error":false,"result":"fix started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .await;
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load seeded attempt")
        .expect("seeded attempt exists");
    let expected_updated_at = attempt.updated_at;
    attempt.source = AgentWorkspaceRepairSource::PrAutofix;
    attempt.pr_autofix_health_fingerprint = Some("github_pr_autofix:684:checks:rust".to_string());
    // Internal scheduling markers must never surface to the recipient as repair context.
    attempt.pending_reasons = vec!["auto_retry_blocked_repair:1".to_string()];
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(61);
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at,
                next_phase: AgentWorkspaceRepairPhase::Requested,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("age PR autofix orphan"),
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("rescue orphaned PR autofix dispatch"),
        1
    );

    let message = latest_sent_repair_message(&state, &conversation_id).await;
    assert!(
        message.contains("redelivering an interrupted PR fix"),
        "PR autofix redelivery must use the PR fixer assignment, got: {message}"
    );
    assert!(message.contains("complete_agent_workspace_pr_fix"));
    assert!(message.contains("get_agent_workspace_pr_fix_context"));
    assert!(message.contains("PR #684"));
    assert!(message.contains("github_pr_autofix:684:checks:rust"));
    assert!(
        !message.contains("use the available repair-completion tool"),
        "PR autofix redelivery must not reuse the generic workspace repair assignment"
    );
    assert!(
        !message.contains("auto_retry_blocked_repair"),
        "internal scheduling markers must not leak into the assignment: {message}"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn non_pr_autofix_redelivery_keeps_the_generic_workspace_repair_assignment() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) = seed_orphaned_repair_dispatch(
        121,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"result","session_id":"generic-repair-redelivery","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .await;
    age_requested_repair_attempt(&state, &conversation_id).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("rescue orphaned publish dispatch"),
        1
    );

    let message = latest_sent_repair_message(&state, &conversation_id).await;
    assert!(
        message.contains("complete_agent_workspace_repair"),
        "a publish repair must name the repairer's own completion tool: {message}"
    );
    assert!(
        !message.contains("complete_agent_workspace_pr_fix"),
        "a publish repair must not be addressed to the PR fixer: {message}"
    );
}

/// Seeds a workspace whose path resolves the way production requires: a real project repository
/// with a real worktree checked out at the workspace branch. Successor evaluation reads live PR
/// health through that path, so a fixture without it can only ever exercise the withhold branch.
async fn seed_pr_autofix_health_workspace(
    suffix: u8,
) -> (
    AppState,
    ChatConversationId,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let state = AppState::new_test();
    let worktree_parent = tempfile::tempdir().expect("create PR autofix worktree parent");
    let project_dir = tempfile::tempdir().expect("create PR autofix project directory");
    recovery_git(project_dir.path(), &["init", "-b", "main"]);
    recovery_git(
        project_dir.path(),
        &["config", "user.email", "recovery@example.com"],
    );
    recovery_git(
        project_dir.path(),
        &["config", "user.name", "Recovery Test"],
    );
    std::fs::write(project_dir.path().join("README.md"), "base\n").expect("write base file");
    recovery_git(project_dir.path(), &["add", "README.md"]);
    recovery_git(project_dir.path(), &["commit", "-m", "base"]);

    let conversation_id = conversation_id(suffix);
    let mut project = Project::new(
        "pr autofix health project".to_string(),
        project_dir.path().display().to_string(),
    );
    project.id = project_id();
    project.worktree_parent_directory = Some(worktree_parent.path().display().to_string());
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("derive exact PR autofix workspace path");
    recovery_git(
        project_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            "ralphx/test/publish-recovery",
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    state
        .project_repo
        .create(project)
        .await
        .expect("seed PR autofix project");
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.worktree_path = workspace_path.display().to_string();
    workspace.base_commit = Some(recovery_git(project_dir.path(), &["rev-parse", "HEAD"]));
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed PR autofix workspace");
    (state, conversation_id, worktree_parent, project_dir)
}

#[tokio::test]
async fn blocked_pr_autofix_with_unchanged_health_parks_without_spawning() {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(122).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;

    let health = failing_check_pr_health("head-unchanged", "Rust Tests");
    let fingerprint = health_fingerprint(684, &health);
    let github = Arc::new(crate::tests::mock_github_service::MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    state.github_service =
        Some(github.clone() as Arc<dyn crate::domain::services::GithubServiceTrait>);

    let blocked = block_pr_autofix_attempt_with_fingerprint(
        &state,
        &conversation_id,
        Some(fingerprint.clone()),
    )
    .await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("evaluate blocked PR autofix successor"),
        0
    );

    let held = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load held attempt")
        .expect("held attempt remains current");
    assert_eq!(
        held.generation, blocked.generation,
        "no successor generation"
    );
    assert_eq!(held.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(held.pending_reasons.iter().any(|reason| {
        reason
        == crate::application::agent_workspace_publish_repair_state::UNCHANGED_HEALTH_REPAIR_REASON
    }));
    assert_eq!(
        held.pr_autofix_health_fingerprint.as_deref(),
        Some(fingerprint.as_str())
    );
    assert!(
        state
            .agent_run_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("list runs")
            .is_empty(),
        "an unchanged failure fingerprint must not spend another agent generation"
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events");
    assert!(
        events.iter().any(|event| event.step
            == crate::application::agent_workspace_publish_repair_state::REPAIR_FINGERPRINT_HOLD_STEP),
        "the hold must be user visible, never a silent skip"
    );

    // A parked hold must survive the Ready auto-retry lane; only the poller may end it.
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("re-run recovery over the held attempt"),
        0
    );
    let still_held = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload held attempt")
        .expect("held attempt is still current");
    assert_eq!(still_held.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(still_held.settled_at.is_none());
}

/// Retry caps count attempts, not cost. A conversation that has already burned its agent-minutes
/// budget on one failure identity must hand the failure to a human instead of buying another
/// generation, and the handover must be visible rather than a silent stop.
#[tokio::test]
async fn exhausted_agent_minutes_budget_parks_needs_human_with_a_notification() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(125).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;
    let fingerprint = "github_pr_autofix:684:checks:rust-tests".to_string();
    let blocked = block_pr_autofix_attempt_with_fingerprint(
        &state,
        &conversation_id,
        Some(fingerprint.clone()),
    )
    .await;

    // A finished run that already consumed far more than the default 45-minute budget.
    let mut run = crate::domain::entities::AgentRun::new(conversation_id.clone());
    run.started_at = chrono::Utc::now() - chrono::Duration::minutes(90);
    run.completed_at = Some(chrono::Utc::now() - chrono::Duration::minutes(1));
    run.status = crate::domain::entities::AgentRunStatus::Completed;
    let run_id = run.id.clone();
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed an expensive finished repair run");
    bind_reserved_run_to_attempt(&state, &conversation_id, &run_id).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("evaluate an over-budget PR autofix generation"),
        1
    );

    let parked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load parked attempt")
        .expect("parked attempt remains current");
    assert_eq!(
        parked.generation, blocked.generation,
        "an exhausted budget must not buy another generation"
    );
    assert!(
        parked.pending_reasons.iter().any(|reason| reason
            == crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON),
        "budget exhaustion is a human handover, not an automatic retry: {parked:?}"
    );

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events");
    assert!(
        events
            .iter()
            .any(|event| event.step == "repair_budget_exhausted"),
        "the spend must be recorded on the publication timeline"
    );

    let notifications = state
        .notification_repo
        .list(None, None, 20)
        .await
        .expect("list notifications");
    assert!(
        notifications.notifications.iter().any(|notification| {
            notification.target.conversation_id.as_deref()
                == Some(conversation_id.as_str().as_str())
                && notification
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains("repair generations"))
        }),
        "budget exhaustion must reach the user, never stop silently: {:?}",
        notifications.notifications
    );

    // The workspace also remembers the identity, so a fresh streak cannot restart on it.
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("reload workspace")
        .expect("workspace exists");
    assert_eq!(
        workspace.last_blocked_pr_health_fingerprint.as_deref(),
        Some(fingerprint.as_str())
    );
}

/// Binds an existing run to the current attempt as its durable reservation.
async fn bind_reserved_run_to_attempt(
    state: &AppState,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
) {
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load attempt to bind")
        .expect("attempt exists to bind");
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.reserved_agent_run_id = Some(run_id.clone());
    attempt.updated_at += chrono::Duration::microseconds(1);
    let outcome = state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("bind reserved run");
    assert!(matches!(
        outcome,
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
}

#[tokio::test]
async fn blocked_pr_autofix_without_provable_health_withholds_the_successor() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(123);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed unprovable-health workspace");
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;
    block_pr_autofix_attempt_with_fingerprint(
        &state,
        &conversation_id,
        Some("github_pr_autofix:684:checks:rust".to_string()),
    )
    .await;

    // No GitHub service: the current failure identity cannot be proven, so no agent may be spent.
    assert!(state.github_service.is_none());
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("withhold successor without provable health"),
        0
    );
    let unchanged = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load withheld attempt")
        .expect("withheld attempt remains current");
    assert_eq!(unchanged.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(state
        .agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("list runs")
        .is_empty());
}

async fn start_blocked_pr_autofix_generation(
    state: &AppState,
    conversation_id: &ChatConversationId,
) {
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::PrAutofix,
                AgentWorkspaceRepairContinuation::ResumePrSupervision,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "pr autofix generation".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start PR autofix generation");
    assert!(matches!(
        started,
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(_)
    ));
}

/// A PR autofix generation that was dispatched against an exact observed failure, with a base that
/// has not moved. This is the only shape the successor gate applies to.
fn blocked_pr_autofix_attempt(
    conversation_id: &ChatConversationId,
    fingerprint: &str,
) -> AgentWorkspaceRepairAttempt {
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
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.pr_autofix_health_fingerprint = Some(fingerprint.to_string());
    attempt
}

#[tokio::test]
async fn pr_autofix_successor_withholds_when_github_cannot_be_read_at_all() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(70);
    let workspace = needs_agent_workspace(conversation_id.clone());
    let attempt = blocked_pr_autofix_attempt(&conversation_id, "ci:Clippy:failure");
    assert!(attempt.target_base_commit.is_none());
    assert!(state.github_service.is_none());

    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::Withhold("github_service_unavailable")
    );
}

#[tokio::test]
async fn pr_autofix_successor_proceeds_when_the_repair_base_moved() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(71);
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.base_commit = Some("base-b".to_string());
    let mut attempt = blocked_pr_autofix_attempt(&conversation_id, "ci:Clippy:failure");
    attempt.target_base_commit = Some("base-a".to_string());

    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::Proceed(None)
    );
}

#[tokio::test]
async fn pr_autofix_successor_withholds_when_no_pr_owns_the_workspace() {
    let mut state = AppState::new_test();
    state.github_service = Some(Arc::new(MockGithubService::new()));
    let conversation_id = conversation_id(72);
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.publication_pr_number = None;
    workspace.linked_plan_branch_id = None;
    let attempt = blocked_pr_autofix_attempt(&conversation_id, "ci:Clippy:failure");

    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::Withhold("pr_number_unresolved")
    );
}

#[tokio::test]
async fn pr_autofix_successor_borrows_the_linked_plan_branch_pr_only_for_its_own_session() {
    let mut state = AppState::new_test();
    state.github_service = Some(Arc::new(MockGithubService::new()));
    let conversation_id = conversation_id(73);
    let session_id = IdeationSessionId::from_string("session-pr-autofix-successor");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("plan-artifact-pr-autofix"),
        session_id.clone(),
        project_id(),
        "ralphx/plan/pr-autofix".to_string(),
        "main".to_string(),
    );
    plan_branch.pr_number = Some(910);
    state
        .plan_branch_repo
        .create(plan_branch.clone())
        .await
        .expect("seed linked plan branch");

    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.publication_pr_number = None;
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    workspace.linked_ideation_session_id = Some(session_id);
    let attempt = blocked_pr_autofix_attempt(&conversation_id, "ci:Clippy:failure");

    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::Withhold("project_missing")
    );

    let mut foreign = workspace.clone();
    foreign.linked_ideation_session_id =
        Some(IdeationSessionId::from_string("session-someone-else"));
    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &foreign).await,
        PrAutofixSuccessorDecision::Withhold("pr_number_unresolved")
    );
}

#[tokio::test]
async fn pr_autofix_successor_withholds_when_the_workspace_path_cannot_be_resolved() {
    let mut state = AppState::new_test();
    state.github_service = Some(Arc::new(MockGithubService::new()));
    let conversation_id = conversation_id(74);
    let mut project = Project::new(
        "pr autofix successor project".to_string(),
        "/tmp/ralphx-pr-autofix-successor-missing-project".to_string(),
    );
    project.id = project_id();
    state
        .project_repo
        .create(project)
        .await
        .expect("seed project");
    let workspace = needs_agent_workspace(conversation_id.clone());
    let attempt = blocked_pr_autofix_attempt(&conversation_id, "ci:Clippy:failure");

    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::Withhold("workspace_path_unresolved")
    );
}
