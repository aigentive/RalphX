use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use async_trait::async_trait;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_recovery::agent_workspace_repair_owns_unpublished_publish_continuation;
use crate::application::agent_workspace_publish_recovery::{
    blocked_repair_fences_new_base_work, claim_pending_redrive_delivery,
    due_repair_dispatch_message, evaluate_pr_autofix_successor, is_blocked_and_not_auto_retryable,
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
    recover_stale_transient_publish_statuses, recover_stale_transient_publish_statuses_for_state,
    recover_stale_transient_publish_statuses_for_state_with_redrive_emitter,
    settle_missing_workspace_resolution, settle_redrive_delivery, PrAutofixSuccessorDecision,
    StalePublishRepairRecoveryOutcome, AGENT_WORKSPACE_PUBLISH_REDRIVE_DELIVERING_STATUS,
    AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS, AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX,
    AUTO_RETRY_READY_REPAIR_REASON_PREFIX, BLOCKED_STREAK_REARMED_REASON_PREFIX,
    CONTINUATION_OPEN_EFFECT_ATTENTION_REASON, EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX,
    STALE_NEEDS_AGENT_CLASSIFICATION, STALE_REPAIR_BLOCKED_SUMMARY, STALE_REPAIR_RECOVERED_STEP,
    STALE_TRANSIENT_CLASSIFICATION, STALE_TRANSIENT_RECOVERED_STEP, WORKSPACE_MISSING_SETTLED_STEP,
};
use crate::application::agent_workspace_publish_repair_state::{
    block_agent_workspace_repair_completion, explicit_agent_workspace_repair_retry_allowed,
    held_repair_has_unpublished_head, reserve_agent_workspace_repair_dispatch,
    start_or_join_agent_workspace_repair, AgentWorkspaceRepairDispatchOutcome,
    AgentWorkspaceRepairStartOutcome, AgentWorkspaceRepairStartRequest,
    AgentWorkspaceRepairTransitionOutcome, BASE_STALE_AFTER_UPDATE_REPAIR_REASON,
    MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES, NEEDS_HUMAN_REPAIR_REASON,
    PRE_EXISTING_ON_BASE_REPAIR_REASON, UNCHANGED_HEALTH_REPAIR_REASON,
};
use crate::application::agent_workspace_review::{
    resolve_review_target, AgentWorkspaceReviewPacket, AgentWorkspaceReviewTarget,
};
use crate::application::publish_resilience::{
    reconcile_open_agent_workspace_repair_push_effect,
    try_acquire_agent_workspace_repair_publish_continuation_guard,
};
use crate::application::{AppState, GitService};
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, AgentRun,
    AgentRunActionKind, AgentRunId, AgentRunStatus, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind,
    AgentWorkspaceRepairEffectStatus, AgentWorkspaceRepairOutcome, AgentWorkspaceRepairPhase,
    AgentWorkspaceRepairSource, AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversation, ChatConversationId,
    GitMutationKind, GitTargetIdentity, GitTargetLeaseOwner, IdeationAnalysisBaseRefKind,
    IdeationSessionId, PlanBranch, Project, ProjectId,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentConversationWorkspaceRepository,
    AgentRunRepository, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, BeginGitMutation,
    CompleteAgentWorkspaceRepairEffect, CompleteAgentWorkspaceRepairEffectOutcome,
    CreateAgentWorkspaceRepairEffect, CreateAgentWorkspaceRepairEffectOutcome, ProjectRepository,
    SettleAgentWorkspaceRepairAttempt, SettleAgentWorkspaceRepairAttemptOutcome,
    StartOrJoinAgentWorkspaceRepairAttempt, StartOrJoinAgentWorkspaceRepairAttemptOutcome,
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
fn generic_repair_redelivery_uses_default_context_when_only_machine_markers_remain() {
    let markers = [
        NEEDS_HUMAN_REPAIR_REASON.to_string(),
        PRE_EXISTING_ON_BASE_REPAIR_REASON.to_string(),
        UNCHANGED_HEALTH_REPAIR_REASON.to_string(),
        format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}3"),
        format!("{AUTO_RETRY_READY_REPAIR_REASON_PREFIX}2"),
        format!("{EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX}bba066f"),
    ];

    for marker in markers {
        let mut attempt = AgentWorkspaceRepairAttempt::new(
            conversation_id(90),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "main",
            false,
            true,
            false,
            None,
            chrono::Utc::now(),
        );
        attempt.pending_reasons = vec![marker.clone()];
        let message = due_repair_dispatch_message(
            &attempt,
            &needs_agent_workspace(attempt.conversation_id.clone()),
        );

        assert!(message
            .contains("Context: The current durable workspace repair still needs attention."));
        assert!(
            !message.contains(&marker),
            "machine marker {marker:?} must not leak into agent context: {message}"
        );
    }
}

#[test]
fn generic_repair_redelivery_uses_older_human_context_before_needs_human_marker() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(91),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.pending_reasons = vec![
        "old base ref was deleted after its PR merged".to_string(),
        NEEDS_HUMAN_REPAIR_REASON.to_string(),
    ];

    let message = due_repair_dispatch_message(
        &attempt,
        &needs_agent_workspace(attempt.conversation_id.clone()),
    );

    assert!(message.contains("Context: old base ref was deleted after its PR merged"));
    assert!(!message.contains(NEEDS_HUMAN_REPAIR_REASON));
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

const CONTINUATION_REPAIR_HEAD: &str = "1111111111111111111111111111111111111111";

/// Seeds a workspace plus one repair attempt, optionally with the Observed `PushBranch` receipt
/// whose remote OID equals the attempt's repair head — the exact evidence that the repair already
/// landed remotely and the block therefore happened in the publish continuation.
async fn seed_repair_attempt_with_optional_observed_push(
    suffix: u8,
    observed_remote_oid: Option<&str>,
) -> (
    AppState,
    AgentWorkspaceRepairAttempt,
    Arc<MemoryAgentConversationWorkspaceRepository>,
) {
    let mut state = AppState::new_test();
    let memory_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    state.agent_conversation_workspace_repo = memory_repo.clone();
    state.agent_workspace_repair_repo = memory_repo.clone();
    let conversation_id = conversation_id(suffix);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("fence fixture workspace should persist");
    let now = chrono::Utc::now();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id,
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        now,
    );
    attempt.repair_head_commit = Some(CONTINUATION_REPAIR_HEAD.to_string());
    let attempt = match state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "fence fixture".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("fence fixture attempt should persist")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("fence fixture must start a fresh attempt, got {outcome:?}"),
    };

    if let Some(remote_oid) = observed_remote_oid {
        crate::testing::record_observed_agent_workspace_repair_push_receipt(
            state.agent_workspace_repair_repo.as_ref(),
            &attempt,
            remote_oid,
        )
        .await;
    }

    (state, attempt, memory_repo)
}

fn blocked_and_exhausted(attempt: &AgentWorkspaceRepairAttempt) -> AgentWorkspaceRepairAttempt {
    let mut blocked = attempt.clone();
    blocked.phase = AgentWorkspaceRepairPhase::Blocked;
    blocked.next_dispatch_at = None;
    blocked.dispatch_count = MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES;
    blocked.blocker = Some("PR description failed".to_string());
    blocked
}

/// A describe-only block leaves the repaired branch on the remote. Fencing new base-freshness work
/// behind it strands the workspace, so only repair-stage blocks (and human holds) keep the fence.
#[tokio::test]
async fn continuation_stage_blocked_repair_stops_fencing_new_base_work() {
    let (state, attempt, _memory_repo) =
        seed_repair_attempt_with_optional_observed_push(31, Some(CONTINUATION_REPAIR_HEAD)).await;

    assert!(
        !blocked_repair_fences_new_base_work(&state, &attempt).await,
        "a live attempt is not a fence at all"
    );

    let blocked = blocked_and_exhausted(&attempt);
    assert!(
        !blocked_repair_fences_new_base_work(&state, &blocked).await,
        "an observed push proves the block happened after the repair reached the remote"
    );

    let mut retryable = blocked.clone();
    retryable.dispatch_count = 0;
    retryable.next_dispatch_at = Some(chrono::Utc::now() + chrono::Duration::seconds(60));
    assert!(!blocked_repair_fences_new_base_work(&state, &retryable).await);

    let mut needs_human = blocked.clone();
    needs_human.pending_reasons = vec![NEEDS_HUMAN_REPAIR_REASON.to_string()];
    assert!(
        blocked_repair_fences_new_base_work(&state, &needs_human).await,
        "a human hold keeps the fence regardless of the push receipt"
    );
}

#[tokio::test]
async fn repair_stage_blocked_repair_keeps_fencing_new_base_work() {
    let (state, attempt, _memory_repo) =
        seed_repair_attempt_with_optional_observed_push(32, None).await;

    assert!(
        blocked_repair_fences_new_base_work(&state, &blocked_and_exhausted(&attempt)).await,
        "without a push receipt the local repair never landed, so the fence stays"
    );
}

#[tokio::test]
async fn observed_push_for_another_head_keeps_fencing_new_base_work() {
    let (state, attempt, _memory_repo) = seed_repair_attempt_with_optional_observed_push(
        33,
        Some("2222222222222222222222222222222222222222"),
    )
    .await;

    assert!(
        blocked_repair_fences_new_base_work(&state, &blocked_and_exhausted(&attempt)).await,
        "a receipt for a different head is not proof that this repair head was pushed"
    );
}

/// An unreadable push receipt is never proof that the repair landed, so the fence must survive it.
#[tokio::test]
async fn unreadable_push_receipt_keeps_fencing_new_base_work() {
    let (state, attempt, memory_repo) =
        seed_repair_attempt_with_optional_observed_push(34, Some(CONTINUATION_REPAIR_HEAD)).await;
    memory_repo.fail_next_repair_effect_read("repair effect store is unavailable");

    assert!(
        blocked_repair_fences_new_base_work(&state, &blocked_and_exhausted(&attempt)).await,
        "an effect-read failure must fail closed"
    );
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
    // Production retarget_agent_workspace_repair_pr_handoff now mirrors the fresh base onto
    // the attempt's target_base_commit so that auto-retry successors inherit it correctly.
    attempt.target_base_commit = Some(fresh_base_commit.clone());
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

    for (suffix, phase, continuation, recovery_passes, expected_blocker) in [
        (
            97,
            AgentWorkspaceRepairPhase::Validating,
            AgentWorkspaceRepairContinuation::Publish,
            1,
            "lost canonical Git target authority",
        ),
        (
            98,
            AgentWorkspaceRepairPhase::ContinuationPending,
            AgentWorkspaceRepairContinuation::Manual,
            3,
            "tried 3 times",
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

        for pass in 1..=recovery_passes {
            assert_eq!(
                recover_agent_workspace_repair_attempts_for_state(&state)
                    .await
                    .expect("reconcile interrupted durable phase"),
                u32::from(pass == recovery_passes)
            );
        }
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
        events
            .iter()
            .filter(|event| event.step == "repair_auto_retry_dispatched")
            .count(),
        1,
        "executing a due auto-retry must append exactly one repair_auto_retry_dispatched event (proof obligation 9)"
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
            explicit_publish_requested: false,
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
async fn ready_ci_and_base_stale_holds_remain_untouched_by_recovery() {
    for (suffix, hold_kind) in [(126, "ci_rerun"), (127, "base_stale")] {
        let state = AppState::new_test();
        let conversation_id = conversation_id(suffix);
        state
            .agent_conversation_workspace_repo
            .create_or_update(needs_agent_workspace(conversation_id.clone()))
            .await
            .expect("seed stationary hold workspace");
        state
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
                reason: format!("seed {hold_kind} recovery hold"),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("start stationary hold repair");
        let ready = park_repair_attempt_ready_after(
            &state,
            &conversation_id,
            AgentWorkspaceRepairPhase::Requested,
            61,
        )
        .await;
        let expected_updated_at = ready.updated_at;
        let mut held = ready;
        match hold_kind {
            "ci_rerun" => {
                held.ci_rerun_count = 1;
                held.ci_rerun_fingerprint = Some("ci-rerun:held:126".to_string());
            }
            "base_stale" => held
                .pending_reasons
                .push(BASE_STALE_AFTER_UPDATE_REPAIR_REASON.to_string()),
            _ => unreachable!(),
        }
        held.updated_at += chrono::Duration::microseconds(1);
        let held = match state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt: held,
                expected_phase: AgentWorkspaceRepairPhase::Ready,
                expected_updated_at,
                next_phase: AgentWorkspaceRepairPhase::Ready,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("persist stationary Ready hold")
        {
            AgentWorkspaceRepairAttemptTransitionOutcome::Applied(held) => held,
            outcome => panic!("stationary Ready hold must persist, got {outcome:?}"),
        };
        let events_before = state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list pre-recovery publication events");

        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("recover stationary Ready hold"),
            0
        );

        let current = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("reload stationary hold")
            .expect("stationary hold remains current");
        assert_eq!(
            current, held,
            "{hold_kind} recovery must not mutate the attempt"
        );
        assert!(!current
            .pending_reasons
            .iter()
            .any(|reason| reason.starts_with(AUTO_RETRY_READY_REPAIR_REASON_PREFIX)));
        assert!(state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&current.id)
            .await
            .expect("inspect stationary hold effects")
            .is_none());
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .list_publication_events(&conversation_id)
                .await
                .expect("list post-recovery publication events"),
            events_before,
            "{hold_kind} recovery must not emit continuation events"
        );
    }
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
async fn ready_publish_repair_without_consent_or_auto_publish_remains_untouched() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(121);
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.auto_publish_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed disabled-auto-publish workspace");
    state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                false,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "background publish repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start background repair");
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
            .expect("disabled automatic publish must be safe to recover"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload parked repair")
        .expect("parked repair remains current");
    assert_eq!(current.id, ready.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(!current.explicit_publish_requested);
    assert!(
        !current
            .pending_reasons
            .iter()
            .any(|reason| reason.starts_with("auto_retry_ready_repair:")),
        "automatic recovery must not spend a retry without durable publish authority"
    );
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
        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("refreshed")
        );

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
    async fn stale_transient_recovery_preserves_live_owner_then_recovers_terminal_owner() {
        let state = AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("abababab-abab-abab-abab-abababababab");
        state
            .agent_conversation_workspace_repo
            .create_or_update(transient_workspace(conversation_id.clone(), "refreshing"))
            .await
            .expect("seed workspace");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id.clone()))
            .await
            .expect("seed live owner run");
        let run_id = run.id.as_str();
        state
            .agent_conversation_workspace_repo
            .claim_publish_lease(
                &conversation_id,
                &run_id,
                "owned-transient-token",
                chrono::Utc::now(),
                None,
                false,
            )
            .await
            .expect("claim owner lease");

        assert_eq!(
            recover_stale_transient_publish_statuses_for_state(&state, 0)
                .await
                .expect("live-owner recovery sweep"),
            0,
            "a stale timestamp must not override live run authority"
        );
        let live_owned = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load live-owned workspace")
            .expect("workspace exists");
        assert_eq!(
            live_owned.publish_lease_token.as_deref(),
            Some("owned-transient-token")
        );
        assert_eq!(
            live_owned.publication_push_status.as_deref(),
            Some("refreshing")
        );

        state
            .agent_run_repo
            .fail(&run.id, "owner terminated")
            .await
            .expect("terminalize owner run");
        assert_eq!(
            recover_stale_transient_publish_statuses_for_state_with_redrive_emitter(
                &state,
                0,
                &|_conversation_id| Err("event bus unavailable".to_string()),
            )
            .await
            .expect("terminal-owner recovery sweep"),
            1
        );
        let recovered = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load recovered workspace")
            .expect("workspace exists");
        assert_eq!(recovered.publish_lease_owner_run_id, None);
        assert_eq!(recovered.publish_lease_token, None);
        assert_eq!(
            recovered.publication_push_status.as_deref(),
            Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS)
        );
        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("load recovery events");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.step == STALE_TRANSIENT_RECOVERED_STEP)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn startup_recovery_immediately_reclaims_fresh_orphaned_operation_lease() {
        let state = AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("acacacac-acac-acac-acac-acacacacacac");
        state
            .agent_conversation_workspace_repo
            .create_or_update(transient_workspace(conversation_id.clone(), "refreshing"))
            .await
            .expect("seed workspace");
        state
            .agent_conversation_workspace_repo
            .claim_publish_lease(
                &conversation_id,
                &format!("publish-operation:{conversation_id}"),
                "orphaned-operation-token",
                chrono::Utc::now(),
                None,
                false,
            )
            .await
            .expect("seed operation lease from a prior process");

        assert_eq!(
            recover_stale_transient_publish_statuses_for_state_with_redrive_emitter(
                &state,
                300,
                &|_conversation_id| Err("event bus unavailable".to_string()),
            )
            .await
            .expect("startup recovery sweep"),
            1,
            "missing process-local liveness must reclaim without the legacy five-minute wait"
        );
        let recovered = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(recovered.publish_lease_token, None);
        assert_eq!(
            recovered.publication_push_status.as_deref(),
            Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS)
        );
        assert_eq!(
            recover_stale_transient_publish_statuses_for_state_with_redrive_emitter(
                &state,
                300,
                &|_conversation_id| Err("event bus unavailable".to_string()),
            )
            .await
            .expect("pending re-drive remains eligible without duplicating recovery state"),
            0
        );
    }

    #[tokio::test]
    async fn concurrent_pending_redrive_sweeps_emit_once() {
        let state = AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("acacacac-acac-acac-acac-acacacacacbd");
        state
            .agent_conversation_workspace_repo
            .create_or_update(transient_workspace(
                conversation_id.clone(),
                AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS,
            ))
            .await
            .expect("seed pending redrive workspace");
        let emitted = Arc::new(AtomicUsize::new(0));
        let emit_redrive = {
            let emitted = Arc::clone(&emitted);
            move |_conversation_id: &ChatConversationId| {
                emitted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        };

        let (first, second) = tokio::join!(
            recover_stale_transient_publish_statuses_for_state_with_redrive_emitter(
                &state,
                0,
                &emit_redrive,
            ),
            recover_stale_transient_publish_statuses_for_state_with_redrive_emitter(
                &state,
                0,
                &emit_redrive,
            )
        );

        assert_eq!(
            first.expect("first recovery") + second.expect("second recovery"),
            1
        );
        assert_eq!(
            emitted.load(Ordering::SeqCst),
            1,
            "only the worker that atomically claims the pending re-drive may emit it"
        );
        let recovered = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load recovered workspace")
            .expect("workspace exists");
        assert_eq!(
            recovered.publication_push_status.as_deref(),
            Some("refreshed")
        );
    }

    #[tokio::test]
    async fn failed_pending_redrive_emit_restores_the_durable_pending_marker() {
        let state = AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("acacacac-acac-acac-acac-acacacacacbe");
        state
            .agent_conversation_workspace_repo
            .create_or_update(transient_workspace(
                conversation_id.clone(),
                AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS,
            ))
            .await
            .expect("seed pending redrive workspace");

        assert_eq!(
            recover_stale_transient_publish_statuses_for_state_with_redrive_emitter(
                &state,
                0,
                &|_conversation_id| Err("event bus unavailable".to_string()),
            )
            .await
            .expect("failed emit recovery"),
            0
        );
        let recovered = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load restored workspace")
            .expect("workspace exists");
        assert_eq!(
            recovered.publication_push_status.as_deref(),
            Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS),
            "an emit failure must return the claimed row to the durable retry queue"
        );
    }

    #[tokio::test]
    async fn stale_redrive_settlement_cannot_overwrite_newer_publication_state() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let conversation_id =
            ChatConversationId::from_string("acacacac-acac-acac-acac-acacacacacbf");
        workspace_repo
            .create_or_update(transient_workspace(
                conversation_id.clone(),
                AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS,
            ))
            .await
            .expect("seed pending redrive workspace");
        let pending = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load pending workspace")
            .expect("workspace exists");
        let delivering = claim_pending_redrive_delivery(workspace_repo.as_ref(), &pending)
            .await
            .expect("claim pending delivery")
            .expect("pending delivery claim applies");
        assert_eq!(
            delivering.publication_push_status.as_deref(),
            Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_DELIVERING_STATUS)
        );

        workspace_repo
            .update_publication(&conversation_id, None, None, None, Some("pushed"))
            .await
            .expect("persist newer publication state after emit");

        assert!(
            !settle_redrive_delivery(workspace_repo.as_ref(), &delivering)
                .await
                .expect("fenced stale settlement"),
            "the post-emit write must reject a stale delivery owner"
        );
        let current = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load current workspace")
            .expect("workspace exists");
        assert_eq!(current.publication_push_status.as_deref(), Some("pushed"));
    }

    #[tokio::test]
    async fn recovery_preserves_a_live_process_owned_operation_lease() {
        let state = AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("adadadad-adad-adad-adad-adadadadadad");
        state
            .agent_conversation_workspace_repo
            .create_or_update(transient_workspace(conversation_id.clone(), "refreshing"))
            .await
            .expect("seed workspace");
        state
            .agent_conversation_workspace_repo
            .claim_publish_lease(
                &conversation_id,
                &format!("publish-operation:{conversation_id}"),
                "live-operation-token",
                chrono::Utc::now(),
                None,
                false,
            )
            .await
            .expect("claim operation lease");
        let _operation_scope =
            crate::application::agent_workspace_publish_lease::begin_publish_operation_scope(
                &conversation_id,
            );
        crate::application::agent_workspace_publish_lease::spawn_publish_operation_lease_heartbeat(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
            "live-operation-token".to_string(),
        );

        assert_eq!(
            recover_stale_transient_publish_statuses_for_state(&state, 0)
                .await
                .expect("live operation recovery sweep"),
            0
        );
        let live = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            live.publish_lease_token.as_deref(),
            Some("live-operation-token")
        );
        assert_eq!(live.publication_push_status.as_deref(), Some("refreshing"));
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
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.source = AgentWorkspaceRepairSource::PrAutofix;
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.pr_autofix_health_fingerprint = fingerprint;
    attempt.pr_autofix_dispatch_head_commit = Some("dispatch-head".to_string());
    // Leaving target_base_commit at None keeps the base-advance check (repair_base_advanced) false
    // vacuously; these fixtures exercise the health fingerprint comparison, not the base-moved path.
    attempt.target_base_commit = None;
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
    github.state().fetch_pr_health_result = Some(Ok(health.clone()));
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

#[tokio::test]
async fn ready_health_hold_with_unpublished_head_marks_one_durable_redrive_without_spawning() {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(124).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;
    let health = failing_check_pr_health("remote-ready-head", "Rust Tests");
    let fingerprint = health_fingerprint(684, &health);
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health.clone()));
    state.github_service =
        Some(Arc::clone(&github) as Arc<dyn crate::domain::services::GithubServiceTrait>);
    block_pr_autofix_attempt_with_fingerprint(&state, &conversation_id, Some(fingerprint)).await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("park unchanged health");
    let held = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load health-held attempt")
        .expect("health-held attempt exists");
    assert_eq!(held.phase, AgentWorkspaceRepairPhase::Ready);

    let expected_updated_at = held.updated_at;
    let mut unpublished = held.clone();
    unpublished.repair_head_commit = Some("validated-local-ready-head".to_string());
    unpublished.updated_at += chrono::Duration::microseconds(1);
    let unpublished = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: unpublished,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist unpublished ready head")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("unpublished ready-head checkpoint must apply, got {outcome:?}"),
    };
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load held-head workspace")
        .expect("held-head workspace exists");
    let target_identity = GitService::canonical_target_identity(
        std::path::Path::new(&workspace.worktree_path),
        &workspace.branch_name,
    )
    .await
    .expect("resolve held-head target identity");
    let foreign_owner = GitTargetLeaseOwner::branch_update("foreign-task", "foreign-update");
    let AcquireGitTargetLeaseOutcome::Acquired {
        fencing_epoch: foreign_epoch,
    } = state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner: foreign_owner.clone(),
        })
        .await
        .expect("acquire held-head target for a foreign owner")
    else {
        panic!("foreign held-head target lease should be newly acquired");
    };
    github.state().fetch_pr_health_result = Some(Err(crate::error::AppError::Infrastructure(
        "GitHub health unavailable".to_string(),
    )));

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("unreadable health must hold closed");
    let unreadable = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload unreadable-health hold")
        .expect("unreadable-health hold remains current");
    assert_eq!(unreadable.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(!unreadable
        .pending_reasons
        .iter()
        .any(|reason| reason.starts_with("pr_autofix_head_redrive:")));
    github.state().fetch_pr_health_result = Some(Ok(health.clone()));

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("authorize one held-head redrive");

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload held-head redrive")
        .expect("redriven attempt remains current");
    assert_eq!(current.id, unpublished.id);
    assert_eq!(current.generation, unpublished.generation);
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| { reason == "pr_autofix_head_redrive:validated-local-ready-head" }));
    assert!(
        state
            .agent_run_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("list repair runs")
            .is_empty(),
        "a publish re-drive must not start a fixer generation"
    );

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("repeat held-head recovery is idempotent");
    let repeated = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload repeat held-head redrive")
        .expect("repeat attempt remains current");
    assert_eq!(
        repeated
            .pending_reasons
            .iter()
            .filter(|reason| reason.as_str() == "pr_autofix_head_redrive:validated-local-ready-head")
            .count(),
        1,
        "the exact-head marker prevents duplicate re-drives"
    );
    let expected_updated_at = repeated.updated_at;
    let mut capped = repeated;
    capped.pending_reasons.extend([
        "pr_autofix_head_redrive_retry:validated-local-ready-head:2".to_string(),
        "pr_autofix_head_redrive_retry:validated-local-ready-head:3".to_string(),
    ]);
    capped.updated_at += chrono::Duration::microseconds(1);
    let capped = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: capped,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist exhausted held-head retry history")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("held-head retry history must apply, got {outcome:?}"),
    };
    let capped_updated_at = capped.updated_at;
    github.state().fetch_pr_health_result = Some(Ok(health.clone()));
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("capped held-head retry stays held"),
        0
    );
    let capped = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload capped held-head retry")
        .expect("capped held-head retry remains current");
    assert_eq!(capped.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(capped.updated_at, capped_updated_at);
    assert!(capped.settled_at.is_none());

    assert!(matches!(
        state
            .branch_update_repo
            .release_target_lease(&target_identity, &foreign_owner, foreign_epoch)
            .await
            .expect("release foreign held-head target lease"),
        crate::domain::repositories::GitAuthorityCasOutcome::Applied { .. }
    ));
    let messages_before = state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("load baseline held-head messages")
        .len();

    let expected_updated_at = capped.updated_at;
    let mut changed_head = capped;
    changed_head.repair_head_commit = Some("validated-new-ready-head".to_string());
    changed_head.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: changed_head,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist changed repair head")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("changed repair head must apply, got {outcome:?}"),
    }
    github.state().fetch_pr_health_result = Some(Ok(health));
    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("changed repair head re-arms held-head retry");
    let resumed = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload re-armed held-head retry")
        .expect("re-armed held-head retry remains current");
    assert!(resumed
        .pending_reasons
        .iter()
        .any(|reason| reason == "pr_autofix_head_redrive:validated-new-ready-head"));
    assert!(resumed
        .pending_reasons
        .iter()
        .any(|reason| { reason == "pr_autofix_head_redrive_retry:validated-new-ready-head:1" }));
    assert_eq!(resumed.id, unpublished.id);
    assert_eq!(resumed.generation, unpublished.generation);
    assert!(matches!(
        resumed.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ));

    assert_eq!(
        resumed
            .pending_reasons
            .iter()
            .filter(|reason| reason.as_str() == "pr_autofix_head_redrive:validated-new-ready-head")
            .count(),
        1,
        "successful retry never duplicates the authorization marker"
    );
    let first_effect = state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&resumed.id)
        .await
        .expect("load held-head repair effects")
        .expect("resumed publish continuation should own one durable effect");
    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("in-flight held-head continuation remains idempotent");
    let repeated_effect = state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&resumed.id)
        .await
        .expect("reload held-head repair effects")
        .expect("in-flight publish effect remains open");
    assert_eq!(repeated_effect.id, first_effect.id);
    assert!(state
        .agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("load held-head repair runs")
        .is_empty());
    assert_eq!(
        state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("load held-head repair messages")
            .len(),
        messages_before,
        "held-head resume retries must not enqueue another fixer assignment"
    );
}

/// End-to-end shape of the PR #1018 deadlock: a base update ran inside the attempt (advancing the
/// workspace base past the attempt's dispatch target and producing a local merge commit), the
/// fixer then misclassified its completion, and the workspace parked on a health hold. The
/// recorded base-update head alone must authorize exactly one durable publish redrive.
#[tokio::test]
async fn ready_health_hold_with_only_base_update_head_marks_one_durable_redrive() {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(130).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;
    let health = failing_check_pr_health("remote-ready-head", "Rust Tests");
    let fingerprint = health_fingerprint(684, &health);
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health.clone()));
    state.github_service =
        Some(Arc::clone(&github) as Arc<dyn crate::domain::services::GithubServiceTrait>);
    block_pr_autofix_attempt_with_fingerprint(&state, &conversation_id, Some(fingerprint)).await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("park unchanged health");
    let held = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load health-held attempt")
        .expect("health-held attempt exists");
    assert_eq!(held.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(held.repair_head_commit.is_none());

    // The base update the fixer ran itself: workspace base moves, attempt keeps its dispatch
    // target, and the only local head is the resulting merge commit.
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load base-update workspace")
        .expect("base-update workspace exists");
    workspace.base_commit = Some("base-after-in-attempt-update".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("advance the workspace base like a completed base update");

    let expected_updated_at = held.updated_at;
    let mut with_base_update = held.clone();
    with_base_update.target_base_commit = Some("dispatch-time-base".to_string());
    with_base_update.base_update_head_commit = Some("base-update-merge-head".to_string());
    with_base_update.updated_at += chrono::Duration::microseconds(1);
    let with_base_update = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: with_base_update,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist in-attempt base-update evidence")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("base-update evidence must apply, got {outcome:?}"),
    };

    github.state().fetch_pr_health_result = Some(Ok(health.clone()));
    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("authorize one base-update redrive");

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload base-update redrive")
        .expect("redriven attempt remains current");
    assert_eq!(current.id, with_base_update.id);
    assert_eq!(current.generation, with_base_update.generation);
    assert!(
        current
            .pending_reasons
            .iter()
            .any(|reason| reason == "pr_autofix_head_redrive:base-update-merge-head"),
        "the base-update head must authorize the redrive on its own"
    );
    assert!(
        state
            .agent_run_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("list repair runs")
            .is_empty(),
        "a publish re-drive must not start a fixer generation"
    );

    github.state().fetch_pr_health_result = Some(Ok(health));
    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("repeat base-update recovery is idempotent");
    let repeated = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload repeat base-update redrive")
        .expect("repeat attempt remains current");
    assert_eq!(
        repeated
            .pending_reasons
            .iter()
            .filter(|reason| reason.as_str() == "pr_autofix_head_redrive:base-update-merge-head")
            .count(),
        1,
        "the exact-head marker prevents duplicate re-drives"
    );
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
    // No target base commit means the base cannot have advanced, so nothing but GitHub could
    // authorize another generation — and there is no GitHub service to ask.
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
    // The check now compares against health.sync_state.base_ref_oid (live GitHub evidence) rather
    // than workspace.base_commit (the diff baseline), so this test needs a seeded workspace + real
    // GitHub mock to reach the health-fetch gate.
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(71).await;
    let mut health = failing_check_pr_health("head-sha", "Rust Tests");
    // GitHub reports a base that differs from the attempt's dispatch-time target — base has moved.
    health.sync_state.base_ref_oid = Some("observed-base-b".to_string());
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    state.github_service = Some(github as Arc<dyn crate::domain::services::GithubServiceTrait>);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let mut attempt = blocked_pr_autofix_attempt(&conversation_id, "ci:Clippy:failure");
    // The attempt targets the older base; GitHub now reports a newer one.
    attempt.target_base_commit = Some("original-base-a".to_string());

    // ProceedRetargeted carries the observed OID so the successor is targeted at it; without
    // that retarget the same base movement would re-authorize a successor on every evaluation.
    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::ProceedRetargeted {
            observed_base_commit: "observed-base-b".to_string()
        }
    );
}

#[tokio::test]
async fn supersede_base_skew_does_not_bypass_hold_when_observed_base_matches_target() {
    // Falsifying test for review blocker 1: after the supersede/defer routes, workspace.base_commit
    // is the branch-point while the attempt carries the observed base. Before the fix,
    // repair_base_advanced compared against workspace.base_commit and permanently answered true,
    // short-circuiting HoldUnchanged and spending extra fixer generations on unchanged PR health.
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(204).await;
    let mut health = failing_check_pr_health("head-sha", "Rust Tests");
    let fingerprint = health_fingerprint(684, &health);
    // GitHub reports the same base the attempt was targeted at — no base movement.
    health.sync_state.base_ref_oid = Some("observed-base".to_string());
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    state.github_service = Some(github as Arc<dyn crate::domain::services::GithubServiceTrait>);
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    // Supersede/defer shape: attempt targets the observed base; workspace stays at branch point.
    workspace.base_commit = Some("branch-point".to_string());
    let mut attempt = blocked_pr_autofix_attempt(&conversation_id, &fingerprint);
    attempt.target_base_commit = Some("observed-base".to_string());

    // Without the fix, repair_base_advanced(attempt, workspace) is permanently true because
    // "observed-base" != "branch-point", so it returns Proceed(None) instead of HoldUnchanged.
    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::HoldUnchanged
    );
}

#[tokio::test]
async fn blocked_repair_retry_successor_inherits_predecessor_target_base_commit() {
    // Falsifying test for review blocker 2: the auto-retry successor was built with
    // workspace.base_commit (the diff baseline / branch point), but after the supersede/defer
    // routes the attempt carries the observed base in target_base_commit. The successor must
    // inherit the predecessor's target so completion validates against the right base.
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(205).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;

    // Transition the generation to Blocked with a skewed state: attempt targets the observed base,
    // workspace stays at the branch point (the supersede/defer shape after the poller fix).
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load attempt")
        .expect("attempt exists");
    let expected_updated_at = attempt.updated_at;
    let mut blocked = attempt.clone();
    blocked.source = AgentWorkspaceRepairSource::PrAutofix;
    blocked.phase = AgentWorkspaceRepairPhase::Blocked;
    blocked.target_base_commit = Some("observed-base-sha".to_string());
    blocked.pr_autofix_health_fingerprint = None; // no fingerprint → Proceed(None) immediately
    blocked.blocker = Some("transient_ci".to_string());
    blocked.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1_000);
    let blocked = match state
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
        .expect("transition to blocked")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(a) => a,
        outcome => panic!("must apply, got {outcome:?}"),
    };

    // Workspace base stays at branch-point, distinct from attempt's target.
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    workspace.base_commit = Some("branch-point-sha".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist branch-point workspace");

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("recovery pass runs");

    let successor = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load successor attempt")
        .expect("successor attempt exists");
    assert_ne!(
        successor.id, blocked.id,
        "a new generation must have been started"
    );
    assert_eq!(
        successor.target_base_commit.as_deref(),
        Some("observed-base-sha"),
        "successor must inherit the predecessor's target, not workspace.base_commit"
    );
    assert_ne!(
        successor.target_base_commit.as_deref(),
        Some("branch-point-sha"),
        "branch-point workspace base must not be propagated to the successor"
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

    // The borrowed PR resolves, so evaluation advances past PR identity and fails on the missing
    // project instead of on an unresolved PR number.
    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::Withhold("project_missing")
    );

    // A workspace bound to a different ideation session must not borrow this plan branch's PR.
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

    // An unreadable workspace is not evidence that the PR changed; it must never look like
    // "health changed", which is the one answer that spends another agent.
    assert_eq!(
        evaluate_pr_autofix_successor(&state, &attempt, &workspace).await,
        PrAutofixSuccessorDecision::Withhold("workspace_path_unresolved")
    );
}

async fn evaluate_successor_with_heads(
    suffix: u8,
    remote_head: Option<&str>,
    repair_head: Option<&str>,
    preserve_fingerprint: bool,
) -> PrAutofixSuccessorDecision {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(suffix).await;
    let mut health = failing_check_pr_health(remote_head.unwrap_or("unused-head"), "Rust Tests");
    health.sync_state.head_ref_oid = remote_head.map(str::to_string);
    let fingerprint = health_fingerprint(684, &health);
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    state.github_service = Some(github as Arc<dyn crate::domain::services::GithubServiceTrait>);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load successor decision workspace")
        .expect("successor decision workspace exists");
    let mut attempt = blocked_pr_autofix_attempt(
        &conversation_id,
        if preserve_fingerprint {
            &fingerprint
        } else {
            "github_pr_autofix:684:checks:different"
        },
    );
    // No target_base_commit means repair_base_advanced is false vacuously; these fixtures exercise
    // the head-publication and fingerprint paths, not the base-moved escape hatch.
    attempt.repair_head_commit = repair_head.map(str::to_string);

    evaluate_pr_autofix_successor(&state, &attempt, &workspace).await
}

#[tokio::test]
async fn unchanged_pr_health_with_unpublished_repair_head_redrives_publish() {
    assert_eq!(
        evaluate_successor_with_heads(75, Some("remote-head"), Some("local-head"), true).await,
        PrAutofixSuccessorDecision::RedrivePublish
    );
}

#[test]
fn unpublished_repair_head_predicate_trims_and_fails_closed() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(74),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );

    attempt.repair_head_commit = Some(" local-head ".to_string());
    assert!(held_repair_has_unpublished_head(
        &attempt,
        Some("remote-head")
    ));
    assert!(!held_repair_has_unpublished_head(
        &attempt,
        Some(" local-head ")
    ));
    assert!(held_repair_has_unpublished_head(
        &attempt,
        Some("local-head")
    ));
    assert!(!held_repair_has_unpublished_head(&attempt, Some("   ")));

    attempt.repair_head_commit = Some("   ".to_string());
    assert!(!held_repair_has_unpublished_head(
        &attempt,
        Some("remote-head")
    ));
    attempt.repair_head_commit = None;
    assert!(!held_repair_has_unpublished_head(
        &attempt,
        Some("remote-head")
    ));
}

#[test]
fn unpublished_publish_continuation_requires_an_exact_current_head_marker() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(75),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );

    assert!(!agent_workspace_repair_owns_unpublished_publish_continuation(&attempt));
    attempt.repair_head_commit = Some("   ".to_string());
    assert!(!agent_workspace_repair_owns_unpublished_publish_continuation(&attempt));

    attempt.repair_head_commit = Some(" local-head ".to_string());
    attempt.pending_reasons = vec!["pr_autofix_head_redrive:other-head".to_string()];
    assert!(!agent_workspace_repair_owns_unpublished_publish_continuation(&attempt));

    attempt.pending_reasons = vec!["pr_autofix_head_redrive:local-head".to_string()];
    assert!(agent_workspace_repair_owns_unpublished_publish_continuation(&attempt));
}

#[tokio::test]
async fn unchanged_pr_health_without_proven_unpublished_output_still_holds() {
    for (suffix, remote_head, repair_head) in [
        (76, Some("same-head"), Some("same-head")),
        (77, Some("remote-head"), None),
        (78, None, Some("local-head")),
    ] {
        assert_eq!(
            evaluate_successor_with_heads(suffix, remote_head, repair_head, true).await,
            PrAutofixSuccessorDecision::HoldUnchanged,
            "missing or already-published output must stay fail closed"
        );
    }
}

#[tokio::test]
async fn changed_pr_health_still_authorizes_a_successor_when_local_output_exists() {
    assert!(matches!(
        evaluate_successor_with_heads(79, Some("remote-head"), Some("local-head"), false).await,
        PrAutofixSuccessorDecision::Proceed(Some(_))
    ));
}

/// The base-update shape the deadlock actually takes: the attempt still targets its dispatch-time
/// base, the workspace base has moved because a base update ran *inside* this attempt, and the
/// only local head is that update's merge commit. Every other fixture leaves `target_base_commit`
/// at `None`, which makes `repair_base_advanced` false vacuously.
async fn evaluate_successor_after_in_attempt_base_update(
    suffix: u8,
    preserve_fingerprint: bool,
) -> PrAutofixSuccessorDecision {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(suffix).await;
    let health = failing_check_pr_health("remote-head", "Rust Tests");
    let fingerprint = health_fingerprint(684, &health);
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    state.github_service = Some(github as Arc<dyn crate::domain::services::GithubServiceTrait>);
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load base-advanced successor workspace")
        .expect("base-advanced successor workspace exists");

    let mut attempt = blocked_pr_autofix_attempt(
        &conversation_id,
        if preserve_fingerprint {
            &fingerprint
        } else {
            "github_pr_autofix:684:checks:different"
        },
    );
    attempt.target_base_commit = Some("dispatch-time-base".to_string());
    // No validated completion — only the head the backend recorded for the in-attempt update.
    attempt.base_update_head_commit = Some("base-update-merge-head".to_string());
    workspace.base_commit = Some("base-after-in-attempt-update".to_string());
    assert_ne!(attempt.target_base_commit, workspace.base_commit);

    evaluate_pr_autofix_successor(&state, &attempt, &workspace).await
}

#[tokio::test]
async fn in_attempt_base_update_still_redrives_publish_when_health_is_unchanged() {
    // Falsifying test for the narrowed `repair_base_advanced` guard: before it, the self-inflicted
    // base advance short-circuited to `Proceed(None)` and the recorded head was never published.
    assert_eq!(
        evaluate_successor_after_in_attempt_base_update(126, true).await,
        PrAutofixSuccessorDecision::RedrivePublish
    );
}

#[tokio::test]
async fn in_attempt_base_update_with_changed_health_still_takes_the_successor_path() {
    assert!(matches!(
        evaluate_successor_after_in_attempt_base_update(127, false).await,
        PrAutofixSuccessorDecision::Proceed(Some(_))
    ));
}

#[test]
fn unpublished_local_head_prefers_validated_completion_then_base_update_evidence() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(128),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );

    assert_eq!(attempt.unpublished_local_head(), None);

    attempt.base_update_head_commit = Some("  base-update-head  ".to_string());
    assert_eq!(attempt.unpublished_local_head(), Some("base-update-head"));

    attempt.repair_head_commit = Some("   ".to_string());
    assert_eq!(
        attempt.unpublished_local_head(),
        Some("base-update-head"),
        "a whitespace-only completion head must fall through, not shadow real evidence"
    );

    attempt.repair_head_commit = Some(" validated-head ".to_string());
    assert_eq!(attempt.unpublished_local_head(), Some("validated-head"));

    attempt.base_update_head_commit = Some("   ".to_string());
    attempt.repair_head_commit = None;
    assert_eq!(attempt.unpublished_local_head(), None);
}

#[test]
fn base_update_head_alone_counts_as_an_unpublished_head() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(129),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );

    attempt.base_update_head_commit = Some("base-update-head".to_string());
    assert!(held_repair_has_unpublished_head(
        &attempt,
        Some("remote-head")
    ));
    assert!(!held_repair_has_unpublished_head(
        &attempt,
        Some("base-update-head")
    ));
    assert!(!held_repair_has_unpublished_head(&attempt, Some("   ")));
    assert!(!held_repair_has_unpublished_head(&attempt, None));

    // The marker the durable redrive writes must key on the same value.
    attempt.pending_reasons = vec!["pr_autofix_head_redrive:base-update-head".to_string()];
    assert!(agent_workspace_repair_owns_unpublished_publish_continuation(&attempt));
}

async fn mark_blocked_pr_autofix_as_unpublished(
    state: &AppState,
    conversation_id: &ChatConversationId,
    repair_head: &str,
    retry_streak: u32,
) -> AgentWorkspaceRepairAttempt {
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load blocked PR autofix to mark unpublished")
        .expect("blocked PR autofix exists");
    let expected_updated_at = attempt.updated_at;
    attempt.repair_head_commit = Some(repair_head.to_string());
    attempt.pending_reasons = if retry_streak == 0 {
        Vec::new()
    } else {
        vec![format!("auto_retry_blocked_repair:{retry_streak}")]
    };
    attempt.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist unpublished PR autofix output")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("unpublished PR autofix checkpoint must apply, got {outcome:?}"),
    }
}

async fn seed_unpublished_blocked_pr_autofix(
    suffix: u8,
    retry_streak: u32,
) -> (
    AppState,
    ChatConversationId,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let (mut state, conversation_id, worktree_parent, project_dir) =
        seed_pr_autofix_health_workspace(suffix).await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load unpublished-output workspace")
        .expect("unpublished-output workspace exists");
    workspace.auto_publish_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("disable automatic publication in redrive fixture");
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;
    let health = failing_check_pr_health("remote-head", "Rust Tests");
    let fingerprint = health_fingerprint(684, &health);
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    state.github_service = Some(github as Arc<dyn crate::domain::services::GithubServiceTrait>);
    block_pr_autofix_attempt_with_fingerprint(&state, &conversation_id, Some(fingerprint)).await;
    mark_blocked_pr_autofix_as_unpublished(
        &state,
        &conversation_id,
        "validated-local-head",
        retry_streak,
    )
    .await;
    (state, conversation_id, worktree_parent, project_dir)
}

#[tokio::test]
async fn blocked_unpublished_pr_autofix_redrives_the_existing_publish_boundary() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_unpublished_blocked_pr_autofix(80, 0).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("re-drive unpublished repair output"),
        1
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load re-driven repair")
        .expect("re-driven repair remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == "auto_retry_blocked_repair:1"));
    assert!(!current.pending_reasons.iter().any(|reason| {
        reason
        == crate::application::agent_workspace_publish_repair_state::UNCHANGED_HEALTH_REPAIR_REASON
    }));
    assert!(state
        .agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("list repair runs")
        .is_empty());
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publish redrive events");
    assert!(events
        .iter()
        .any(|event| event.step == "repair_publish_redrive"));
}

#[tokio::test]
async fn blocked_unpublished_pr_autofix_redrive_stops_at_workspace_review() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_unpublished_blocked_pr_autofix(85, 0).await;
    state
        .review_settings_repo
        .update_settings(&crate::domain::review::ReviewSettings {
            require_workspace_review: true,
            ..crate::domain::review::ReviewSettings::default()
        })
        .await
        .expect("require Workspace Review for publish redrive");
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load review-gated redrive workspace")
        .expect("review-gated redrive workspace exists");
    std::fs::write(
        std::path::Path::new(&workspace.worktree_path).join("review-gated-repair.md"),
        "review this completed repair\n",
    )
    .expect("write review-gated repair output");
    recovery_git(
        std::path::Path::new(&workspace.worktree_path),
        &["add", "review-gated-repair.md"],
    );
    recovery_git(
        std::path::Path::new(&workspace.worktree_path),
        &["commit", "-m", "review-gated repair"],
    );
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .expect("load review-gated redrive project")
        .expect("review-gated redrive project exists");
    let target = resolve_review_target(&workspace, &project)
        .await
        .expect("resolve review-gated redrive target")
        .expect("review-gated redrive has reviewable changes");
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor(conversation_id.clone(), &target))
        .await
        .expect("seed active Workspace Review for redrive target");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("re-drive unpublished output through Workspace Review gate"),
        1
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load review-gated redrive")
        .expect("review-gated redrive remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::AwaitingReview);
    assert_ne!(
        current.phase,
        AgentWorkspaceRepairPhase::ContinuationPending
    );
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == "auto_retry_blocked_repair:1"));
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("load redrive review monitor")
        .expect("redrive review monitor remains durable");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Reviewing);
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Reviewing
    );
}

#[tokio::test]
async fn exhausted_unpublished_publish_redrive_hands_off_to_a_human() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_unpublished_blocked_pr_autofix(81, 3).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("park exhausted unpublished repair output"),
        1
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load exhausted publish redrive")
        .expect("exhausted publish redrive remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(current.pending_reasons.iter().any(|reason| reason
        == crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON));
    assert!(is_blocked_and_not_auto_retryable(&current));
    let notifications = state
        .notification_repo
        .list(None, None, 20)
        .await
        .expect("list exhausted redrive notifications");
    assert!(notifications
        .notifications
        .iter()
        .any(|notification| notification
            .body
            .as_deref()
            .is_some_and(|body| body.contains("re-drove publication"))));
}

#[tokio::test]
async fn exhausted_publish_redrive_checks_each_repair_head_only_once() {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_unpublished_blocked_pr_autofix(86, 3).await;
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(failing_check_pr_health(
        "validated-local-head",
        "Rust Tests",
    )));
    state.github_service =
        Some(github.clone() as Arc<dyn crate::domain::services::GithubServiceTrait>);

    for _ in 0..2 {
        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("bound exhausted publish re-drive health check"),
            0
        );
    }

    assert_eq!(github.state().fetch_pr_health_calls, 1);
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load exhausted checked repair")
        .expect("exhausted checked repair remains current");
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == "exhausted_publish_redrive_checked:validated-local-head"));

    mark_blocked_pr_autofix_as_unpublished(&state, &conversation_id, "new-local-head", 3).await;
    github.state().fetch_pr_health_result =
        Some(Ok(failing_check_pr_health("new-local-head", "Rust Tests")));
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recheck changed exhausted repair head"),
        0
    );
    assert_eq!(github.state().fetch_pr_health_calls, 2);
}

async fn seed_publish_continuation_with_lease(
    suffix: u8,
) -> (
    AppState,
    ChatConversationId,
    tempfile::TempDir,
    tempfile::TempDir,
    GitTargetIdentity,
    GitTargetLeaseOwner,
    u64,
) {
    let (state, conversation_id, worktree_parent, project_dir) =
        seed_pr_autofix_health_workspace(suffix).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load continuation workspace")
        .expect("continuation workspace exists");
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
            reason: "seed publish continuation lease recovery".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start publish continuation fixture");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut attempt) = started else {
        panic!("publish continuation fixture must start");
    };
    let identity = GitService::canonical_target_identity(
        std::path::Path::new(&workspace.worktree_path),
        &workspace.branch_name,
    )
    .await
    .expect("resolve continuation target identity");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner: owner.clone(),
        })
        .await
        .expect("acquire continuation fixture lease")
    else {
        panic!("continuation fixture lease must be newly acquired");
    };
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::ContinuationPending;
    attempt.git_common_dir = Some(identity.git_common_dir().to_string_lossy().into_owned());
    attempt.target_ref = Some(identity.full_ref().to_string());
    attempt.target_identity_version = Some(
        crate::application::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION,
    );
    attempt.target_lease_epoch = Some(fencing_epoch);
    attempt.updated_at += chrono::Duration::microseconds(1);
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at,
                next_phase: AgentWorkspaceRepairPhase::ContinuationPending,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("checkpoint publish continuation fixture"),
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
    (
        state,
        conversation_id,
        worktree_parent,
        project_dir,
        identity,
        owner,
        fencing_epoch,
    )
}

#[tokio::test]
async fn lost_continuation_lease_reacquires_before_publish_reconciliation() {
    let (state, conversation_id, _worktree_parent, _project_dir, identity, owner, old_epoch) =
        seed_publish_continuation_with_lease(82).await;
    state
        .branch_update_repo
        .release_target_lease(&identity, &owner, old_epoch)
        .await
        .expect("release continuation fixture lease");
    let _busy_guard =
        try_acquire_agent_workspace_repair_publish_continuation_guard(&conversation_id)
            .expect("hold publish guard after lease reacquire");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("reacquire lost continuation lease"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load reacquired continuation")
        .expect("reacquired continuation remains current");
    assert!(matches!(
        current.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ));
    assert!(current
        .target_lease_epoch
        .is_some_and(|epoch| epoch > old_epoch));
    assert!(!current
        .pending_reasons
        .iter()
        .any(|reason| reason.starts_with("continuation_recovery_failure:")));
}

#[tokio::test]
async fn busy_continuation_target_escalates_to_blocked_within_the_recovery_cap() {
    let (state, conversation_id, _worktree_parent, _project_dir, identity, owner, old_epoch) =
        seed_publish_continuation_with_lease(83).await;
    state
        .branch_update_repo
        .release_target_lease(&identity, &owner, old_epoch)
        .await
        .expect("release continuation fixture lease");
    let foreign_owner = GitTargetLeaseOwner::branch_update("foreign-task", "foreign-update");
    assert!(matches!(
        state
            .branch_update_repo
            .acquire_target_lease(AcquireGitTargetLease {
                identity: identity.clone(),
                owner: foreign_owner.clone(),
            })
            .await
            .expect("acquire continuation target for foreign owner"),
        AcquireGitTargetLeaseOutcome::Acquired { .. }
    ));

    for expected_recovered in [0, 0, 1] {
        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("bound busy continuation recovery"),
            expected_recovered
        );
    }
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load bounded busy continuation")
        .expect("bounded busy continuation remains actionable");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    let lease = state
        .branch_update_repo
        .get_target_lease(&identity)
        .await
        .expect("load foreign target lease")
        .expect("foreign target lease remains durable");
    assert_eq!(lease.owner(), &foreign_owner);
    assert!(!lease.is_released());
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list continuation recovery events");
    assert!(events.iter().any(|event| {
        event.step
        == crate::application::agent_workspace_publish_recovery::CONTINUATION_RECOVERY_BLOCKED_STEP
    }));
}

#[tokio::test]
async fn generic_continuation_error_escalates_to_blocked_within_the_recovery_cap() {
    let (state, conversation_id, _worktree_parent, _project_dir, identity, owner, epoch) =
        seed_publish_continuation_with_lease(88).await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load generic-error continuation workspace")
        .expect("generic-error continuation workspace exists");
    workspace.project_id = ProjectId::from_string("missing-continuation-project".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("make continuation project lookup fail");

    for expected_recovered in [0, 0, 1] {
        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("bound generic continuation recovery failure"),
            expected_recovered
        );
    }
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load bounded generic-error continuation")
        .expect("bounded generic-error continuation remains actionable");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(current
        .blocker
        .as_deref()
        .is_some_and(|blocker| blocker.contains("tried 3 times")));
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await
        .expect("check generic-error continuation effects")
        .is_none());
    let lease = state
        .branch_update_repo
        .get_target_lease(&identity)
        .await
        .expect("load generic-error continuation lease")
        .expect("generic-error continuation lease remains durable");
    assert_eq!(lease.owner(), &owner);
    assert_eq!(lease.fencing_epoch(), epoch);
    assert!(lease.is_released());
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list generic continuation recovery events");
    assert!(events.iter().any(|event| {
        event.step
        == crate::application::agent_workspace_publish_recovery::CONTINUATION_RECOVERY_BLOCKED_STEP
    }));
}

#[tokio::test]
async fn open_continuation_effect_fences_reacquire_and_block_escalation() {
    let (state, conversation_id, _worktree_parent, _project_dir, identity, owner, old_epoch) =
        seed_publish_continuation_with_lease(84).await;
    state
        .branch_update_repo
        .release_target_lease(&identity, &owner, old_epoch)
        .await
        .expect("release continuation fixture lease");
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load effect-fenced continuation")
        .expect("effect-fenced continuation exists");
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: attempt.id.clone(),
                generation: attempt.generation,
                expected_phase: attempt.phase,
                expected_attempt_updated_at: attempt.updated_at,
                effect: AgentWorkspaceRepairEffect::new(
                    attempt.id.clone(),
                    AgentWorkspaceRepairEffectKind::PushBranch,
                    "effect-fenced-lost-lease",
                    chrono::Utc::now(),
                ),
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("create open continuation effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    for _ in 0..4 {
        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("keep open continuation effect fenced"),
            0
        );
    }
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload effect-fenced continuation")
        .expect("effect-fenced continuation remains current");
    assert!(matches!(
        current.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ));
    assert_eq!(current.target_lease_epoch, Some(old_epoch));
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await
        .expect("reload fenced open effect")
        .is_some());
    assert!(!current
        .pending_reasons
        .iter()
        .any(|reason| reason.starts_with("continuation_recovery_failure:")));
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == "continuation_open_effect_attention_required"));
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list effect-fenced continuation events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "continuation_open_effect_attention_required")
            .count(),
        1
    );
    let notifications = state
        .notification_repo
        .list(None, None, 20)
        .await
        .expect("list effect-fenced continuation notifications");
    assert_eq!(
        notifications
            .notifications
            .iter()
            .filter(|notification| notification
                .dedupe_key
                .as_deref()
                .is_some_and(|key| key.starts_with("repair_open_effect:")))
            .count(),
        1
    );
}

#[tokio::test]
async fn observed_open_push_effect_is_reconciled_before_lease_reacquire() {
    let (state, conversation_id, _worktree_parent, project_dir, identity, owner, old_epoch) =
        seed_publish_continuation_with_lease(87).await;
    let remote = tempfile::tempdir().expect("create repair push remote");
    recovery_git(remote.path(), &["init", "--bare"]);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load effect reconciliation workspace")
        .expect("effect reconciliation workspace exists");
    recovery_git(
        std::path::Path::new(&workspace.worktree_path),
        &[
            "remote",
            "add",
            "origin",
            remote.path().to_str().expect("remote path"),
        ],
    );
    recovery_git(
        std::path::Path::new(&workspace.worktree_path),
        &["push", "-u", "origin", &workspace.branch_name],
    );
    let intended_head = recovery_git(
        std::path::Path::new(&workspace.worktree_path),
        &["rev-parse", "HEAD"],
    );
    let remote_tracking_ref = format!("refs/remotes/origin/{}", workspace.branch_name);
    recovery_git(
        std::path::Path::new(&workspace.worktree_path),
        &["update-ref", "-d", &remote_tracking_ref],
    );
    assert!(recovery_git(
        std::path::Path::new(&workspace.worktree_path),
        &["for-each-ref", "--format=%(refname)", &remote_tracking_ref],
    )
    .is_empty());
    state
        .branch_update_repo
        .release_target_lease(&identity, &owner, old_epoch)
        .await
        .expect("release effect reconciliation lease");
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load effect reconciliation continuation")
        .expect("effect reconciliation continuation exists");
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    attempt.updated_at += chrono::Duration::microseconds(1);
    let attempt = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint initialized push effect continuation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("initialized push effect continuation must apply, got {outcome:?}"),
    };
    let idempotency_key = "effect-reconciled-after-lost-lease";
    let mut effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        idempotency_key,
        chrono::Utc::now(),
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = Some(intended_head.clone());
    effect.expected_remote_absent = true;
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: attempt.id.clone(),
                generation: attempt.generation,
                expected_phase: attempt.phase,
                expected_attempt_updated_at: attempt.updated_at,
                effect,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("create initialized open push effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));
    let _busy_guard =
        try_acquire_agent_workspace_repair_publish_continuation_guard(&conversation_id)
            .expect("hold continuation after effect reconciliation");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("reconcile observed push effect before reacquire"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload effect-reconciled continuation")
        .expect("effect-reconciled continuation remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Continuing);
    assert!(current
        .target_lease_epoch
        .is_some_and(|epoch| epoch > old_epoch));
    let effect = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(idempotency_key)
        .await
        .expect("load reconciled push effect")
        .expect("reconciled push effect exists");
    assert_eq!(effect.status, AgentWorkspaceRepairEffectStatus::Observed);
    assert!(effect
        .receipt_json
        .as_deref()
        .is_some_and(|receipt| receipt.contains(&intended_head)));
    assert!(recovery_git(
        std::path::Path::new(&workspace.worktree_path),
        &["for-each-ref", "--format=%(refname)", &remote_tracking_ref],
    )
    .is_empty());
    drop(project_dir);
    drop(remote);
}

#[tokio::test]
async fn open_push_effect_reconciliation_requires_its_workspace_and_project() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(89);
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    let mut effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "missing-reconciliation-context",
        chrono::Utc::now(),
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = Some("intended-head".to_string());
    effect.expected_remote_absent = true;

    assert!(matches!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect.clone()).await,
        Err(crate::error::AppError::NotFound(_))
    ));

    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed reconciliation workspace without its project");
    assert!(state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("reload reconciliation workspace")
        .is_some());
    assert!(matches!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect).await,
        Err(crate::error::AppError::NotFound(_))
    ));
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&attempt.id)
        .await
        .expect("verify missing reconciliation context did not create an effect")
        .is_none());
}

#[tokio::test]
async fn open_push_effect_reconciliation_requires_persisted_canonical_identity() {
    let (state, conversation_id, _worktree_parent, _project_dir, identity, _owner, _epoch) =
        seed_publish_continuation_with_lease(90).await;
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load identity reconciliation continuation")
        .expect("identity reconciliation continuation exists");
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    let mut effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "missing-reconciliation-identity",
        chrono::Utc::now(),
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = Some("intended-head".to_string());
    effect.expected_remote_absent = true;

    attempt.git_common_dir = None;
    assert!(matches!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect.clone()).await,
        Err(crate::error::AppError::Conflict(_))
    ));

    attempt.git_common_dir = Some(identity.git_common_dir().to_string_lossy().into_owned());
    attempt.target_ref = None;
    assert!(matches!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect).await,
        Err(crate::error::AppError::Conflict(_))
    ));
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&attempt.id)
        .await
        .expect("verify invalid reconciliation identity did not create an effect")
        .is_none());
}

#[tokio::test]
async fn open_push_effect_reconciliation_stays_pending_outside_the_continuing_phase() {
    let (state, conversation_id, _worktree_parent, _project_dir, _identity, _owner, _epoch) =
        seed_publish_continuation_with_lease(92).await;
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load non-continuing reconciliation attempt")
        .expect("non-continuing reconciliation attempt exists");
    assert_eq!(
        attempt.phase,
        AgentWorkspaceRepairPhase::ContinuationPending
    );
    let mut effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "non-continuing-phase-bound",
        chrono::Utc::now(),
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    // Full exact-OID proof: if the `attempt.phase != Continuing` guard were ever widened to skip
    // termination, this proof alone would be enough to make the effect eligible for `NotApplied`.
    effect.intended_head_oid = Some("intended-head".to_string());
    effect.expected_remote_absent = true;
    let effect = match state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: attempt.phase,
            expected_attempt_updated_at: attempt.updated_at,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist non-continuing reconciliation effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => {
            panic!("expected non-continuing reconciliation effect to persist, got {outcome:?}")
        }
    };

    assert_eq!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect.clone())
            .await
            .expect("reconciliation outside Continuing phase must not error"),
        crate::application::publish_resilience::AgentWorkspaceRepairOpenPushEffectReconciliation::Pending
    );
    // `complete_repair_effect` CAS carries `expected_phase: Continuing`, so no effect can be
    // terminated from `ContinuationPending` regardless of remote proof; this proves the bound
    // rather than assuming it.
    let reloaded = state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&attempt.id)
        .await
        .expect("reload non-continuing reconciliation effect")
        .expect("non-continuing reconciliation effect remains open");
    assert_eq!(reloaded.status, AgentWorkspaceRepairEffectStatus::InFlight);
    assert_eq!(reloaded.updated_at, effect.updated_at);
    assert_eq!(reloaded.completed_at, None);
}

enum ReconciliationRemoteShape {
    /// No remote branch exists yet; the effect recorded `expected_remote_absent`.
    Absent,
    /// The remote is exactly at the OID the effect recorded as its pre-push precondition.
    MatchesPrecondition,
    /// A third party advanced the remote to a state that matches neither the recorded
    /// pre-push precondition nor the (never pushed) intended post-push head.
    Unrelated,
    /// The production wedged shape: the effect was reserved but its preconditions were never
    /// written, so no push was ever authorized for it. Aged past the quiescence bound.
    Uninitialized,
    /// Structurally identical to [`Self::Uninitialized`] but created just now, so the quiescence
    /// window has not elapsed and the effect must stay fenced.
    UninitializedTooYoung,
    /// Like [`Self::Unrelated`] — permanently ambiguous, so reconciliation always returns
    /// `Pending` — but with the effect created `age` ago. Models a future dead-effect shape the
    /// inert predicate does not recognize, which is what the terminal watchdog exists for.
    UnrelatedAged(chrono::Duration),
}

/// How far in the past an [`OpenPushEffectReconciliationFixture`] effect is created. The
/// uninitialized shapes need to sit outside the quiescence window (default 300s) to be classified.
const RECONCILIATION_FIXTURE_INERT_AGE: chrono::Duration = chrono::Duration::hours(6);

/// Builds a `Continuing` repair attempt with a lost target-lease authority and one durable,
/// still-open `InFlight` push effect whose exact-OID proof and observable remote state are
/// controlled by `shape`. The returned `TempDir` guards must stay alive for the caller's recovery
/// call, since the workspace's `origin` remote points into them.
struct OpenPushEffectReconciliationFixture {
    state: AppState,
    conversation_id: ChatConversationId,
    identity: GitTargetIdentity,
    _owner: GitTargetLeaseOwner,
    old_epoch: u64,
    attempt: AgentWorkspaceRepairAttempt,
    effect: AgentWorkspaceRepairEffect,
    // Held only to keep the fixture's real git worktree/project/remote directories alive for the
    // caller's lifetime; never read directly.
    _worktree_parent: tempfile::TempDir,
    _project_dir: tempfile::TempDir,
    _remote: tempfile::TempDir,
}

async fn seed_open_push_effect_reconciliation_fixture(
    suffix: u8,
    shape: ReconciliationRemoteShape,
) -> OpenPushEffectReconciliationFixture {
    let (state, conversation_id, worktree_parent, project_dir, identity, owner, old_epoch) =
        seed_publish_continuation_with_lease(suffix).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load reconciliation workspace")
        .expect("reconciliation workspace exists");
    let worktree_path = std::path::Path::new(&workspace.worktree_path);
    let remote = tempfile::tempdir().expect("create reconciliation remote");
    recovery_git(remote.path(), &["init", "--bare"]);
    recovery_git(
        worktree_path,
        &[
            "remote",
            "add",
            "origin",
            remote.path().to_str().expect("remote path"),
        ],
    );

    let pre_push_oid = if matches!(shape, ReconciliationRemoteShape::Absent) {
        None
    } else {
        recovery_git(
            worktree_path,
            &["push", "-u", "origin", &workspace.branch_name],
        );
        Some(recovery_git(worktree_path, &["rev-parse", "HEAD"]))
    };
    recovery_git(
        worktree_path,
        &["commit", "--allow-empty", "-m", "repair head"],
    );
    let intended_head = recovery_git(worktree_path, &["rev-parse", "HEAD"]);
    if matches!(
        shape,
        ReconciliationRemoteShape::Unrelated | ReconciliationRemoteShape::UnrelatedAged(_)
    ) {
        let concurrent = tempfile::tempdir().expect("create concurrent reconciliation clone");
        recovery_git(
            concurrent.path(),
            &["clone", remote.path().to_str().expect("remote path"), "."],
        );
        recovery_git(
            concurrent.path(),
            &["config", "user.email", "concurrent@example.com"],
        );
        recovery_git(
            concurrent.path(),
            &["config", "user.name", "Concurrent Writer"],
        );
        recovery_git(
            concurrent.path(),
            &["commit", "--allow-empty", "-m", "concurrent head"],
        );
        recovery_git(
            concurrent.path(),
            &[
                "push",
                "--force",
                "origin",
                &format!("HEAD:refs/heads/{}", workspace.branch_name),
            ],
        );
    }

    state
        .branch_update_repo
        .release_target_lease(&identity, &owner, old_epoch)
        .await
        .expect("release reconciliation fixture lease");
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load reconciliation continuation")
        .expect("reconciliation continuation exists");
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    attempt.updated_at += chrono::Duration::microseconds(1);
    let attempt = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint reconciliation continuation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("reconciliation continuation must apply, got {outcome:?}"),
    };

    let idempotency_key = format!("reconciliation-fixture-{suffix}");
    let created_at = match shape {
        ReconciliationRemoteShape::Uninitialized => {
            chrono::Utc::now() - RECONCILIATION_FIXTURE_INERT_AGE
        }
        ReconciliationRemoteShape::UnrelatedAged(age) => chrono::Utc::now() - age,
        _ => chrono::Utc::now(),
    };
    let mut effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        idempotency_key,
        created_at,
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    match shape {
        ReconciliationRemoteShape::Absent => {
            effect.intended_head_oid = Some(intended_head);
            effect.expected_remote_absent = true;
        }
        ReconciliationRemoteShape::MatchesPrecondition
        | ReconciliationRemoteShape::Unrelated
        | ReconciliationRemoteShape::UnrelatedAged(_) => {
            effect.intended_head_oid = Some(intended_head);
            effect.expected_remote_oid = pre_push_oid;
        }
        // Deliberately leaves every precondition unset. This is the exact shape a crash between
        // effect reservation and push-preflight initialization leaves behind, and it is what
        // wedged the reported production workspace.
        ReconciliationRemoteShape::Uninitialized
        | ReconciliationRemoteShape::UninitializedTooYoung => {}
    }
    let effect = match state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: attempt.phase,
            expected_attempt_updated_at: attempt.updated_at,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist reconciliation fixture effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("expected reconciliation fixture effect to persist, got {outcome:?}"),
    };

    OpenPushEffectReconciliationFixture {
        state,
        conversation_id,
        identity,
        _owner: owner,
        old_epoch,
        attempt,
        effect,
        _worktree_parent: worktree_parent,
        _project_dir: project_dir,
        _remote: remote,
    }
}

#[tokio::test]
async fn not_applied_push_effect_clears_the_fence_and_reacquires_the_lease() {
    let fixture = seed_open_push_effect_reconciliation_fixture(
        93,
        ReconciliationRemoteShape::MatchesPrecondition,
    )
    .await;
    let (state, conversation_id, identity, _old_epoch, effect) = (
        fixture.state,
        fixture.conversation_id,
        fixture.identity,
        fixture.old_epoch,
        fixture.effect,
    );
    let _busy_guard =
        try_acquire_agent_workspace_repair_publish_continuation_guard(&conversation_id)
            .expect("hold continuation guard while proving the fence clears");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("reconcile a not-applied push effect"),
        0
    );

    let reloaded_effect = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&effect.idempotency_key)
        .await
        .expect("reload not-applied push effect")
        .expect("not-applied push effect remains durable");
    assert_eq!(
        reloaded_effect.status,
        AgentWorkspaceRepairEffectStatus::Failed
    );
    assert!(reloaded_effect
        .last_error
        .as_deref()
        .is_some_and(|reason| reason.contains("never reached the remote")));
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&effect.attempt_id)
        .await
        .expect("check not-applied push effect fence")
        .is_none());

    // The fence is cleared (effect is Failed). Reacquisition is deferred to the next sweep via
    // the Noop return; no open-effect recovery pending reason is added for this pass.
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload not-applied continuation")
        .expect("not-applied continuation remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Continuing);
    assert!(
        !current
            .pending_reasons
            .iter()
            .any(|reason| reason.starts_with("continuation_open_effect_recovery:")),
        "not-applied pass must not add an open-effect recovery reason: {current:#?}"
    );
    assert!(state
        .branch_update_repo
        .get_target_lease(&identity)
        .await
        .expect("reload not-applied lease")
        .expect("not-applied lease remains durable")
        .active_mutation()
        .is_none());

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list not-applied continuation events");
    assert!(events
        .iter()
        .any(|event| event.step == "continuation_effect_not_applied"));
    assert!(!events
        .iter()
        .any(|event| event.step == "continuation_open_effect_attention_required"));
    assert!(!current
        .pending_reasons
        .iter()
        .any(|reason| reason == "continuation_open_effect_attention_required"));
}

#[tokio::test]
async fn not_applied_push_effect_does_not_re_raise_attention_when_already_escalated() {
    // Regression: when an attempt already carries CONTINUATION_OPEN_EFFECT_ATTENTION_REASON,
    // the not-applied arm must resolve the notification (via record_continuation_effect_not_applied)
    // and return Noop — it must NOT call record_open_effect_continuation_recovery_failure, which
    // would re-record the attention notification under the same dedupe key that was just resolved.
    let fixture = seed_open_push_effect_reconciliation_fixture(
        98,
        ReconciliationRemoteShape::MatchesPrecondition,
    )
    .await;
    let (state, conversation_id, attempt) =
        (fixture.state, fixture.conversation_id, fixture.attempt);

    // Escalate the attempt by injecting CONTINUATION_OPEN_EFFECT_ATTENTION_REASON into its
    // pending_reasons, simulating a previously escalated open-effect streak.
    let mut escalated = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load attempt to escalate")
        .expect("attempt exists to escalate");
    let expected_phase = escalated.phase;
    let expected_updated_at = escalated.updated_at;
    escalated
        .pending_reasons
        .push(CONTINUATION_OPEN_EFFECT_ATTENTION_REASON.to_string());
    escalated.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: escalated,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("escalate attempt with attention reason")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("escalation must apply, got {outcome:?}"),
    }

    let _busy_guard =
        try_acquire_agent_workspace_repair_publish_continuation_guard(&conversation_id)
            .expect("hold continuation guard for already-escalated not-applied test");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("not-applied pass on already-escalated attempt"),
        0
    );

    // The fence must be cleared.
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&attempt.id)
        .await
        .expect("check fence after escalated not-applied pass")
        .is_none());

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload after escalated not-applied pass")
        .expect("attempt remains after escalated not-applied pass");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Continuing);

    // No open-effect recovery credit must have been spent; the streak must not grow.
    assert!(
        !current
            .pending_reasons
            .iter()
            .any(|reason| reason.starts_with("continuation_open_effect_recovery:")),
        "not-applied pass must not increment the open-effect streak: {current:#?}"
    );

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events after escalated not-applied pass");
    assert!(
        events
            .iter()
            .any(|event| event.step == "continuation_effect_not_applied"),
        "not-applied event must be present"
    );
    // surface_open_effect_continuation_attention must NOT have been called by the not-applied
    // arm; if it were, it would append a continuation_open_effect_attention_required step.
    assert!(
        !events
            .iter()
            .any(|event| event.step == "continuation_open_effect_attention_required"),
        "no attention_required event must be appended by the not-applied pass: {events:#?}"
    );
}

#[tokio::test]
async fn not_applied_push_effect_covers_the_expected_remote_absent_shape() {
    let fixture =
        seed_open_push_effect_reconciliation_fixture(94, ReconciliationRemoteShape::Absent).await;
    let (state, conversation_id, effect) = (fixture.state, fixture.conversation_id, fixture.effect);
    let _busy_guard =
        try_acquire_agent_workspace_repair_publish_continuation_guard(&conversation_id)
            .expect("hold continuation guard for the absent-remote not-applied case");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("reconcile an absent-remote not-applied push effect"),
        0
    );

    let reloaded_effect = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&effect.idempotency_key)
        .await
        .expect("reload absent-remote not-applied push effect")
        .expect("absent-remote not-applied push effect remains durable");
    assert_eq!(
        reloaded_effect.status,
        AgentWorkspaceRepairEffectStatus::Failed
    );
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&effect.attempt_id)
        .await
        .expect("check absent-remote not-applied push effect fence")
        .is_none());
}

#[tokio::test]
async fn open_push_effect_reconciliation_stays_pending_for_an_unrelated_remote_oid() {
    let fixture =
        seed_open_push_effect_reconciliation_fixture(95, ReconciliationRemoteShape::Unrelated)
            .await;
    let (state, conversation_id, identity, attempt, effect) = (
        fixture.state,
        fixture.conversation_id,
        fixture.identity,
        fixture.attempt,
        fixture.effect,
    );

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("keep an unrelated-remote push effect fenced"),
        0
    );

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload unrelated-remote continuation")
        .expect("unrelated-remote continuation remains current");
    assert_eq!(current.id, attempt.id);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Continuing);
    assert_eq!(current.target_lease_epoch, attempt.target_lease_epoch);
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == "continuation_open_effect_recovery:1"));
    let reloaded_effect = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&effect.idempotency_key)
        .await
        .expect("reload unrelated-remote push effect")
        .expect("unrelated-remote push effect remains open");
    assert_eq!(
        reloaded_effect.status,
        AgentWorkspaceRepairEffectStatus::InFlight
    );
    assert!(state
        .branch_update_repo
        .get_target_lease(&identity)
        .await
        .expect("reload unrelated-remote lease")
        .expect("unrelated-remote lease remains durable")
        .is_released());
}

#[tokio::test]
async fn open_push_effect_reconciliation_stays_pending_while_its_mutation_claim_is_in_flight() {
    let (state, conversation_id, _worktree_parent, _project_dir, identity, owner, real_epoch) =
        seed_publish_continuation_with_lease(96).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load in-flight-claim reconciliation workspace")
        .expect("in-flight-claim reconciliation workspace exists");
    let worktree_path = std::path::Path::new(&workspace.worktree_path);
    let remote = tempfile::tempdir().expect("create in-flight-claim remote");
    recovery_git(remote.path(), &["init", "--bare"]);
    recovery_git(
        worktree_path,
        &[
            "remote",
            "add",
            "origin",
            remote.path().to_str().expect("remote path"),
        ],
    );
    recovery_git(
        worktree_path,
        &["push", "-u", "origin", &workspace.branch_name],
    );
    let pre_push_oid = recovery_git(worktree_path, &["rev-parse", "HEAD"]);
    recovery_git(
        worktree_path,
        &["commit", "--allow-empty", "-m", "repair head"],
    );
    let intended_head = recovery_git(worktree_path, &["rev-parse", "HEAD"]);

    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load in-flight-claim continuation")
        .expect("in-flight-claim continuation exists");
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    // A durable epoch that no longer matches the still-valid, still-owned real lease. This is
    // what makes `validate_agent_workspace_repair_target_lease` return `Conflict` and enter the
    // reconciliation branch, without releasing or otherwise disturbing the real lease/claim.
    attempt.target_lease_epoch = Some(real_epoch + 1);
    attempt.updated_at += chrono::Duration::microseconds(1);
    let attempt = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint in-flight-claim continuation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("in-flight-claim continuation must apply, got {outcome:?}"),
    };

    let idempotency_key = "in-flight-claim-guard-fixture";
    let mut effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        idempotency_key,
        chrono::Utc::now(),
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = Some(intended_head);
    effect.expected_remote_oid = Some(pre_push_oid);
    let effect = match state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: attempt.phase,
            expected_attempt_updated_at: attempt.updated_at,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist in-flight-claim guard effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("expected in-flight-claim guard effect to persist, got {outcome:?}"),
    };

    // The real lease (still at `real_epoch`, still owned by this attempt) has a live push claim,
    // simulating a push that may still be in flight right now.
    state
        .branch_update_repo
        .begin_git_mutation(BeginGitMutation {
            identity: identity.clone(),
            owner: owner.clone(),
            fencing_epoch: real_epoch,
            claim_id: format!("{}:push", effect.id),
            kind: GitMutationKind::Push,
        })
        .await
        .expect("reserve in-flight reconciliation claim");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("keep in-flight-claim continuation fenced"),
        0
    );

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload in-flight-claim continuation")
        .expect("in-flight-claim continuation remains current");
    assert_eq!(current.target_lease_epoch, Some(real_epoch + 1));
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == "continuation_open_effect_recovery:1"));
    let reloaded_effect = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(idempotency_key)
        .await
        .expect("reload in-flight-claim guard effect")
        .expect("in-flight-claim guard effect remains open");
    assert_eq!(
        reloaded_effect.status,
        AgentWorkspaceRepairEffectStatus::InFlight
    );
    assert!(
        state
            .branch_update_repo
            .get_target_lease(&identity)
            .await
            .expect("reload in-flight-claim guard lease")
            .expect("in-flight-claim guard lease remains durable")
            .active_mutation()
            .is_some(),
        "recovery must not touch the still-live mutation claim"
    );
}

#[tokio::test]
async fn continuation_recovery_failure_streak_stays_independent_of_the_open_effect_streak() {
    let fixture = seed_open_push_effect_reconciliation_fixture(
        97,
        ReconciliationRemoteShape::MatchesPrecondition,
    )
    .await;
    let (state, conversation_id) = (fixture.state, fixture.conversation_id);
    {
        let _busy_guard =
            try_acquire_agent_workspace_repair_publish_continuation_guard(&conversation_id)
                .expect("hold continuation guard while clearing the not-applied fence");
        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("clear the not-applied fence before the streak-independence check"),
            0
        );
    }
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload streak-independence continuation")
        .expect("streak-independence continuation remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Continuing);
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await
        .expect("confirm the not-applied fence is clear")
        .is_none());

    // Break the workspace's project lookup so every following continuation attempt fails for a
    // generic (non-open-effect) reason, exercising the independent `continuation_recovery_failure:`
    // streak instead of the open-effect streak the fence-clearing recovery pass just used.
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load streak-independence workspace")
        .expect("streak-independence workspace exists");
    workspace.project_id =
        ProjectId::from_string("missing-streak-independence-project".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("make streak-independence project lookup fail");

    for expected_recovered in [0, 0, 1] {
        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("bound the independent generic continuation recovery streak"),
            expected_recovered
        );
    }

    let blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load streak-independence blocked continuation")
        .expect("streak-independence blocked continuation remains actionable");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(blocked
        .blocker
        .as_deref()
        .is_some_and(|blocker| blocker.contains("tried 3 times")));
    assert!(blocked
        .pending_reasons
        .iter()
        .any(|reason| reason == "continuation_recovery_failure:1"));
    // The not-applied fence-clearing pass does not record an open-effect recovery reason;
    // only the independent generic continuation failures produce their own streak.
    assert!(!blocked
        .pending_reasons
        .iter()
        .any(|reason| reason.starts_with("continuation_open_effect_recovery:")));
    assert!(!blocked
        .pending_reasons
        .iter()
        .any(|reason| reason == "continuation_open_effect_attention_required"));
}

/// Binds a completed run of a given wall-clock cost as the current attempt's reservation, for
/// exercising the fingerprint-spend budget check.
async fn seed_completed_run_bound_to_attempt(
    state: &AppState,
    conversation_id: &ChatConversationId,
    attempt_id: &crate::domain::entities::AgentWorkspaceRepairAttemptId,
    minutes: i64,
) {
    let mut run = AgentRun::new(conversation_id.clone());
    run.started_at = chrono::Utc::now() - chrono::Duration::minutes(minutes);
    run.completed_at = Some(chrono::Utc::now());
    run.status = AgentRunStatus::Completed;
    let run_id = run.id.clone();
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed a finished repair run for fingerprint-spend accounting");

    let mut attempt = state
        .agent_workspace_repair_repo
        .get_repair_attempt(attempt_id)
        .await
        .expect("load attempt to bind spend run")
        .expect("attempt exists to bind spend run");
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.reserved_agent_run_id = Some(run_id);
    attempt.updated_at += chrono::Duration::microseconds(1);
    assert!(matches!(
        state
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
            .expect("bind spend run to attempt"),
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
}

#[tokio::test]
async fn blocked_pr_autofix_streak_rearms_once_on_changed_evidence_within_budget() {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(200).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;

    let old_health = failing_check_pr_health("head-streak-old", "Rust Tests");
    let old_fingerprint = health_fingerprint(684, &old_health);
    let blocked = block_pr_autofix_attempt_with_fingerprint(
        &state,
        &conversation_id,
        Some(old_fingerprint.clone()),
    )
    .await;

    let expected_updated_at = blocked.updated_at;
    let mut exhausted = blocked.clone();
    exhausted
        .pending_reasons
        .push(format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}3"));
    exhausted.updated_at += chrono::Duration::microseconds(1);
    let exhausted = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: exhausted,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed exhausted blocked streak")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("seeding exhausted blocked streak must apply, got {outcome:?}"),
    };

    let new_health = failing_check_pr_health("head-streak-new", "Lint");
    let new_fingerprint = health_fingerprint(684, &new_health);
    assert_ne!(old_fingerprint, new_fingerprint);
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(new_health.clone()));
    state.github_service =
        Some(github.clone() as Arc<dyn crate::domain::services::GithubServiceTrait>);

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("first pass resets the exhausted blocked streak"),
        0,
        "the re-arm pass itself must not be counted as a successor start"
    );
    let rearmed = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload re-armed blocked attempt")
        .expect("re-armed blocked attempt remains current");
    assert_eq!(rearmed.id, exhausted.id);
    assert_eq!(rearmed.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(
        !rearmed
            .pending_reasons
            .iter()
            .any(|reason| reason.starts_with(AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX)),
        "a successful re-arm must reset the exhausted streak markers: {:?}",
        rearmed.pending_reasons
    );
    assert!(rearmed.pending_reasons.iter().any(
        |reason| reason == &format!("{BLOCKED_STREAK_REARMED_REASON_PREFIX}{new_fingerprint}")
    ));

    // The re-arm CAS refreshes `updated_at`, which would otherwise throttle the very next pass
    // behind the automatic blocked-retry backoff. Age it past that backoff, exactly like a real
    // poll tick arriving later would.
    let expected_updated_at = rearmed.updated_at;
    let mut aged = rearmed.clone();
    aged.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1_000);
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt: aged,
                expected_phase: AgentWorkspaceRepairPhase::Blocked,
                expected_updated_at,
                next_phase: AgentWorkspaceRepairPhase::Blocked,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("age re-armed attempt past the automatic blocked-retry backoff"),
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));

    // The mock health result is single-use per fetch, and the re-arm pass already consumed the
    // one seeded above; seed the same evidence again for the pass that evaluates the successor.
    github.state().fetch_pr_health_result = Some(Ok(new_health));

    // Delivering the successor's dispatch is out of scope here (it needs a real spawnable agent
    // binary); the load-bearing proof is that the reset streak lets `start_or_join_agent_workspace_repair`
    // spawn a fresh generation instead of staying parked on the exhausted streak.
    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("second pass resumes the normal successor path for the new failure identity");
    let successor = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload successor attempt")
        .expect("successor attempt remains current");
    assert_ne!(
        successor.generation, rearmed.generation,
        "with the streak reset, a fresh successor generation must start on the next pass"
    );
}

#[tokio::test]
async fn blocked_pr_autofix_streak_does_not_rearm_when_new_fingerprint_budget_is_exhausted() {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(201).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;

    let old_health = failing_check_pr_health("head-budget-old", "Rust Tests");
    let old_fingerprint = health_fingerprint(684, &old_health);
    let new_health = failing_check_pr_health("head-budget-new", "Lint");
    let new_fingerprint = health_fingerprint(684, &new_health);
    assert_ne!(old_fingerprint, new_fingerprint);

    // A prior, now-settled generation already spent far more than the default budget on exactly
    // the failure identity GitHub currently reports.
    let spent_generation = block_pr_autofix_attempt_with_fingerprint(
        &state,
        &conversation_id,
        Some(new_fingerprint.clone()),
    )
    .await;
    seed_completed_run_bound_to_attempt(&state, &conversation_id, &spent_generation.id, 90).await;
    let spent_generation = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload spent generation")
        .expect("spent generation remains current before settlement");
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .settle_repair_attempt(SettleAgentWorkspaceRepairAttempt {
                attempt_id: spent_generation.id.clone(),
                generation: spent_generation.generation,
                expected_phase: AgentWorkspaceRepairPhase::Blocked,
                expected_updated_at: spent_generation.updated_at,
                outcome: AgentWorkspaceRepairOutcome::Failed,
                settled_at: chrono::Utc::now(),
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("settle spent generation"),
        SettleAgentWorkspaceRepairAttemptOutcome::Applied(_)
    ));

    start_blocked_pr_autofix_generation(&state, &conversation_id).await;
    let blocked = block_pr_autofix_attempt_with_fingerprint(
        &state,
        &conversation_id,
        Some(old_fingerprint.clone()),
    )
    .await;
    let expected_updated_at = blocked.updated_at;
    let mut exhausted = blocked.clone();
    exhausted
        .pending_reasons
        .push(format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}3"));
    exhausted.updated_at += chrono::Duration::microseconds(1);
    let exhausted = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: exhausted,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed second exhausted blocked streak")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("seeding second exhausted blocked streak must apply, got {outcome:?}"),
    };
    assert_ne!(exhausted.generation, spent_generation.generation);

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(new_health.clone()));
    let _ = new_health;
    state.github_service =
        Some(github.clone() as Arc<dyn crate::domain::services::GithubServiceTrait>);

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("an exhausted new-fingerprint budget must never reset the streak"),
        0
    );
    let still_exhausted = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload still-exhausted attempt")
        .expect("still-exhausted attempt remains current");
    assert_eq!(still_exhausted.id, exhausted.id);
    assert_eq!(still_exhausted.generation, exhausted.generation);
    assert!(
        still_exhausted
            .pending_reasons
            .iter()
            .any(|reason| reason == &format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}3")),
        "existing park_exhausted_pr_autofix_budget behavior must be preserved when the new \
         fingerprint's own budget is exhausted: {:?}",
        still_exhausted.pending_reasons
    );
    assert!(!still_exhausted
        .pending_reasons
        .iter()
        .any(|reason| reason.starts_with(BLOCKED_STREAK_REARMED_REASON_PREFIX)));
}

#[tokio::test]
async fn blocked_pr_autofix_streak_does_not_rearm_while_a_repair_head_is_unpublished() {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(203).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;

    let old_health = failing_check_pr_health("head-unpublished-old", "Rust Tests");
    let old_fingerprint = health_fingerprint(684, &old_health);
    let blocked = block_pr_autofix_attempt_with_fingerprint(
        &state,
        &conversation_id,
        Some(old_fingerprint.clone()),
    )
    .await;

    // A non-empty `repair_head_commit` with its redrive-check marker already recorded falls
    // through the existing redrive block (`durable_attempt_recovery.rs:923-956`) without a GitHub
    // read, and the deliberate `!has_unpublished_repair_head` guard must then skip the blocked-
    // streak re-arm entirely rather than resetting the streak for an unpublished head.
    let repair_head = "unpublished-repair-head".to_string();
    let expected_updated_at = blocked.updated_at;
    let mut exhausted = blocked.clone();
    exhausted.repair_head_commit = Some(repair_head.clone());
    exhausted.pending_reasons.extend([
        format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}3"),
        format!("{EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX}{repair_head}"),
    ]);
    exhausted.updated_at += chrono::Duration::microseconds(1);
    let exhausted = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: exhausted,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed exhausted blocked streak with an unpublished repair head")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("seeding exhausted blocked streak must apply, got {outcome:?}"),
    };

    let new_health = failing_check_pr_health("head-unpublished-new", "Lint");
    let new_fingerprint = health_fingerprint(684, &new_health);
    assert_ne!(old_fingerprint, new_fingerprint);
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(new_health));
    state.github_service =
        Some(github.clone() as Arc<dyn crate::domain::services::GithubServiceTrait>);

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recovery pass must not rearm while a repair head is unpublished"),
        0
    );

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload unpublished-head blocked attempt")
        .expect("unpublished-head blocked attempt remains current");
    assert_eq!(current.id, exhausted.id);
    assert_eq!(
        current.updated_at, exhausted.updated_at,
        "the attempt must stay untouched, not just re-converge to the same markers"
    );
    assert!(current
        .pending_reasons
        .iter()
        .any(|reason| reason == &format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}3")));
    assert!(!current
        .pending_reasons
        .iter()
        .any(|reason| reason.starts_with(BLOCKED_STREAK_REARMED_REASON_PREFIX)));
    assert_eq!(
        github.state().fetch_pr_health_calls,
        0,
        "an unpublished repair head must not add an unbounded GitHub read on a parked attempt"
    );
}

#[tokio::test]
async fn blocked_pr_autofix_streak_rearm_guard_prevents_a_second_reset_for_the_same_identity() {
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(202).await;
    start_blocked_pr_autofix_generation(&state, &conversation_id).await;

    let old_health = failing_check_pr_health("head-guard-old", "Rust Tests");
    let old_fingerprint = health_fingerprint(684, &old_health);
    let new_health = failing_check_pr_health("head-guard-new", "Lint");
    let new_fingerprint = health_fingerprint(684, &new_health);
    assert_ne!(old_fingerprint, new_fingerprint);

    let blocked = block_pr_autofix_attempt_with_fingerprint(
        &state,
        &conversation_id,
        Some(old_fingerprint.clone()),
    )
    .await;
    let expected_updated_at = blocked.updated_at;
    let mut already_rearmed_once = blocked.clone();
    already_rearmed_once
        .pending_reasons
        .push(format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}3"));
    already_rearmed_once.pending_reasons.push(format!(
        "{BLOCKED_STREAK_REARMED_REASON_PREFIX}{new_fingerprint}"
    ));
    already_rearmed_once.updated_at += chrono::Duration::microseconds(1);
    let seeded = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: already_rearmed_once,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed already-rearmed identity guard")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("seeding already-rearmed identity guard must apply, got {outcome:?}"),
    };

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(new_health));
    state.github_service =
        Some(github.clone() as Arc<dyn crate::domain::services::GithubServiceTrait>);

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("re-observing the same evidence must never reset the streak twice"),
        0
    );
    let unchanged = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload guarded attempt")
        .expect("guarded attempt remains current");
    assert_eq!(unchanged.id, seeded.id);
    assert_eq!(unchanged.updated_at, seeded.updated_at);
    assert_eq!(unchanged.pending_reasons, seeded.pending_reasons);
}

#[cfg(unix)]
async fn block_repair_with_orphaned_pr_handoff_effect(
    state: &AppState,
    conversation_id: &ChatConversationId,
    handoff_kind: AgentWorkspaceRepairEffectKind,
) -> (AgentWorkspaceRepairAttempt, String) {
    let repair_head = "1111111111111111111111111111111111111111".to_string();
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load repair attempt to block")
        .expect("repair attempt exists to block");
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.repair_head_commit = Some(repair_head.clone());
    attempt.blocker = Some("Pull-request continuation could not complete".to_string());
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1);
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
        .expect("block repair attempt at its PR handoff")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("blocking at the PR handoff must apply, got {outcome:?}"),
    };

    // The push already landed: an observed receipt whose remote OID is the repair head.
    let mut push = AgentWorkspaceRepairEffect::new(
        blocked.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        format!(
            "agent_workspace_repair:{}:{}:{}",
            blocked.id,
            blocked.generation,
            AgentWorkspaceRepairEffectKind::PushBranch
        ),
        blocked.updated_at,
    );
    push.status = AgentWorkspaceRepairEffectStatus::InFlight;
    push.intended_head_oid = Some(repair_head.clone());
    let push = match state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: blocked.id.clone(),
            generation: blocked.generation,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_attempt_updated_at: blocked.updated_at,
            effect: push,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed the in-flight push effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("push effect should be created, got {outcome:?}"),
    };
    let mut observed = push.clone();
    observed.status = AgentWorkspaceRepairEffectStatus::Observed;
    observed.receipt_json = Some(format!(
        "{{\"remote_ref\":\"refs/heads/ralphx/test/publish-recovery\",\"remote_oid\":\"{repair_head}\"}}"
    ));
    observed.updated_at = push.updated_at + chrono::Duration::milliseconds(1);
    observed.completed_at = Some(observed.updated_at);
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
                attempt_id: blocked.id.clone(),
                generation: blocked.generation,
                expected_phase: AgentWorkspaceRepairPhase::Blocked,
                expected_attempt_updated_at: blocked.updated_at,
                expected_effect_updated_at: push.updated_at,
                expected_effect_status: AgentWorkspaceRepairEffectStatus::InFlight,
                effect: observed,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("seed the observed push receipt"),
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(_)
    ));

    // The PR handoff that the dead continuation abandoned in flight.
    let mut handoff = AgentWorkspaceRepairEffect::new(
        blocked.id.clone(),
        handoff_kind,
        format!(
            "agent_workspace_repair:{}:{}:{}",
            blocked.id, blocked.generation, handoff_kind
        ),
        blocked.updated_at,
    );
    handoff.status = AgentWorkspaceRepairEffectStatus::InFlight;
    handoff.intended_head_oid = Some(repair_head.clone());
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: blocked.id.clone(),
                generation: blocked.generation,
                expected_phase: AgentWorkspaceRepairPhase::Blocked,
                expected_attempt_updated_at: blocked.updated_at,
                effect: handoff,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("seed the orphaned PR handoff effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));
    (blocked, repair_head)
}

#[cfg(unix)]
#[tokio::test]
async fn blocked_sweep_terminates_an_orphaned_pr_update_handoff_effect() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(151, "#!/bin/sh\nexit 1\n").await;
    let (blocked, _repair_head) = block_repair_with_orphaned_pr_handoff_effect(
        &state,
        &conversation_id,
        AgentWorkspaceRepairEffectKind::UpdatePr,
    )
    .await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("sweep the blocked repair attempt");

    assert!(
        state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&blocked.id)
            .await
            .expect("read the open effect after the sweep")
            .is_none(),
        "the sweep must clear the orphaned handoff fence in the same pass"
    );
    let terminated = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&format!(
            "agent_workspace_repair:{}:{}:{}",
            blocked.id,
            blocked.generation,
            AgentWorkspaceRepairEffectKind::UpdatePr
        ))
        .await
        .expect("read the terminated handoff effect")
        .expect("the terminated handoff effect is retained");
    assert_eq!(terminated.status, AgentWorkspaceRepairEffectStatus::Failed);
    assert!(terminated.completed_at.is_some());
    let current = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&blocked.id)
        .await
        .expect("reload the swept attempt")
        .expect("the swept attempt persists");
    assert!(
        current.settled_at.is_none(),
        "clearing the fence must not settle the attempt"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn blocked_sweep_keeps_an_orphaned_create_pr_handoff_effect_fenced() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(152, "#!/bin/sh\nexit 1\n").await;
    let (blocked, _repair_head) = block_repair_with_orphaned_pr_handoff_effect(
        &state,
        &conversation_id,
        AgentWorkspaceRepairEffectKind::CreatePr,
    )
    .await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("sweep the blocked repair attempt");

    let open = state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&blocked.id)
        .await
        .expect("read the open effect after the sweep")
        .expect("an unproven pull-request creation stays fenced");
    assert_eq!(open.kind, AgentWorkspaceRepairEffectKind::CreatePr);
    assert_eq!(open.status, AgentWorkspaceRepairEffectStatus::InFlight);
}

/// Proof obligation 6: one pass over a deleted worktree marks the workspace exactly once —
/// evidence written, status `Missing`, the unsettled repair attempt settled — and a second pass
/// changes nothing. Before this, each pass logged a warning and left the workspace retryable, so
/// the same dead workspace was re-examined forever.
#[tokio::test]
async fn a_deleted_worktree_is_marked_missing_once_and_settles_its_repair_attempt() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(61, "#!/bin/sh\nexit 0\n").await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let worktree_path = PathBuf::from(&workspace.worktree_path);
    std::fs::remove_dir_all(&worktree_path).expect("delete the workspace worktree");

    settle_missing_workspace_resolution(&state, &workspace, &worktree_path, true, "test_pass")
        .await
        .expect("settling a deleted worktree must succeed");

    let settled = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("reload workspace")
        .expect("workspace exists");
    assert_eq!(
        settled.status,
        AgentConversationWorkspaceStatus::Missing,
        "the workspace must be recoverable-Missing, never terminalized"
    );

    let evidence: Vec<_> = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load publication events")
        .into_iter()
        .filter(|event| event.step == WORKSPACE_MISSING_SETTLED_STEP)
        .collect();
    assert_eq!(evidence.len(), 1, "exactly one evidence row");
    assert!(evidence[0]
        .summary
        .contains(&worktree_path.display().to_string()));

    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load repair attempt")
        .expect("an attempt exists");
    assert_eq!(
        attempt.phase,
        AgentWorkspaceRepairPhase::Blocked,
        "the unsettled attempt must be blocked for a human, not left auto-retryable"
    );

    // Idempotency: the reloaded (already-Missing) workspace is a no-op.
    settle_missing_workspace_resolution(&state, &settled, &worktree_path, true, "test_pass")
        .await
        .expect("a second pass must be a no-op");
    let repeated = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("reload publication events")
        .into_iter()
        .filter(|event| event.step == WORKSPACE_MISSING_SETTLED_STEP)
        .count();
    assert_eq!(repeated, 1, "no duplicate evidence on a second pass");
}

/// A missing worktree *root* is disk or mount trouble, not a deleted workspace. It must warn and
/// change nothing — otherwise one unmounted volume would settle every workspace on it.
#[tokio::test]
async fn a_missing_worktree_root_settles_nothing() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(62, "#!/bin/sh\nexit 0\n").await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let worktree_path = PathBuf::from(&workspace.worktree_path);

    settle_missing_workspace_resolution(&state, &workspace, &worktree_path, false, "test_pass")
        .await
        .expect("a missing root must not fail the pass");

    let unchanged = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("reload workspace")
        .expect("workspace exists");
    assert_eq!(unchanged.status, AgentConversationWorkspaceStatus::Active);
    assert!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("load publication events")
            .iter()
            .all(|event| event.step != WORKSPACE_MISSING_SETTLED_STEP),
        "no evidence may be written when the whole worktree root is absent"
    );
    assert!(
        state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load repair attempt")
            .is_some_and(|attempt| attempt.is_unsettled()),
        "the repair attempt must stay unsettled so it retries once the volume returns"
    );
}

/// Adds a second conversation, real worktree, workspace, and unsettled repair attempt to a state
/// already seeded by [`seed_orphaned_repair_dispatch`], so one pass can span two attempts.
#[cfg(unix)]
async fn seed_second_repair_attempt_in_same_project(
    state: &AppState,
    project_dir: &std::path::Path,
    suffix: u8,
    branch_name: &str,
) -> ChatConversationId {
    let second_id = conversation_id(suffix);
    let project = state
        .project_repo
        .get_by_id(&project_id())
        .await
        .expect("load seeded project")
        .expect("seeded project exists");
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &second_id)
        .expect("derive second workspace path");
    recovery_git(
        project_dir,
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    let mut conversation = ChatConversation::new_project(project_id());
    conversation.id = second_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed second conversation");
    let mut workspace = needs_agent_workspace(second_id.clone());
    workspace.branch_name = branch_name.to_string();
    workspace.worktree_path = workspace_path.display().to_string();
    workspace.base_commit = Some(recovery_git(project_dir, &["rev-parse", "HEAD"]));
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed second workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                second_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "second attempt in the same pass".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start second repair attempt");
    assert!(matches!(
        started,
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(_)
    ));
    second_id
}

/// Proof obligation 7: a pass containing one orphaned attempt (worktree missing, root present)
/// followed by a healthy attempt reconciles both. The orphan is marked `Missing` and settled, the
/// healthy attempt is still processed, and the pass returns success.
///
/// Before this, `redeliver_due_repair_dispatch` propagated the missing-worktree error through the
/// loop's `?`, so a single orphan aborted the whole pass — the production symptom was the startup
/// `durable claims remain fenced` ERROR, with 17 of 24 unsettled attempts never reconciled and the
/// in-flight git-mutation stage never running at all.
#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn one_orphaned_worktree_no_longer_aborts_the_whole_recovery_pass() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, orphan_id, _worktree_parent, project_dir) =
        seed_orphaned_repair_dispatch(63, "#!/bin/sh\nexit 1\n").await;
    let healthy_id = seed_second_repair_attempt_in_same_project(
        &state,
        project_dir.path(),
        64,
        "ralphx/test/publish-recovery-healthy",
    )
    .await;
    age_requested_repair_attempt(&state, &orphan_id).await;
    age_requested_repair_attempt(&state, &healthy_id).await;

    let orphan_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&orphan_id)
        .await
        .expect("load orphan workspace")
        .expect("orphan workspace exists");
    std::fs::remove_dir_all(PathBuf::from(&orphan_workspace.worktree_path))
        .expect("delete the orphan worktree");

    let recovered = recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("one orphaned worktree must not abort the pass");

    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&orphan_id)
            .await
            .expect("reload orphan workspace")
            .expect("orphan workspace exists")
            .status,
        AgentConversationWorkspaceStatus::Missing,
        "the orphan must be settled as recoverable-Missing"
    );
    assert_eq!(
        state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&orphan_id)
            .await
            .expect("load orphan attempt")
            .expect("orphan attempt exists")
            .phase,
        AgentWorkspaceRepairPhase::Blocked,
        "the orphan's attempt must stop being re-dispatched"
    );

    let healthy = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&healthy_id)
        .await
        .expect("load healthy attempt")
        .expect("healthy attempt remains current");
    assert_eq!(healthy.phase, AgentWorkspaceRepairPhase::Requested);
    assert_eq!(
        healthy.dispatch_count, 1,
        "the healthy attempt must still be processed in the same pass"
    );
    assert!(healthy.next_dispatch_at.is_some());
    assert_eq!(
        recovered, 1,
        "only the healthy attempt counts as recovered; the orphan is a settled no-op"
    );
}

/// Reproduces the production sequence that strands an open effect on a blocked repair: the effect
/// is created in `Continuing` (the only phase production creates repair effects in), and then the
/// claim-recovery blocker fires while it is still in flight. That is exactly what
/// `git_mutation_recovery::block_repair_claim_recovery` does when a push mutation loses its lease,
/// target, or fencing-epoch proof — after which claim recovery declines forever on
/// `phase == Continuing`.
#[cfg(unix)]
async fn block_repair_with_orphaned_open_effect(
    state: &AppState,
    conversation_id: &ChatConversationId,
    kind: AgentWorkspaceRepairEffectKind,
    intended_head_oid: Option<&str>,
) -> (AgentWorkspaceRepairAttempt, String) {
    let repair_head = "2222222222222222222222222222222222222222".to_string();
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load repair attempt to continue")
        .expect("repair attempt exists to continue");
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    attempt.repair_head_commit = Some(repair_head.clone());
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    let continuing = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("move the repair into its continuation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("continuing the repair must apply, got {outcome:?}"),
    };

    let mut effect = AgentWorkspaceRepairEffect::new(
        continuing.id.clone(),
        kind,
        format!(
            "agent_workspace_repair:{}:{}:{}",
            continuing.id, continuing.generation, kind
        ),
        continuing.updated_at,
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = intended_head_oid.map(str::to_string);
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: continuing.id.clone(),
                generation: continuing.generation,
                expected_phase: AgentWorkspaceRepairPhase::Continuing,
                expected_attempt_updated_at: continuing.updated_at,
                effect,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("seed the in-flight effect the owning process abandoned"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    let blocked = match block_agent_workspace_repair_completion(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        continuing,
        "Workspace repair recovery is blocked.",
        "repair mutation lease proof failed",
        None,
        None,
        None,
    )
    .await
    .expect("block the continuation while its effect is still in flight")
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("blocking the continuation must apply, got {outcome:?}"),
    };
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    (blocked, repair_head)
}

#[cfg(unix)]
async fn open_effect_after_sweep(
    state: &AppState,
    attempt_id: &crate::domain::entities::AgentWorkspaceRepairAttemptId,
) -> Option<AgentWorkspaceRepairEffect> {
    state
        .agent_workspace_repair_repo
        .get_open_repair_effect(attempt_id)
        .await
        .expect("read the open effect after the sweep")
}

#[cfg(unix)]
#[tokio::test]
async fn blocked_sweep_terminates_an_orphaned_branch_push_effect() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(153, "#!/bin/sh\nexit 1\n").await;
    let (blocked, repair_head) = block_repair_with_orphaned_open_effect(
        &state,
        &conversation_id,
        AgentWorkspaceRepairEffectKind::PushBranch,
        Some("2222222222222222222222222222222222222222"),
    )
    .await;
    assert!(open_effect_after_sweep(&state, &blocked.id).await.is_some());

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("sweep the blocked repair attempt");

    assert!(
        open_effect_after_sweep(&state, &blocked.id).await.is_none(),
        "the abandoned push must stop fencing the attempt"
    );
    let terminated = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&format!(
            "agent_workspace_repair:{}:{}:{}",
            blocked.id,
            blocked.generation,
            AgentWorkspaceRepairEffectKind::PushBranch
        ))
        .await
        .expect("read the terminated push effect")
        .expect("the terminated push effect is retained");
    assert_eq!(
        terminated.status,
        AgentWorkspaceRepairEffectStatus::Failed,
        "the fence is cleared by failing the effect, never by deleting its history"
    );
    assert!(terminated.completed_at.is_some());
    assert_eq!(
        terminated.intended_head_oid.as_deref(),
        Some(repair_head.as_str())
    );

    // A second pass has nothing left to clear.
    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("re-sweep the blocked repair attempt");
    assert!(open_effect_after_sweep(&state, &blocked.id).await.is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn blocked_sweep_leaves_a_pull_request_creation_effect_fenced() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(154, "#!/bin/sh\nexit 1\n").await;
    let (blocked, _repair_head) = block_repair_with_orphaned_open_effect(
        &state,
        &conversation_id,
        AgentWorkspaceRepairEffectKind::CreatePr,
        Some("2222222222222222222222222222222222222222"),
    )
    .await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("sweep the blocked repair attempt");

    let open = open_effect_after_sweep(&state, &blocked.id)
        .await
        .expect("an unproven pull-request creation stays fenced");
    assert_eq!(open.kind, AgentWorkspaceRepairEffectKind::CreatePr);
    assert_eq!(open.status, AgentWorkspaceRepairEffectStatus::InFlight);
}

#[cfg(unix)]
#[tokio::test]
async fn blocked_sweep_leaves_a_push_effect_whose_head_disagrees_with_the_attempt() {
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(155, "#!/bin/sh\nexit 1\n").await;
    let (blocked, _repair_head) = block_repair_with_orphaned_open_effect(
        &state,
        &conversation_id,
        AgentWorkspaceRepairEffectKind::PushBranch,
        Some("3333333333333333333333333333333333333333"),
    )
    .await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("sweep the blocked repair attempt");

    let open = open_effect_after_sweep(&state, &blocked.id)
        .await
        .expect("a push for a head the attempt cannot vouch for stays fenced");
    assert_eq!(open.status, AgentWorkspaceRepairEffectStatus::InFlight);
}

const PR_FIXER_RESCUE_CLI: &str = r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"pr fix started"}]},"session_id":"pr-fixer-rescue-session"}'
printf '%s\n' '{"type":"result","session_id":"pr-fixer-rescue-session","is_error":false,"result":"pr fix started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#;

/// Converts the seeded orphan into an aged PR autofix generation so the recovery sweep rescues it
/// through the delivery lane.
#[cfg(unix)]
async fn age_requested_pr_autofix_orphan(
    state: &AppState,
    conversation_id: &ChatConversationId,
    dispatch_head_commit: Option<&str>,
) {
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load seeded attempt")
        .expect("seeded attempt exists");
    let expected_updated_at = attempt.updated_at;
    attempt.source = AgentWorkspaceRepairSource::PrAutofix;
    attempt.pr_autofix_health_fingerprint = Some("github_pr_autofix:684:checks:rust".to_string());
    attempt.pr_autofix_dispatch_head_commit = dispatch_head_commit.map(str::to_string);
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
}

#[cfg(unix)]
async fn seeded_workspace_worktree_path(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> PathBuf {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .expect("load seeded workspace")
        .expect("seeded workspace exists");
    PathBuf::from(workspace.worktree_path)
}

#[cfg(unix)]
async fn rescued_dispatch_head(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Option<String> {
    let recovered = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load rescued attempt")
        .expect("rescued attempt remains current");
    assert_eq!(recovered.phase, AgentWorkspaceRepairPhase::Repairing);
    assert!(recovered.reserved_agent_run_id.is_some());
    recovered.pr_autofix_dispatch_head_commit
}

/// Gives the seeded fixture a bare `origin` and pushes both refs, so freshness inspection (which
/// always fetches) has a real remote to read.
#[cfg(unix)]
fn attach_bare_origin(
    project_dir: &std::path::Path,
    workspace_path: &std::path::Path,
    origin: &std::path::Path,
) {
    recovery_git(origin, &["init", "--bare", "-b", "main"]);
    recovery_git(
        project_dir,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("origin path"),
        ],
    );
    recovery_git(project_dir, &["push", "origin", "main"]);
    recovery_git(
        workspace_path,
        &["push", "origin", "ralphx/test/publish-recovery"],
    );
}

/// Drives the seeded orphan through a real rescue delivery so it holds a canonical target lease,
/// then parks it in `Repairing` with no live run. That is exactly the interrupted shape
/// `recover_clean_interrupted_repair` owns, and the lease is a hard precondition of that path.
#[cfg(unix)]
async fn interrupt_repair_at_target_base(
    state: &AppState,
    conversation_id: &ChatConversationId,
    target_base_commit: &str,
) -> AgentWorkspaceRepairAttempt {
    age_requested_pr_autofix_orphan(state, conversation_id, None).await;
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(state)
            .await
            .expect("deliver the seeded repair so it acquires its target lease"),
        1
    );
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load delivered attempt")
        .expect("delivered attempt exists");
    assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Repairing);
    assert!(
        attempt.target_lease_epoch.is_some(),
        "the delivery must leave a canonical target lease behind"
    );
    let expected_updated_at = attempt.updated_at;
    attempt.target_base_commit = Some(target_base_commit.to_string());
    // No live run means the owning process is gone: the interrupted shape, not an active repair.
    attempt.reserved_agent_run_id = None;
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(61);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Repairing,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Repairing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("interrupt the delivered repair")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("interrupting the delivered repair must apply, got {outcome:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn interrupted_repair_behind_an_advanced_base_retargets_instead_of_blocking() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, project_dir) =
        seed_orphaned_repair_dispatch(124, PR_FIXER_RESCUE_CLI).await;
    let workspace_path = seeded_workspace_worktree_path(&state, &conversation_id).await;
    let origin = tempfile::tempdir().expect("create bare origin for retarget");
    attach_bare_origin(project_dir.path(), &workspace_path, origin.path());
    let old_base = recovery_git(project_dir.path(), &["rev-parse", "HEAD"]);
    // main moves on while the repair is interrupted, exactly the PR #1023 shape.
    std::fs::write(project_dir.path().join("advanced.md"), "advanced\n")
        .expect("write advanced base file");
    recovery_git(project_dir.path(), &["add", "advanced.md"]);
    recovery_git(project_dir.path(), &["commit", "-m", "advance main"]);
    recovery_git(project_dir.path(), &["push", "origin", "main"]);
    let new_base = recovery_git(project_dir.path(), &["rev-parse", "HEAD"]);
    assert_ne!(old_base, new_base);
    let interrupted = interrupt_repair_at_target_base(&state, &conversation_id, &old_base).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recover the interrupted repair"),
        1
    );

    let successor = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load the retargeted successor")
        .expect("a successor generation exists");
    assert_ne!(
        successor.id, interrupted.id,
        "retargeting must supersede the interrupted generation, not mutate it"
    );
    assert_eq!(
        successor.target_base_commit.as_deref(),
        Some(new_base.as_str()),
        "the successor must target the tip the classifier actually read"
    );
    assert_ne!(
        successor.phase,
        AgentWorkspaceRepairPhase::Blocked,
        "a settled tree behind a newer base must never produce a blocked banner"
    );
    let settled = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&interrupted.id)
        .await
        .expect("load the superseded generation")
        .expect("the superseded generation is still readable");
    assert_eq!(
        settled.outcome,
        Some(AgentWorkspaceRepairOutcome::Superseded)
    );
}

/// Which of the three post-settlement reads the retarget path performs should fail.
#[cfg(unix)]
enum PostSettlementFailure {
    /// The worktree is gone, so `resolve_effective_agent_conversation_workspace_path` fails.
    MissingWorktree,
    /// The worktree and its `.git` entry survive path resolution, so the failure lands on
    /// `GitService::canonical_target_identity` instead.
    UnreadableRepository,
    /// The project row is gone, so the retarget's own project lookup fails.
    MissingProjectRow,
}

/// The classifier and the retarget read the same facts, so a single-threaded test cannot make the
/// second read fail on its own. Both reads pass through `project_repo.get_by_id`, and on the
/// interrupted-repair path the classifier's is read 1 while the retarget's is read 2 — firing on
/// read 2 reproduces the real race (a worktree or project row disappearing mid-recovery) through
/// the production route instead of a hand-built durable row.
#[cfg(unix)]
const POST_SETTLEMENT_PROJECT_READ: usize = 2;

#[cfg(unix)]
struct SabotagedProjectRepository {
    inner: Arc<dyn ProjectRepository>,
    workspace_path: PathBuf,
    failure: PostSettlementFailure,
    reads: AtomicUsize,
}

#[cfg(unix)]
#[async_trait]
impl ProjectRepository for SabotagedProjectRepository {
    async fn create(&self, project: Project) -> crate::error::AppResult<Project> {
        self.inner.create(project).await
    }

    async fn get_by_id(&self, id: &ProjectId) -> crate::error::AppResult<Option<Project>> {
        if self.reads.fetch_add(1, Ordering::SeqCst) + 1 == POST_SETTLEMENT_PROJECT_READ {
            match self.failure {
                PostSettlementFailure::MissingWorktree => {
                    std::fs::remove_dir_all(&self.workspace_path)
                        .expect("remove the workspace worktree mid-recovery");
                }
                PostSettlementFailure::UnreadableRepository => {
                    std::fs::write(self.workspace_path.join(".git"), "gitdir: /nonexistent\n")
                        .expect("break the workspace git link mid-recovery");
                }
                PostSettlementFailure::MissingProjectRow => return Ok(None),
            }
        }
        self.inner.get_by_id(id).await
    }

    async fn get_all(&self) -> crate::error::AppResult<Vec<Project>> {
        self.inner.get_all().await
    }

    async fn update(&self, project: &Project) -> crate::error::AppResult<()> {
        self.inner.update(project).await
    }

    async fn delete(&self, id: &ProjectId) -> crate::error::AppResult<()> {
        self.inner.delete(id).await
    }

    async fn get_by_working_directory(
        &self,
        path: &str,
    ) -> crate::error::AppResult<Option<Project>> {
        self.inner.get_by_working_directory(path).await
    }

    async fn archive(&self, id: &ProjectId) -> crate::error::AppResult<Project> {
        self.inner.archive(id).await
    }
}

/// Drives the same retarget fixture as
/// `interrupted_repair_behind_an_advanced_base_retargets_instead_of_blocking`, then fails exactly
/// the resolution the retarget performs after its successor is already durable. The sweep iterates
/// every recoverable attempt with `?`, so propagating here would stop recovery for every other
/// workspace in the pass.
#[cfg(unix)]
#[allow(clippy::await_holding_lock)]
async fn assert_retarget_degrades_when_post_settlement_resolution_fails(
    suffix: u8,
    failure: PostSettlementFailure,
) {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (mut state, conversation_id, _worktree_parent, project_dir) =
        seed_orphaned_repair_dispatch(suffix, PR_FIXER_RESCUE_CLI).await;
    let workspace_path = seeded_workspace_worktree_path(&state, &conversation_id).await;
    let origin = tempfile::tempdir().expect("create bare origin for the degraded retarget");
    attach_bare_origin(project_dir.path(), &workspace_path, origin.path());
    let old_base = recovery_git(project_dir.path(), &["rev-parse", "HEAD"]);
    std::fs::write(project_dir.path().join("advanced.md"), "advanced\n")
        .expect("write advanced base file");
    recovery_git(project_dir.path(), &["add", "advanced.md"]);
    recovery_git(project_dir.path(), &["commit", "-m", "advance main"]);
    recovery_git(project_dir.path(), &["push", "origin", "main"]);
    let new_base = recovery_git(project_dir.path(), &["rev-parse", "HEAD"]);
    let interrupted = interrupt_repair_at_target_base(&state, &conversation_id, &old_base).await;
    let inner = Arc::clone(&state.project_repo);
    state.project_repo = Arc::new(SabotagedProjectRepository {
        inner,
        workspace_path: workspace_path.clone(),
        failure,
        reads: AtomicUsize::new(0),
    });

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("a failure after the successor is durable must not fail the recovery pass"),
        1
    );

    let successor = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load the retargeted successor")
        .expect("a successor generation exists");
    assert_ne!(
        successor.id, interrupted.id,
        "the successor must still exist after the degraded return"
    );
    assert_eq!(
        successor.target_base_commit.as_deref(),
        Some(new_base.as_str()),
        "the successor must still target the tip the classifier read"
    );
    assert_ne!(
        successor.phase,
        AgentWorkspaceRepairPhase::Blocked,
        "an undelivered successor belongs to the rescue lane, not a blocked banner"
    );
    let settled = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&interrupted.id)
        .await
        .expect("load the superseded generation")
        .expect("the superseded generation is still readable");
    assert_eq!(
        settled.outcome,
        Some(AgentWorkspaceRepairOutcome::Superseded),
        "the settlement that already happened must not be rolled back"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn retarget_degrades_when_the_workspace_worktree_disappears_after_settlement() {
    assert_retarget_degrades_when_post_settlement_resolution_fails(
        126,
        PostSettlementFailure::MissingWorktree,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn retarget_degrades_when_the_workspace_repository_is_unreadable_after_settlement() {
    assert_retarget_degrades_when_post_settlement_resolution_fails(
        127,
        PostSettlementFailure::UnreadableRepository,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn retarget_degrades_when_the_project_row_disappears_after_settlement() {
    assert_retarget_degrades_when_post_settlement_resolution_fails(
        128,
        PostSettlementFailure::MissingProjectRow,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn interrupted_repair_with_a_dirty_tree_blocks_without_leaking_the_error_variant() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, project_dir) =
        seed_orphaned_repair_dispatch(125, PR_FIXER_RESCUE_CLI).await;
    let workspace_path = seeded_workspace_worktree_path(&state, &conversation_id).await;
    let origin = tempfile::tempdir().expect("create bare origin for dirty-tree block");
    attach_bare_origin(project_dir.path(), &workspace_path, origin.path());
    // The base stays exactly where the attempt targeted it, so only the dirty tree can fail.
    let base = recovery_git(project_dir.path(), &["rev-parse", "HEAD"]);
    interrupt_repair_at_target_base(&state, &conversation_id, &base).await;
    std::fs::write(workspace_path.join("unstaged.md"), "half-finished\n")
        .expect("write uncommitted repair file");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recover the dirty interrupted repair"),
        1
    );

    let blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load the blocked attempt")
        .expect("the blocked attempt remains current");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    let blocker = blocked.blocker.expect("a blocked repair records why");
    assert!(
        blocker.contains("uncommitted"),
        "the banner must name the actual condition, got: {blocker}"
    );
    assert!(
        !blocker.contains("Conflict:"),
        "the banner must not leak an AppError variant name, got: {blocker}"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rescued_pr_autofix_dispatch_backfills_the_dispatch_head_from_the_remote_branch() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, project_dir) =
        seed_orphaned_repair_dispatch(121, PR_FIXER_RESCUE_CLI).await;
    let workspace_path = seeded_workspace_worktree_path(&state, &conversation_id).await;
    let origin = tempfile::tempdir().expect("create bare origin for dispatch-head backfill");
    recovery_git(origin.path(), &["init", "--bare", "-b", "main"]);
    recovery_git(
        project_dir.path(),
        &[
            "remote",
            "add",
            "origin",
            origin.path().to_str().expect("origin path"),
        ],
    );
    recovery_git(
        &workspace_path,
        &["push", "origin", "ralphx/test/publish-recovery"],
    );
    let remote_head = recovery_git(&workspace_path, &["rev-parse", "HEAD"]);
    // The stranded-unpushed shape: the fixer's local commit is ahead of what the PR can see.
    std::fs::write(workspace_path.join("local-fix.txt"), "fix\n").expect("write local repair file");
    recovery_git(&workspace_path, &["add", "local-fix.txt"]);
    recovery_git(&workspace_path, &["commit", "-m", "local repair commit"]);
    let local_head = recovery_git(&workspace_path, &["rev-parse", "HEAD"]);
    assert_ne!(
        remote_head, local_head,
        "the fixture must leave the local head ahead of the remote head"
    );
    age_requested_pr_autofix_orphan(&state, &conversation_id, None).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("rescue orphaned PR autofix dispatch"),
        1
    );

    assert_eq!(
        rescued_dispatch_head(&state, &conversation_id).await,
        Some(remote_head),
        "a rescued PR autofix dispatch must record the remote head the PR is running against"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rescued_pr_autofix_dispatch_falls_back_to_the_local_branch_head_without_a_remote() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(122, PR_FIXER_RESCUE_CLI).await;
    // No `origin` remote exists, so both the fetch and the remote-ref read must degrade quietly.
    let workspace_path = seeded_workspace_worktree_path(&state, &conversation_id).await;
    let local_head = recovery_git(&workspace_path, &["rev-parse", "HEAD"]);
    age_requested_pr_autofix_orphan(&state, &conversation_id, None).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("rescue orphaned PR autofix dispatch without a remote"),
        1
    );

    assert_eq!(
        rescued_dispatch_head(&state, &conversation_id).await,
        Some(local_head),
        "an unreachable remote must fall back to the local branch head, never to NULL"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rescued_pr_autofix_dispatch_preserves_an_existing_dispatch_head() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(123, PR_FIXER_RESCUE_CLI).await;
    age_requested_pr_autofix_orphan(&state, &conversation_id, Some("poller-dispatch-head")).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("rescue orphaned PR autofix dispatch with recorded evidence"),
        1
    );

    assert_eq!(
        rescued_dispatch_head(&state, &conversation_id).await,
        Some("poller-dispatch-head".to_string()),
        "backfill must never overwrite dispatch evidence the poller already proved"
    );
}

/// Drives the seeded orphan through a real delivery (which acquires the canonical target lease),
/// then parks it in `Continuing` holding an abandoned in-flight effect. With no GitHub service the
/// continuation blocks the attempt and returns an error, which is the production route into the
/// blocked-with-open-effect escalation gap.
#[cfg(unix)]
async fn continue_repair_into_a_blocking_publish_with_open_effect(
    state: &AppState,
    conversation_id: &ChatConversationId,
    kind: AgentWorkspaceRepairEffectKind,
) -> AgentWorkspaceRepairAttempt {
    let repair_head = "4444444444444444444444444444444444444444".to_string();
    age_requested_pr_autofix_orphan(state, conversation_id, None).await;
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(state)
            .await
            .expect("deliver the seeded repair so it acquires its target lease"),
        1
    );
    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load delivered attempt")
        .expect("delivered attempt exists");
    assert!(attempt.target_lease_epoch.is_some());
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    attempt.continuation = AgentWorkspaceRepairContinuation::Publish;
    attempt.repair_head_commit = Some(repair_head.clone());
    attempt.reserved_agent_run_id = None;
    attempt.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    let continuing = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("move the delivered repair into its continuation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("continuing the delivered repair must apply, got {outcome:?}"),
    };

    let mut effect = AgentWorkspaceRepairEffect::new(
        continuing.id.clone(),
        kind,
        format!(
            "agent_workspace_repair:{}:{}:{}",
            continuing.id, continuing.generation, kind
        ),
        continuing.updated_at,
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = Some(repair_head);
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: continuing.id.clone(),
                generation: continuing.generation,
                expected_phase: AgentWorkspaceRepairPhase::Continuing,
                expected_attempt_updated_at: continuing.updated_at,
                effect,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("seed the in-flight effect the owning process abandoned"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));
    continuing
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn blocked_repair_with_an_open_push_effect_regains_a_live_user_retry() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(156, PR_FIXER_RESCUE_CLI).await;
    // No GitHub service, so the continuation blocks the attempt and returns an error.
    assert!(state.github_service.is_none());
    let continuing = continue_repair_into_a_blocking_publish_with_open_effect(
        &state,
        &conversation_id,
        AgentWorkspaceRepairEffectKind::UpdatePr,
    )
    .await;
    assert!(
        !explicit_agent_workspace_repair_retry_allowed(
            state.agent_workspace_repair_repo.as_ref(),
            &continuing
        )
        .await
        .expect("read the retry admission before recovery"),
        "a continuing attempt has no retry action to begin with"
    );

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("let the continuation block behind its open effect");

    let blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load the blocked attempt")
        .expect("the blocked attempt remains current");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(
        blocked
            .pending_reasons
            .iter()
            .any(|reason| reason == CONTINUATION_OPEN_EFFECT_ATTENTION_REASON),
        "a blocked attempt fenced by its own effect must record why it needs attention, got: {:?}",
        blocked.pending_reasons
    );
    assert!(
        explicit_agent_workspace_repair_retry_allowed(
            state.agent_workspace_repair_repo.as_ref(),
            &blocked
        )
        .await
        .expect("read the retry admission after recovery"),
        "an idempotent pull-request update replay must leave the user a live Retry action"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn blocked_repair_with_an_open_pull_request_creation_keeps_its_retry_withheld() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (state, conversation_id, _worktree_parent, _project_dir) =
        seed_orphaned_repair_dispatch(157, PR_FIXER_RESCUE_CLI).await;
    continue_repair_into_a_blocking_publish_with_open_effect(
        &state,
        &conversation_id,
        AgentWorkspaceRepairEffectKind::CreatePr,
    )
    .await;

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("let the continuation block behind its open effect");

    let blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load the blocked attempt")
        .expect("the blocked attempt remains current");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(
        !explicit_agent_workspace_repair_retry_allowed(
            state.agent_workspace_repair_repo.as_ref(),
            &blocked
        )
        .await
        .expect("read the retry admission after recovery"),
        "an unproven pull-request creation must never be re-admitted for replay"
    );
}

#[tokio::test]
async fn base_advanced_successor_targets_the_observed_base() {
    // Validates the ProceedRetargeted path: when the base moves, the successor must carry the
    // observed OID as its target_base_commit so the next evaluation's repair_base_advanced check
    // returns false and does not re-authorize another generation.
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(206).await;

    start_blocked_pr_autofix_generation(&state, &conversation_id).await;

    // Build health with a failing check and a base that differs from the attempt's target.
    let mut health = failing_check_pr_health("head-sha", "Rust Tests");
    let fingerprint = health_fingerprint(684, &health);
    health.sync_state.base_ref_oid = Some("observed-base-b".to_string());
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    state.github_service = Some(github as Arc<dyn crate::domain::services::GithubServiceTrait>);

    // Block the attempt with a fingerprint set and a stale target, aged past the backoff.
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load attempt")
        .expect("attempt exists");
    let expected_updated_at = attempt.updated_at;
    let expected_phase = attempt.phase;
    let mut blocked = attempt;
    blocked.source = AgentWorkspaceRepairSource::PrAutofix;
    blocked.phase = AgentWorkspaceRepairPhase::Blocked;
    blocked.pr_autofix_health_fingerprint = Some(fingerprint);
    blocked.target_base_commit = Some("original-base-a".to_string());
    blocked.blocker = Some("transient_ci".to_string());
    blocked.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1_000);
    let blocked = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: blocked,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("block attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(a) => a,
        outcome => panic!("must apply, got {outcome:?}"),
    };

    let workspace_before = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let original_workspace_base = workspace_before.base_commit.clone();

    recover_agent_workspace_repair_attempts_for_state(&state)
        .await
        .expect("recovery pass runs");

    let successor = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load successor")
        .expect("successor exists");
    assert_ne!(
        successor.id, blocked.id,
        "a new generation must have been started"
    );
    assert_eq!(
        successor.target_base_commit.as_deref(),
        Some("observed-base-b"),
        "successor must target the observed base, not the predecessor's stale target"
    );

    let workspace_after = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace after")
        .expect("workspace exists");
    assert_eq!(
        workspace_after.base_commit, original_workspace_base,
        "workspace.base_commit must not be mutated by the recovery pass"
    );
}

#[tokio::test]
async fn base_advance_authorizes_exactly_one_successor() {
    // Once the successor is retargeted to the observed base, evaluating its state with identical
    // health must return HoldUnchanged — proving exactly one successor is authorized per advance.
    let (mut state, conversation_id, _worktree_parent, _project_dir) =
        seed_pr_autofix_health_workspace(207).await;

    let mut health = failing_check_pr_health("head-sha", "Rust Tests");
    let fingerprint = health_fingerprint(684, &health);
    // GitHub reports the same base the successor was retargeted onto — no further movement.
    health.sync_state.base_ref_oid = Some("observed-base-b".to_string());
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(health));
    state.github_service = Some(github as Arc<dyn crate::domain::services::GithubServiceTrait>);

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");

    // Simulate the successor that was retargeted: its target now matches what GitHub reports,
    // so repair_base_advanced returns false and the fingerprint comparison governs.
    let mut successor = blocked_pr_autofix_attempt(&conversation_id, &fingerprint);
    successor.target_base_commit = Some("observed-base-b".to_string());

    assert_eq!(
        evaluate_pr_autofix_successor(&state, &successor, &workspace).await,
        PrAutofixSuccessorDecision::HoldUnchanged,
        "identical health with a matching target must park, not authorize another successor"
    );
}

// ---------------------------------------------------------------------------
// Inert push-effect classification (steps 1-3)
//
// An effect whose intended/expected OIDs were never written is proof that no push was authorized
// for it, because initialization persists those preconditions strictly before `git push` runs.
// Terminating it releases the one-open-effect fence that otherwise wedges the continuation
// permanently: claim recovery deliberately releases the mutation claim without settling the
// effect, and the continuation gate refuses to reacquire the target lease while an effect is open.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quiescent_inert_push_effect_is_classified_never_authorized_and_clears_the_fence() {
    let fixture =
        seed_open_push_effect_reconciliation_fixture(105, ReconciliationRemoteShape::Uninitialized)
            .await;
    let (state, attempt, effect) = (fixture.state, fixture.attempt, fixture.effect);

    assert_eq!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect.clone())
            .await
            .expect("classify inert push effect"),
        crate::application::publish_resilience::AgentWorkspaceRepairOpenPushEffectReconciliation::NotApplied(
            crate::application::publish_resilience::AgentWorkspaceRepairPushEffectNotAppliedReason::NeverAuthorized
        ),
        "an uninitialized effect proves no push was authorized, not that the remote is unchanged"
    );

    let settled = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&effect.idempotency_key)
        .await
        .expect("reload settled inert effect")
        .expect("settled inert effect exists");
    assert_eq!(settled.status, AgentWorkspaceRepairEffectStatus::Failed);
    assert!(
        settled.completed_at.is_some(),
        "completed_at is what actually releases the one-open-effect fence"
    );
    assert!(settled
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("never initialized")));
    assert!(
        state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&attempt.id)
            .await
            .expect("query open effect after settlement")
            .is_none(),
        "the fence must actually be clear, or the continuation still cannot reacquire"
    );
}

#[tokio::test]
async fn inert_push_effect_younger_than_the_quiescence_window_stays_pending() {
    // A preflight reserved seconds ago may simply be mid-flight right now. Only age makes the
    // "never initialized" reading safe.
    let fixture = seed_open_push_effect_reconciliation_fixture(
        106,
        ReconciliationRemoteShape::UninitializedTooYoung,
    )
    .await;
    let (state, attempt, effect) = (fixture.state, fixture.attempt, fixture.effect);

    assert_eq!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect.clone())
            .await
            .expect("young inert effect must not error"),
        crate::application::publish_resilience::AgentWorkspaceRepairOpenPushEffectReconciliation::Pending
    );

    let unchanged = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&effect.idempotency_key)
        .await
        .expect("reload young inert effect")
        .expect("young inert effect exists");
    assert_eq!(unchanged.status, AgentWorkspaceRepairEffectStatus::InFlight);
    assert!(unchanged.completed_at.is_none());
}

#[tokio::test]
async fn inert_push_effect_settlement_fails_closed_when_the_attempt_moved_on() {
    // CAS loser: a concurrent sweep advanced the attempt, so this pass must write nothing.
    let fixture =
        seed_open_push_effect_reconciliation_fixture(107, ReconciliationRemoteShape::Uninitialized)
            .await;
    let (state, mut attempt, effect) = (fixture.state, fixture.attempt, fixture.effect);
    attempt.updated_at += chrono::Duration::seconds(30);

    assert!(matches!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect.clone()).await,
        Err(crate::error::AppError::Conflict(_))
    ));

    let unchanged = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&effect.idempotency_key)
        .await
        .expect("reload effect after a lost CAS")
        .expect("effect after a lost CAS exists");
    assert_eq!(unchanged.status, AgentWorkspaceRepairEffectStatus::InFlight);
    assert!(unchanged.completed_at.is_none());
}

#[tokio::test]
async fn inert_push_effect_with_a_live_repair_claim_stays_pending() {
    // ORDERING REGRESSION GUARD. The live-mutation-claim check must run before the inert
    // classification. If it is ever moved back below the OID-shape branches, this attempt's push
    // — which may be in flight right now — would be declared "never authorized" and its effect
    // terminated underneath a running `git push`.
    let (state, conversation_id, _worktree_parent, _project_dir, identity, owner, real_epoch) =
        seed_publish_continuation_with_lease(108).await;

    let mut attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load live-claim inert continuation")
        .expect("live-claim inert continuation exists");
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Continuing;
    // A durable epoch that no longer matches the still-valid, still-owned real lease, so the
    // reconciliation branch is entered without disturbing the real lease or its claim.
    attempt.target_lease_epoch = Some(real_epoch + 1);
    attempt.updated_at += chrono::Duration::microseconds(1);
    let attempt = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint live-claim inert continuation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("live-claim inert continuation must apply, got {outcome:?}"),
    };

    let idempotency_key = "live-claim-inert-effect";
    let mut effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        idempotency_key,
        chrono::Utc::now() - RECONCILIATION_FIXTURE_INERT_AGE,
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    let effect = match state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: attempt.phase,
            expected_attempt_updated_at: attempt.updated_at,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist live-claim inert effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("expected live-claim inert effect to persist, got {outcome:?}"),
    };

    state
        .branch_update_repo
        .begin_git_mutation(BeginGitMutation {
            identity,
            owner,
            fencing_epoch: real_epoch,
            claim_id: format!("{}:push", effect.id),
            kind: GitMutationKind::Push,
        })
        .await
        .expect("reserve live push claim");

    assert_eq!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect.clone())
            .await
            .expect("live claim must not error"),
        crate::application::publish_resilience::AgentWorkspaceRepairOpenPushEffectReconciliation::Pending
    );

    let unchanged = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(idempotency_key)
        .await
        .expect("reload effect held by a live claim")
        .expect("effect held by a live claim exists");
    assert_eq!(unchanged.status, AgentWorkspaceRepairEffectStatus::InFlight);
    assert!(unchanged.completed_at.is_none());
}

#[tokio::test]
async fn initialized_push_effect_keeps_its_remote_unchanged_reason() {
    // The pre-existing NotApplied path must keep asserting the remote-based proof, which is the
    // only one that ever performed a remote read.
    let fixture = seed_open_push_effect_reconciliation_fixture(
        109,
        ReconciliationRemoteShape::MatchesPrecondition,
    )
    .await;
    let (state, attempt, effect) = (fixture.state, fixture.attempt, fixture.effect);

    assert_eq!(
        reconcile_open_agent_workspace_repair_push_effect(&state, &attempt, effect)
            .await
            .expect("classify initialized effect"),
        crate::application::publish_resilience::AgentWorkspaceRepairOpenPushEffectReconciliation::NotApplied(
            crate::application::publish_resilience::AgentWorkspaceRepairPushEffectNotAppliedReason::RemoteUnchanged
        )
    );
}

// ---------------------------------------------------------------------------
// Step 4 — the heal chain, end to end (the load-bearing gate)
//
// Reproduces the reported production shape and drives it through the ordinary periodic sweep with
// no migration, no DB surgery, and no per-conversation repair.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wedged_inert_effect_continuation_heals_itself_through_the_ordinary_sweep() {
    let fixture =
        seed_open_push_effect_reconciliation_fixture(110, ReconciliationRemoteShape::Uninitialized)
            .await;
    let (state, conversation_id, attempt) =
        (fixture.state, fixture.conversation_id, fixture.attempt);

    // Escalate exactly as production did: the attempt already carries the open-effect attention
    // reason after three failed recovery checks.
    let mut escalated = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load attempt to escalate")
        .expect("attempt exists to escalate");
    let expected_phase = escalated.phase;
    let expected_updated_at = escalated.updated_at;
    escalated
        .pending_reasons
        .push(CONTINUATION_OPEN_EFFECT_ATTENTION_REASON.to_string());
    escalated.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: escalated,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("escalate wedged attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("escalation must apply, got {outcome:?}"),
    }

    // Obligation 7: existing wedged conversations are reachable by a plain scan tick, with no
    // migration and no backfill. `settled_at IS NULL` is the only selector.
    assert!(
        state
            .agent_workspace_repair_repo
            .list_recoverable_repair_attempts()
            .await
            .expect("list recoverable attempts")
            .iter()
            .any(|candidate| candidate.id == attempt.id),
        "the wedged attempt must be picked up by the ordinary periodic scan"
    );

    let epoch_before = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load epoch before heal")
        .expect("attempt exists before heal")
        .target_lease_epoch;

    // ---- Sweep 1: settles the inert effect and clears the fence, nothing else. ----
    {
        let _busy_guard =
            try_acquire_agent_workspace_repair_publish_continuation_guard(&conversation_id)
                .expect("hold continuation guard for the fence-clearing sweep");
        assert_eq!(
            recover_agent_workspace_repair_attempts_for_state(&state)
                .await
                .expect("fence-clearing sweep"),
            0
        );
    }

    assert!(
        state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&attempt.id)
            .await
            .expect("query fence after sweep 1")
            .is_none(),
        "obligation 5: the fence must actually be released"
    );
    let after_first = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload after sweep 1")
        .expect("attempt remains after sweep 1");
    assert_eq!(
        after_first.phase,
        AgentWorkspaceRepairPhase::Continuing,
        "settling the effect must not move the attempt's phase"
    );
    assert!(
        !after_first
            .pending_reasons
            .iter()
            .any(|reason| reason.starts_with("continuation_open_effect_recovery:")),
        "the not-applied arm must not spend an open-effect recovery credit: {after_first:#?}"
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events after sweep 1");
    let not_applied = events
        .iter()
        .find(|event| event.step == "continuation_effect_not_applied")
        .expect("the not-applied event must be recorded");
    assert!(
        not_applied.summary.contains("never started"),
        "an effect that was never initialized must not claim a remote was checked: {:?}",
        not_applied.summary
    );

    // ---- Sweep 2: with the fence gone, the continuation reacquires its target lease. ----
    // This is the hop the fence made unreachable: `reacquire_..._for_continuation` sits *after*
    // the open-effect gate, so before this fix it could never run.
    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("lease-reacquiring sweep"),
        1,
        "obligation 6: the sweep must report a recovery, meaning the continuation actually \
         resumed rather than staying fenced"
    );
    let after_second = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload after sweep 2")
        .expect("attempt remains after sweep 2");
    // Obligation 5/6. Two outcomes are legitimate here and both prove the fence stopped blocking:
    // the continuation reacquires a newer epoch, or it runs through and releases target authority
    // at a settled boundary (`release_repair_lease_if_settled_boundary`, which can only run once
    // `get_open_repair_effect` returns `None`). Still holding the *original* epoch is the only
    // failure, because that is the wedged state: fenced, untouched, going nowhere.
    assert_ne!(
        after_second.target_lease_epoch, epoch_before,
        "the continuation must act on its target authority once unfenced, not sit on the epoch \
         it was wedged with"
    );

    // Obligation 11: a third sweep over the already-settled effect is a no-op and raises no
    // duplicate not-applied event.
    let _ = recover_agent_workspace_repair_attempts_for_state(&state).await;
    let final_events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events after the idempotence sweep");
    assert_eq!(
        final_events
            .iter()
            .filter(|event| event.step == "continuation_effect_not_applied")
            .count(),
        1,
        "settling an already-settled effect must not re-emit its event"
    );
}

// ---------------------------------------------------------------------------
// Step 5 — terminal watchdog
//
// Defense in depth for a future dead-effect shape the primary fix does not recognize. Rather than
// inventing supersession semantics for `Continuing`, force-settle to `Blocked`, which already has
// both a working supersession path and an admitted user Retry action.
// ---------------------------------------------------------------------------

/// Seeds an already-escalated `Continuing` attempt whose open effect stays `Pending` under
/// reconciliation (an unrelated remote), with the effect aged by `effect_age`.
async fn seed_escalated_wedged_attempt(
    suffix: u8,
    effect_age: chrono::Duration,
) -> (
    AppState,
    ChatConversationId,
    crate::domain::entities::AgentWorkspaceRepairAttemptId,
    tempfile::TempDir,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    // `UnrelatedAged` keeps reconciliation permanently `Pending` (a third party advanced the
    // remote), which is what an unrecognized future dead-effect shape looks like, and ages the
    // effect's `created_at` — the watchdog's clock.
    let fixture = seed_open_push_effect_reconciliation_fixture(
        suffix,
        ReconciliationRemoteShape::UnrelatedAged(effect_age),
    )
    .await;
    let (state, conversation_id, attempt_id) = (
        fixture.state,
        fixture.conversation_id,
        fixture.attempt.id.clone(),
    );

    let mut escalated = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load attempt to escalate")
        .expect("attempt exists to escalate");
    let expected_phase = escalated.phase;
    let expected_updated_at = escalated.updated_at;
    escalated
        .pending_reasons
        .push(CONTINUATION_OPEN_EFFECT_ATTENTION_REASON.to_string());
    escalated.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: escalated,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("escalate wedged attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("escalation must apply, got {outcome:?}"),
    }

    // The workspace's `origin` points into these temp dirs, so they must outlive the caller's
    // recovery sweeps.
    (
        state,
        conversation_id,
        attempt_id,
        fixture._worktree_parent,
        fixture._project_dir,
        fixture._remote,
    )
}

#[tokio::test]
async fn escalated_attempt_within_the_wedged_bound_is_left_alone() {
    let (state, conversation_id, _attempt_id, _worktree_parent, _project_dir, _remote) =
        seed_escalated_wedged_attempt(111, chrono::Duration::hours(1)).await;

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("sweep a recently-wedged attempt"),
        0
    );

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload recently-wedged attempt")
        .expect("recently-wedged attempt exists");
    assert_eq!(
        current.phase,
        AgentWorkspaceRepairPhase::Continuing,
        "the watchdog must not pre-empt ordinary recovery"
    );
}

#[tokio::test]
async fn escalated_attempt_past_the_wedged_bound_is_blocked_with_a_usable_retry() {
    let (state, conversation_id, attempt_id, _worktree_parent, _project_dir, _remote) =
        seed_escalated_wedged_attempt(112, chrono::Duration::days(3)).await;

    // A scheduled dispatch before the block: `Blocked` only admits Retry while `next_dispatch_at`
    // is unset, and blocking does not clear it. If the watchdog forgets, the escape hatch produces
    // a blocked workspace with no action at all — worse than the treadmill it replaces.
    let mut scheduled = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load attempt to schedule")
        .expect("attempt exists to schedule");
    let expected_phase = scheduled.phase;
    let expected_updated_at = scheduled.updated_at;
    scheduled.next_dispatch_at = Some(chrono::Utc::now() + chrono::Duration::hours(1));
    scheduled.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: scheduled,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("schedule a dispatch before blocking")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("scheduling must apply, got {outcome:?}"),
    }

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("sweep a long-wedged attempt"),
        1,
        "the watchdog settles the attempt, which counts as a recovery"
    );

    let blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload long-wedged attempt")
        .expect("long-wedged attempt exists");
    assert_eq!(
        blocked.phase,
        AgentWorkspaceRepairPhase::Blocked,
        "obligation 8: a bounded escape hatch must exist"
    );
    assert!(
        blocked
            .pending_reasons
            .iter()
            .any(|reason| reason == CONTINUATION_OPEN_EFFECT_ATTENTION_REASON),
        "the attention reason must survive the block; Retry admission requires it"
    );
    assert!(
        blocked.next_dispatch_at.is_none(),
        "obligation 13: a scheduled dispatch would suppress the Retry action entirely"
    );
    assert!(
        blocked
            .summary
            .as_deref()
            .is_some_and(|summary| !summary.contains("effect fence")),
        "the block summary must be plain language: {:?}",
        blocked.summary
    );

    // Obligation 13: the user is actually offered an action.
    assert_eq!(
        crate::application::agent_workspace_publish_repair_state::load_agent_workspace_repair_operation_recovery_action(
            state.agent_workspace_repair_repo.as_ref(),
            &blocked,
        )
        .await
        .expect("project the recovery action for the blocked attempt"),
        crate::domain::entities::AgentWorkspaceRepairOperationRecoveryAction::RetryRepair
    );

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events after the watchdog block");
    assert!(
        events
            .iter()
            .any(|event| event.step == "continuation_wedged_attempt_blocked"),
        "a time-based auto-block must be visible in the timeline, never silent: {events:#?}"
    );
    assert_eq!(
        blocked.id, attempt_id,
        "the watchdog must not start a successor"
    );
}

#[tokio::test]
async fn wedged_watchdog_clock_is_not_reset_by_joined_pr_evidence() {
    // REGRESSION GUARD for the wrong-clock trap. `start_or_join_repair_attempt` takes its `Joined`
    // branch for any unsettled attempt on the same base ref and unconditionally assigns
    // `current.updated_at = attempt.updated_at`. A wedged workspace is exactly the one that keeps
    // receiving fresh PR evidence, so an `attempt.updated_at`-based bound would be refreshed
    // forever and the watchdog would be dead code on the only shape it exists to catch.
    let (state, conversation_id, _attempt_id, _worktree_parent, _project_dir, _remote) =
        seed_escalated_wedged_attempt(113, chrono::Duration::days(3)).await;

    // Arrive with fresh PR evidence three times, exactly as the poller does.
    for round in 0..3 {
        let outcome = state
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
                reason: format!("fresh conflict PR evidence {round}"),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("deliver fresh PR evidence");
        assert!(
            matches!(
                outcome,
                StartOrJoinAgentWorkspaceRepairAttemptOutcome::Joined(_)
            ),
            "fresh evidence against an unsettled same-base attempt must join, got {outcome:?}"
        );
    }

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("sweep after repeated joins"),
        1,
        "repeated joins must not have deferred the watchdog"
    );

    let blocked = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload after repeated joins")
        .expect("attempt exists after repeated joins");
    assert_eq!(
        blocked.phase,
        AgentWorkspaceRepairPhase::Blocked,
        "obligation 12: the bound must elapse on the effect's clock, not the attempt's"
    );
}
