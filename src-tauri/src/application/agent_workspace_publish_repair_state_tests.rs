use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_recovery::{
    AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX, AUTO_RETRY_READY_REPAIR_REASON_PREFIX,
    EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX,
};
use crate::application::agent_workspace_publish_repair_state::{
    abort_agent_workspace_pr_fix_review_handoff, agent_workspace_repair_is_base_stale_held,
    agent_workspace_repair_is_ci_held, agent_workspace_repair_is_health_held,
    block_agent_workspace_pr_fix_claim, block_agent_workspace_repair_needs_human,
    claim_agent_workspace_repair, classify_agent_workspace_repair_completion_authority,
    classify_agent_workspace_repair_delivery, complete_agent_workspace_pr_fix_claim,
    complete_agent_workspace_repair_claim, continue_agent_workspace_repair_at_boundary,
    continue_agent_workspace_repair_at_boundary_with_review_starter,
    current_agent_workspace_repair_claim_for_completion, inspect_agent_workspace_repair_completion,
    is_machine_repair_reason_marker, last_human_repair_reason,
    mark_agent_workspace_base_update_target, reconcile_active_agent_workspace_repair,
    release_agent_workspace_base_stale_hold,
    reopen_agent_workspace_repair_after_validation_failure, repair_attempt_projection,
    repair_event_authorizes_active_run, reserve_agent_workspace_base_stale_hold,
    reserve_agent_workspace_base_update, reserve_agent_workspace_ci_await,
    reserve_agent_workspace_ci_rerun, reserve_agent_workspace_pre_existing_on_base,
    reserve_agent_workspace_repair_completion_validation, reserve_agent_workspace_repair_dispatch,
    resume_current_agent_workspace_repair_publish, retry_agent_workspace_pr_autofix_hold_override,
    settle_agent_workspace_repair_dispatch_outcome, settle_agent_workspace_repair_failure,
    start_or_join_agent_workspace_repair, start_or_join_agent_workspace_repair_without_projection,
    stop_agent_workspace_pr_autofix_for_hold, terminal_run_authorizes_repair_recovery,
    transition_agent_workspace_repair_attempt, validate_agent_workspace_repair_target_lease,
    AgentWorkspacePrAutofixHoldActionOutcome, AgentWorkspaceRepairDispatchOutcome,
    AgentWorkspaceRepairDispatchSettlement, AgentWorkspaceRepairPublishResumeOutcome,
    AgentWorkspaceRepairStartOutcome, AgentWorkspaceRepairStartRequest,
    AgentWorkspaceRepairTransitionOutcome, DurableRepairWorkspaceReviewStartFuture,
    DurableRepairWorkspaceReviewStarter, PrAutofixCarryover, PublishAuthority,
    AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION, AWAITING_CI_REPAIR_REASON,
    DEFERRED_REPAIR_WAIT_TIMEOUT_SECS, MAX_AGENT_WORKSPACE_CI_RERUN_RETRIES,
    MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES, NEEDS_HUMAN_REPAIR_REASON,
    PRE_EXISTING_ON_BASE_REPAIR_REASON, REPAIR_SENT_STEP, UNCHANGED_HEALTH_REPAIR_REASON,
};
use crate::application::agent_workspace_review::{
    load_agent_workspace_review_context, AgentWorkspaceReviewStart,
};
use crate::application::chat_service::{ChatServiceError, SendResult};
use crate::application::{AppState, GitService};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, AgentRunId, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairCompletionAuthority, AgentWorkspaceRepairContinuation,
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairOutcome,
    AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome, ArtifactId, ChatConversationId,
    GitTargetIdentity, GitTargetLeaseOwner, IdeationAnalysisBaseRefKind, PlanBranchId, Project,
    ProjectId,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentConversationWorkspaceRepository,
    AgentRunRepository, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, AgentWorkspaceRepairRepository,
    BranchUpdateRepository, CreateAgentWorkspaceRepairEffect,
    CreateAgentWorkspaceRepairEffectOutcome, SettleAgentWorkspaceRepairAttempt,
    SettleAgentWorkspaceRepairAttemptOutcome,
};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryBranchUpdateRepository,
};

#[test]
fn repair_reason_helpers_exclude_every_machine_marker_and_preserve_latest_human_context() {
    let human_context = "Resolve the workspace conflict in src/lib.rs.".to_string();
    let newer_human_context = "Retry after the maintainer refreshes the base branch.".to_string();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("repair-reason-helper-human".to_string()),
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
        human_context.clone(),
        NEEDS_HUMAN_REPAIR_REASON.to_string(),
        PRE_EXISTING_ON_BASE_REPAIR_REASON.to_string(),
        UNCHANGED_HEALTH_REPAIR_REASON.to_string(),
        AWAITING_CI_REPAIR_REASON.to_string(),
        format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}2"),
        format!("{AUTO_RETRY_READY_REPAIR_REASON_PREFIX}1"),
        format!("{EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX}bba066f"),
        newer_human_context.clone(),
    ];

    for marker in &attempt.pending_reasons[1..8] {
        assert!(
            is_machine_repair_reason_marker(marker),
            "{marker:?} must remain internal scheduling state"
        );
    }
    assert!(!is_machine_repair_reason_marker(&human_context));
    assert!(!is_machine_repair_reason_marker(&newer_human_context));
    assert!(is_machine_repair_reason_marker(""));
    assert!(is_machine_repair_reason_marker("   \t"));
    assert!(is_machine_repair_reason_marker(&format!(
        "  {NEEDS_HUMAN_REPAIR_REASON}  "
    )));
    assert_eq!(
        last_human_repair_reason(&attempt),
        Some(newer_human_context.as_str()),
        "the most recent human reason must win over earlier context and internal markers"
    );
}

#[test]
fn repair_reason_helpers_return_no_context_when_only_machine_markers_remain() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("repair-reason-helper-markers".to_string()),
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
        NEEDS_HUMAN_REPAIR_REASON.to_string(),
        PRE_EXISTING_ON_BASE_REPAIR_REASON.to_string(),
        UNCHANGED_HEALTH_REPAIR_REASON.to_string(),
        format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}3"),
        format!("{AUTO_RETRY_READY_REPAIR_REASON_PREFIX}2"),
        format!("{EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX}bba066f"),
        "   ".to_string(),
    ];

    assert_eq!(last_human_repair_reason(&attempt), None);
}

#[test]
fn ci_held_predicate_recognizes_rerun_reservations_and_await_holds() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("ci-held-predicate"),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Ready;

    assert!(!agent_workspace_repair_is_ci_held(&attempt));
    attempt.ci_rerun_count = 1;
    assert!(
        !agent_workspace_repair_is_ci_held(&attempt),
        "a rerun count without its fingerprint is not a projected CI hold"
    );
    attempt.ci_rerun_fingerprint = Some("ci-rerun:123".to_string());
    assert!(agent_workspace_repair_is_ci_held(&attempt));

    attempt.ci_rerun_count = 0;
    attempt.ci_rerun_fingerprint = None;
    attempt
        .pending_reasons
        .push(AWAITING_CI_REPAIR_REASON.to_string());
    assert!(agent_workspace_repair_is_ci_held(&attempt));
}

#[test]
fn compatibility_projection_marks_only_stationary_ready_repairs_as_held() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("repair-projection-hold"),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Ready;
    attempt.pending_reasons = vec![UNCHANGED_HEALTH_REPAIR_REASON.to_string()];
    assert_eq!(
        repair_attempt_projection(&attempt, "held", Some(false))
            .pr_supervision_status
            .as_deref(),
        Some("held")
    );

    attempt.pending_reasons = vec!["pr_autofix_head_redrive:local-head".to_string()];
    assert_eq!(
        repair_attempt_projection(&attempt, "redrive", Some(false))
            .pr_supervision_status
            .as_deref(),
        Some("paused"),
        "active publish redrive must never project a held supervision status"
    );

    attempt.pending_reasons.clear();
    assert_eq!(
        repair_attempt_projection(&attempt, "ready", Some(false))
            .pr_supervision_status
            .as_deref(),
        Some("paused"),
        "genuine Ready remains publishable"
    );
}

fn repair_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("repair-state-project".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base".to_string()),
        "ralphx/repair-state".to_string(),
        "/tmp/ralphx-repair-state".to_string(),
    );
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace
}

#[tokio::test]
async fn repair_completion_rejects_execution_owned_non_ideation_workspace_before_git_inspection() {
    let state = AppState::new_test();
    let project = Project::new(
        "Execution-owned repair rejection".to_string(),
        "/not/a/real/execution-owned-workspace".to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project before eligibility inspection");
    let mut workspace = repair_workspace(ChatConversationId::from_string(
        "execution-owned-repair-rejection",
    ));
    workspace.project_id = project.id;
    workspace.linked_plan_branch_id = Some(PlanBranchId::from_string(
        "execution-owned-repair-plan".to_string(),
    ));

    let error = inspect_agent_workspace_repair_completion(&state, &workspace, "main", None)
        .await
        .expect_err("non-Ideation execution-owned workspaces must remain ineligible");

    assert!(
        matches!(error, crate::error::AppError::Validation(_)),
        "the eligibility boundary must reject before attempting to resolve the nonexistent workspace"
    );
}

#[test]
fn repair_delivery_classifier_blocks_deterministic_errors_and_retries_uncertain_delivery() {
    let conversation_id = ChatConversationId::from_string("repair-delivery-classifier");
    let run_id = AgentRunId::from_string("repair-delivery-classifier-run");

    for error in [
        ChatServiceError::InvalidInput("invalid repair configuration".to_string()),
        ChatServiceError::AgentNotAvailable("repair agent is unavailable".to_string()),
        ChatServiceError::SpawnValidation {
            harness: crate::domain::agents::AgentHarnessKind::Claude,
            model: "unsupported-model".to_string(),
            reason: "unsupported repair role".to_string(),
        },
        ChatServiceError::ParseError("invalid repair launch configuration".to_string()),
        ChatServiceError::ContextNotFound("workspace context is missing".to_string()),
        ChatServiceError::ConversationNotFound("workspace conversation is missing".to_string()),
        ChatServiceError::PersonaUnavailable("repair persona is unavailable".to_string()),
    ] {
        assert_eq!(
            classify_agent_workspace_repair_delivery(Err(&error), &conversation_id, &run_id),
            AgentWorkspaceRepairDispatchSettlement::NonRetryableFailure,
            "{error} must block the exact generation because retrying cannot change it"
        );
    }

    for error in [
        ChatServiceError::SpawnFailed("process start interrupted".to_string()),
        ChatServiceError::CommunicationFailed("provider connection reset".to_string()),
        ChatServiceError::RepositoryError("temporary database error".to_string()),
        ChatServiceError::AgentRunFailed("launch observation is incomplete".to_string()),
    ] {
        assert_eq!(
            classify_agent_workspace_repair_delivery(Err(&error), &conversation_id, &run_id),
            AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
            "{error} is delivery-uncertain and must retain Task 38 retry semantics"
        );
    }

    let immediate_start_rejection =
        ChatServiceError::ImmediateStartRejected("another agent run is active".to_string());
    assert_eq!(
        classify_agent_workspace_repair_delivery(
            Err(&immediate_start_rejection),
            &conversation_id,
            &run_id,
        ),
        AgentWorkspaceRepairDispatchSettlement::DeferredQueued,
        "a busy conversation must defer instead of consuming the bounded delivery retry budget"
    );

    let delivered_not_persisted =
        ChatServiceError::MessageDeliveredNotPersisted("transcript write failed".to_string());
    assert_eq!(
        classify_agent_workspace_repair_delivery(
            Err(&delivered_not_persisted),
            &conversation_id,
            &run_id,
        ),
        AgentWorkspaceRepairDispatchSettlement::Delivered,
        "a live process that accepted the repair turn must not receive a duplicate retry"
    );

    let mismatched = SendResult {
        conversation_id: "another-conversation".to_string(),
        agent_run_id: run_id.as_str().to_string(),
        ..Default::default()
    };
    assert_eq!(
        classify_agent_workspace_repair_delivery(Ok(&mismatched), &conversation_id, &run_id),
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        "a mismatched acknowledgement leaves delivery uncertain"
    );

    for queued in [
        SendResult {
            conversation_id: conversation_id.as_str().to_string(),
            agent_run_id: run_id.as_str().to_string(),
            was_queued: true,
            ..Default::default()
        },
        SendResult {
            conversation_id: conversation_id.as_str().to_string(),
            agent_run_id: run_id.as_str().to_string(),
            queued_as_pending: true,
            ..Default::default()
        },
    ] {
        assert_eq!(
            classify_agent_workspace_repair_delivery(Ok(&queued), &conversation_id, &run_id),
            AgentWorkspaceRepairDispatchSettlement::DeferredQueued,
            "an accepted queued delivery waits for capacity and must not consume retry budget"
        );
    }
}

fn review_boundary_git(repo: &std::path::Path, args: &[&str]) -> String {
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

struct RecordingDurableRepairWorkspaceReviewStarter {
    starts: Arc<AtomicUsize>,
}

impl RecordingDurableRepairWorkspaceReviewStarter {
    fn new() -> Self {
        Self {
            starts: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn starts(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }
}

impl DurableRepairWorkspaceReviewStarter for RecordingDurableRepairWorkspaceReviewStarter {
    fn start<'a>(
        &'a self,
        state: Arc<AppState>,
        workspace: &'a AgentConversationWorkspace,
        _force: bool,
    ) -> DurableRepairWorkspaceReviewStartFuture<'a> {
        let starts = Arc::clone(&self.starts);
        let workspace = workspace.clone();
        Box::pin(async move {
            starts.fetch_add(1, Ordering::SeqCst);
            let mut monitor = load_agent_workspace_review_context(state.as_ref(), &workspace)
                .await?
                .monitor;
            monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
            monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
            monitor.last_run_id = Some("durable-reviewer-run".to_string());
            state
                .agent_conversation_workspace_repo
                .upsert_workspace_review_monitor(monitor)
                .await?;
            state
                .agent_conversation_workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    workspace.conversation_id.clone(),
                    "workspace_review",
                    "reviewing",
                    "Started the reviewer for the durable repair generation.",
                    None,
                ))
                .await?;
            let context = load_agent_workspace_review_context(state.as_ref(), &workspace).await?;
            Ok(AgentWorkspaceReviewStart {
                context,
                started: true,
                skipped_reason: None,
                was_queued: false,
            })
        })
    }
}

struct FixedDurableRepairWorkspaceReviewStarter {
    status: AgentWorkspaceReviewGateStatus,
}

impl DurableRepairWorkspaceReviewStarter for FixedDurableRepairWorkspaceReviewStarter {
    fn start<'a>(
        &'a self,
        state: Arc<AppState>,
        workspace: &'a AgentConversationWorkspace,
        _force: bool,
    ) -> DurableRepairWorkspaceReviewStartFuture<'a> {
        Box::pin(async move {
            let mut context =
                load_agent_workspace_review_context(state.as_ref(), workspace).await?;
            context.monitor.review_gate_status = self.status;
            if self.status == AgentWorkspaceReviewGateStatus::Passed {
                state
                    .review_settings_repo
                    .update_settings(&crate::domain::review::ReviewSettings {
                        require_workspace_review: false,
                        ..crate::domain::review::ReviewSettings::default()
                    })
                    .await
                    .map_err(|error| {
                        crate::error::AppError::Infrastructure(format!(
                            "update review settings in fixed starter: {error}"
                        ))
                    })?;
            }
            Ok(AgentWorkspaceReviewStart {
                context,
                started: true,
                skipped_reason: None,
                was_queued: false,
            })
        })
    }
}

async fn workspace_review_boundary_context(
    state: &AppState,
    root: &std::path::Path,
    require_workspace_review: bool,
) -> AgentWorkspaceRepairAttempt {
    let repo = root.join("workspace-review-boundary");
    std::fs::create_dir_all(&repo).expect("review boundary repository should be created");
    review_boundary_git(&repo, &["init", "-b", "main"]);
    review_boundary_git(&repo, &["config", "user.email", "test@example.com"]);
    review_boundary_git(&repo, &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    review_boundary_git(&repo, &["add", "README.md"]);
    review_boundary_git(&repo, &["commit", "-m", "base"]);
    let base_commit = review_boundary_git(&repo, &["rev-parse", "HEAD"]);
    std::fs::write(repo.join("repair.md"), "repair complete\n")
        .expect("repair file should be written");
    review_boundary_git(&repo, &["add", "repair.md"]);
    review_boundary_git(&repo, &["commit", "-m", "repair"]);

    let mut project = Project::new(
        "Durable repair review boundary".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(root.join("worktrees").to_string_lossy().to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("review boundary project should persist");
    let conversation_id = ChatConversationId::from_string("repair-review-boundary");
    let branch_name = "ralphx/repair-review-boundary".to_string();
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("review boundary workspace path should resolve");
    GitService::create_worktree(&repo, &workspace_path, &branch_name, "main")
        .await
        .expect("review boundary worktree should be created");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some(base_commit.clone()),
        branch_name,
        workspace_path.to_string_lossy().to_string(),
    );
    workspace.auto_publish_enabled = true;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("review boundary workspace should persist");
    state
        .review_settings_repo
        .update_settings(&crate::domain::review::ReviewSettings {
            require_workspace_review,
            ..crate::domain::review::ReviewSettings::default()
        })
        .await
        .expect("review boundary settings should persist");
    let mut request = repair_start_request(
        conversation_id,
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::Publish,
        "base update repaired",
    );
    request.target_base_commit = Some(base_commit);
    start_or_join_agent_workspace_repair(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        request,
    )
    .await
    .expect("review boundary repair attempt should start")
    .into_attempt()
}

async fn checkpoint_workspace_review_boundary_lease(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
) -> (AgentWorkspaceRepairAttempt, GitTargetIdentity, u64) {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await
        .expect("review boundary workspace should load")
        .expect("review boundary workspace should exist");
    let identity = GitService::canonical_target_identity(
        std::path::Path::new(&workspace.worktree_path),
        &workspace.branch_name,
    )
    .await
    .expect("review boundary target identity should resolve");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner,
        })
        .await
        .expect("review boundary repair lease should acquire")
    else {
        panic!("review boundary repair lease should be newly acquired");
    };
    let mut checkpointed = attempt.clone();
    checkpointed.git_common_dir = Some(identity.git_common_dir().to_string_lossy().into_owned());
    checkpointed.target_ref = Some(identity.full_ref().to_string());
    checkpointed.target_identity_version = Some(
        crate::application::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION,
    );
    checkpointed.target_lease_epoch = Some(fencing_epoch);
    checkpointed.updated_at += chrono::Duration::microseconds(1);
    let checkpointed = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: checkpointed,
            expected_phase: attempt.phase,
            expected_updated_at: attempt.updated_at,
            next_phase: attempt.phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("review boundary lease should checkpoint on the exact attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected review boundary lease checkpoint, got {outcome:?}"),
    };
    (checkpointed, identity, fencing_epoch)
}

fn assert_repair_target_authority_is_cleared(attempt: &AgentWorkspaceRepairAttempt) {
    assert!(attempt.git_common_dir.is_none());
    assert!(attempt.target_ref.is_none());
    assert!(attempt.target_identity_version.is_none());
    assert!(attempt.target_lease_epoch.is_none());
}

fn repair_start_request(
    conversation_id: ChatConversationId,
    source: AgentWorkspaceRepairSource,
    continuation: AgentWorkspaceRepairContinuation,
    reason: &str,
) -> AgentWorkspaceRepairStartRequest {
    AgentWorkspaceRepairStartRequest {
        conversation_id,
        source,
        continuation,
        target_base_ref: "main".to_string(),
        target_base_commit: Some("base-a".to_string()),
        verified_newer_base: false,
        reason: reason.to_string(),
        summary: "Repair requested.".to_string(),
        auto_merge_current: None,
        explicit_publish_requested: false,
        retry_blocked: false,
        carryover_pr_autofix_evidence: None,
    }
}

#[tokio::test]
async fn started_repair_carries_forward_observed_pr_autofix_evidence() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("repair-attempt-carryover");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();

    // Without the carryover, the successor starts with no failure identity and the next poll can
    // no longer tell an unchanged failure from a new one.
    let mut request = repair_start_request(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::Publish,
        "pr autofix successor",
    );
    request.carryover_pr_autofix_evidence = Some(PrAutofixCarryover {
        dispatch_head_commit: Some("head-observed".to_string()),
        health_fingerprint: Some("ci:Clippy:failure".to_string()),
    });

    let attempt = match start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        request,
    )
    .await
    .unwrap()
    {
        AgentWorkspaceRepairStartOutcome::Started(attempt) => attempt,
        outcome => panic!("expected a started attempt, got {outcome:?}"),
    };

    assert_eq!(
        attempt.pr_autofix_dispatch_head_commit.as_deref(),
        Some("head-observed")
    );
    assert_eq!(
        attempt.pr_autofix_health_fingerprint.as_deref(),
        Some("ci:Clippy:failure")
    );
}

#[tokio::test]
async fn explicit_attempt_coalesces_concurrent_starts_without_duplicate_audit_events() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("repair-attempt-coalesce");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let first = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::BaseUpdate,
            AgentWorkspaceRepairContinuation::UpdateOnly,
            "base update",
        ),
    );
    let second = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "publish failure",
        ),
    );
    let (first, second) = tokio::join!(first, second);
    let outcomes = [first.unwrap(), second.unwrap()];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AgentWorkspaceRepairStartOutcome::Started(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AgentWorkspaceRepairStartOutcome::Joined(_)))
            .count(),
        1
    );
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .unwrap()
        .expect("one coalesced repair attempt");
    assert_eq!(current.generation, 1);
    assert_eq!(
        current.continuation,
        AgentWorkspaceRepairContinuation::Publish
    );
    assert_eq!(current.pending_reasons.len(), 2);
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn stale_phase_transition_cannot_mutate_projection_or_append_audit_events() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("repair-attempt-stale-transition");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let started = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::BaseUpdate,
            AgentWorkspaceRepairContinuation::UpdateOnly,
            "base update",
        ),
    )
    .await
    .unwrap()
    .into_attempt();
    let before_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace");

    let outcome = transition_agent_workspace_repair_attempt(
        Arc::clone(&repair_repo),
        started,
        AgentWorkspaceRepairPhase::Dispatching,
        "late dispatch",
        None,
    )
    .await
    .unwrap();

    assert!(outcome.is_stale());
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace")
            .pr_supervision_updated_at,
        before_workspace.pr_supervision_updated_at
    );
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn validation_rejection_reopens_the_same_repair_generation_and_projection() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("repair-validation-reopen");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let started = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::BaseUpdate,
            AgentWorkspaceRepairContinuation::Publish,
            "base update",
        ),
    )
    .await
    .unwrap()
    .into_attempt();
    let validating = reserve_agent_workspace_repair_completion_validation(
        Arc::clone(&repair_repo),
        started,
        Some(true),
    )
    .await
    .unwrap();
    let AgentWorkspaceRepairTransitionOutcome::Applied(validating) = validating else {
        panic!("current repair must reserve validation");
    };

    let reopened = reopen_agent_workspace_repair_after_validation_failure(
        Arc::clone(&repair_repo),
        validating,
        Some(true),
    )
    .await
    .unwrap();
    let AgentWorkspaceRepairTransitionOutcome::Applied(reopened) = reopened else {
        panic!("validation rejection must reopen the same generation");
    };

    assert_eq!(reopened.phase, AgentWorkspaceRepairPhase::Repairing);
    assert_eq!(reopened.generation, 1);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace projection");
    assert_eq!(
        workspace.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(workspace.pr_auto_merge_current, Some(true));
}

#[tokio::test]
async fn repair_start_and_durable_lease_validation_fail_closed_without_canonical_owners() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("repair-missing-canonical-owner");
    let missing_workspace = start_or_join_agent_workspace_repair_without_projection(
        Arc::clone(&repair_repo),
        workspace_repo as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id,
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "missing workspace",
        ),
    )
    .await
    .expect_err("repair cannot start without its canonical workspace");
    assert!(missing_workspace.to_string().contains("workspace"));

    let branch_update_repo = MemoryBranchUpdateRepository::new();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("repair-invalid-durable-lease"),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.git_common_dir = Some("/tmp/repair-invalid-durable-lease".to_string());
    attempt.target_ref = Some("refs/heads/ralphx/repair-invalid-durable-lease".to_string());
    attempt.target_lease_epoch = Some(1);
    attempt.target_identity_version = Some(AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION + 1);
    let unsupported = validate_agent_workspace_repair_target_lease(&branch_update_repo, &attempt)
        .await
        .expect_err("unknown canonical identity versions must be rejected");
    assert!(unsupported.to_string().contains("unsupported"));

    attempt.target_identity_version = Some(AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION);
    let missing_lease = validate_agent_workspace_repair_target_lease(&branch_update_repo, &attempt)
        .await
        .expect_err("persisted target metadata needs the matching durable lease");
    assert!(missing_lease.to_string().contains("missing"));
}

#[tokio::test]
async fn repair_completion_validation_rejects_unpublishable_workspace_shapes() {
    let temp = tempfile::tempdir().expect("repair validation tempdir");
    let state = AppState::new_test();
    let attempt = workspace_review_boundary_context(&state, temp.path(), false).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await
        .expect("load validation workspace")
        .expect("validation workspace exists");

    let mut unlinked_ideation = workspace.clone();
    unlinked_ideation.mode = AgentConversationWorkspaceMode::Ideation;
    let unlinked = inspect_agent_workspace_repair_completion(
        &state,
        &unlinked_ideation,
        "main",
        attempt.target_base_commit.as_deref(),
    )
    .await
    .expect_err("unlinked ideation cannot publish a direct repair");
    assert!(unlinked.to_string().contains("linked plan branch"));

    let wrong_branch = AgentConversationWorkspace {
        branch_name: "ralphx/not-checked-out".to_string(),
        ..workspace.clone()
    };
    let branch_error = inspect_agent_workspace_repair_completion(
        &state,
        &wrong_branch,
        "main",
        attempt.target_base_commit.as_deref(),
    )
    .await
    .expect_err("validation must prove the canonical branch is checked out");
    assert!(branch_error.to_string().contains("expected branch"));

    let missing_base = inspect_agent_workspace_repair_completion(
        &state,
        &workspace,
        " ",
        attempt.target_base_commit.as_deref(),
    )
    .await
    .expect_err("validation requires its durable target base ref");
    assert!(missing_base.to_string().contains("target base ref"));
}

#[tokio::test]
async fn publish_resume_phase_guards_preserve_current_durable_authority() {
    async fn state_in_phase(
        suffix: &str,
        phase: AgentWorkspaceRepairPhase,
    ) -> (AppState, AgentWorkspaceRepairAttempt) {
        let state = AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string(format!("repair-resume-phase-{suffix}"));
        state
            .agent_conversation_workspace_repo
            .create_or_update(repair_workspace(conversation_id.clone()))
            .await
            .expect("seed phase workspace");
        let started = start_or_join_agent_workspace_repair(
            Arc::clone(&state.agent_workspace_repair_repo),
            Arc::clone(&state.agent_conversation_workspace_repo),
            repair_start_request(
                conversation_id,
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "phase guard",
            ),
        )
        .await
        .expect("start phase guard repair")
        .into_attempt();
        if phase == AgentWorkspaceRepairPhase::Requested {
            return (state, started);
        }
        let mut attempt = started.clone();
        attempt.phase = phase;
        attempt.updated_at += chrono::Duration::microseconds(1);
        let attempt = match state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at: started.updated_at,
                next_phase: phase,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("persist phase guard")
        {
            AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
            outcome => panic!("expected current phase guard, got {outcome:?}"),
        };
        (state, attempt)
    }

    let (ready_state, ready) = state_in_phase("ready", AgentWorkspaceRepairPhase::Ready).await;
    assert_eq!(
        resume_current_agent_workspace_repair_publish(
            &ready_state,
            &ready.conversation_id,
            "background publish probe",
            false,
            PublishAuthority::VerifiedAutomation,
        )
        .await
        .expect("background probe leaves Ready parked"),
        AgentWorkspaceRepairPublishResumeOutcome::Ready
    );
    let missing_project = resume_current_agent_workspace_repair_publish(
        &ready_state,
        &ready.conversation_id,
        "explicit publish",
        true,
        PublishAuthority::UserExplicit,
    )
    .await
    .expect_err("explicit resume requires its canonical project");
    assert!(missing_project.to_string().contains("project"));

    let (awaiting_state, awaiting) =
        state_in_phase("awaiting", AgentWorkspaceRepairPhase::AwaitingReview).await;
    awaiting_state
        .agent_conversation_workspace_repo
        .delete(&awaiting.conversation_id)
        .await
        .expect("remove canonical workspace");
    let missing_workspace = resume_current_agent_workspace_repair_publish(
        &awaiting_state,
        &awaiting.conversation_id,
        "resume review",
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    .expect_err("review resume requires its canonical workspace");
    assert!(missing_workspace.to_string().contains("workspace"));

    let (blocked_state, blocked) =
        state_in_phase("blocked", AgentWorkspaceRepairPhase::Blocked).await;
    assert_eq!(
        resume_current_agent_workspace_repair_publish(
            &blocked_state,
            &blocked.conversation_id,
            "blocked probe",
            false,
            PublishAuthority::VerifiedAutomation,
        )
        .await
        .expect("blocked repair remains blocked"),
        AgentWorkspaceRepairPublishResumeOutcome::Blocked
    );

    let (requested_state, requested) =
        state_in_phase("requested", AgentWorkspaceRepairPhase::Requested).await;
    assert_eq!(
        resume_current_agent_workspace_repair_publish(
            &requested_state,
            &requested.conversation_id,
            "requested probe",
            false,
            PublishAuthority::VerifiedAutomation,
        )
        .await
        .expect("live requested repair remains busy"),
        AgentWorkspaceRepairPublishResumeOutcome::Busy
    );
}

#[tokio::test]
async fn dispatch_refuses_to_replace_an_open_external_effect() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("repair-dispatch-open-effect");
    state
        .agent_conversation_workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("seed dispatch workspace");
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        repair_start_request(
            conversation_id,
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "open effect",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    let effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::CreatePr,
        "dispatch-open-effect",
        chrono::Utc::now(),
    );
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: attempt.id.clone(),
                generation: attempt.generation,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_attempt_updated_at: attempt.updated_at,
                effect,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("checkpoint open effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));
    let target = GitTargetIdentity::new(
        PathBuf::from("/tmp/repair-dispatch-open-effect"),
        "refs/heads/ralphx/repair-dispatch-open-effect",
    )
    .expect("canonical target");
    let error = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target.clone(),
        attempt,
        AgentRunId::from_string("repair-dispatch-open-effect-run"),
        "dispatch repair",
        None,
    )
    .await
    .expect_err("dispatch cannot replace an open external effect");
    assert!(error.to_string().contains("active Git effect"));
    assert!(
        state
            .branch_update_repo
            .get_target_lease(&target)
            .await
            .expect("inspect target lease")
            .is_none(),
        "rejected dispatch must not acquire Git authority"
    );
}

#[tokio::test]
async fn dispatch_reservation_releases_target_authority_for_stale_and_missing_generations() {
    let temp = tempfile::tempdir().expect("dispatch fencing tempdir");
    let state = AppState::new_test();
    let attempt = workspace_review_boundary_context(&state, temp.path(), false).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await
        .unwrap()
        .expect("dispatch workspace");
    let target_identity = GitService::canonical_target_identity(
        std::path::Path::new(&workspace.worktree_path),
        &workspace.branch_name,
    )
    .await
    .expect("dispatch target identity");

    let mut not_due = attempt.clone();
    not_due.next_dispatch_at = Some(chrono::Utc::now() + chrono::Duration::minutes(1));
    assert!(matches!(
        reserve_agent_workspace_repair_dispatch(
            Arc::clone(&state.agent_workspace_repair_repo),
            Arc::clone(&state.branch_update_repo),
            target_identity.clone(),
            not_due,
            AgentRunId::from_string("not-due-dispatch-run"),
            "not due",
            None,
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairDispatchOutcome::Stale(_)
    ));
    assert!(
        state
            .branch_update_repo
            .get_target_lease(&target_identity)
            .await
            .unwrap()
            .is_none(),
        "a not-due generation must not acquire target authority"
    );

    let mut advanced = attempt.clone();
    advanced.summary = Some("newer same-generation snapshot".to_string());
    advanced.updated_at += chrono::Duration::microseconds(1);
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                attempt: advanced,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at: attempt.updated_at,
                next_phase: AgentWorkspaceRepairPhase::Requested,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .unwrap(),
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
    assert!(matches!(
        reserve_agent_workspace_repair_dispatch(
            Arc::clone(&state.agent_workspace_repair_repo),
            Arc::clone(&state.branch_update_repo),
            target_identity.clone(),
            attempt,
            AgentRunId::from_string("stale-dispatch-run"),
            "stale dispatch",
            None,
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairDispatchOutcome::Stale(_)
    ));
    assert!(
        state
            .branch_update_repo
            .get_target_lease(&target_identity)
            .await
            .unwrap()
            .expect("stale dispatch lease receipt")
            .is_released(),
        "a stale checkpoint must release the lease it acquired"
    );

    let missing = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("missing-dispatch-generation"),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    assert!(matches!(
        reserve_agent_workspace_repair_dispatch(
            Arc::clone(&state.agent_workspace_repair_repo),
            Arc::clone(&state.branch_update_repo),
            target_identity.clone(),
            missing,
            AgentRunId::from_string("missing-dispatch-run"),
            "missing dispatch",
            None,
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairDispatchOutcome::Missing
    ));
    assert!(
        state
            .branch_update_repo
            .get_target_lease(&target_identity)
            .await
            .unwrap()
            .expect("missing dispatch lease receipt")
            .is_released(),
        "a missing generation must release the lease it acquired"
    );
}

#[tokio::test]
async fn join_upgrades_continuation_and_re_reads_current_publish_preferences() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("repair-attempt-preferences");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.auto_publish_enabled = false;
    workspace.pr_auto_merge_desired = false;
    workspace_repo.create_or_update(workspace).await.unwrap();
    start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::BaseUpdate,
            AgentWorkspaceRepairContinuation::UpdateOnly,
            "base update",
        ),
    )
    .await
    .unwrap();

    let mut changed = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace");
    changed.auto_publish_enabled = true;
    changed.pr_auto_merge_desired = true;
    changed.pr_auto_merge_method = "squash".to_string();
    workspace_repo.create_or_update(changed).await.unwrap();

    let joined = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "manual publish",
        ),
    )
    .await
    .unwrap()
    .into_attempt();

    assert_eq!(
        joined.continuation,
        AgentWorkspaceRepairContinuation::Publish
    );
    assert!(joined.auto_publish_enabled);
    assert!(joined.auto_merge_desired);
    assert_eq!(joined.auto_merge_method.as_deref(), Some("squash"));
    assert_eq!(
        joined.pending_reasons,
        vec!["base update", "manual publish"]
    );
}

#[tokio::test]
async fn continuation_boundary_re_reads_auto_publish_preference_before_leaving_repair() {
    let state = AppState::new_test();
    let mut review_settings = state
        .review_settings_repo
        .get_settings()
        .await
        .expect("review settings should load");
    review_settings.require_workspace_review = false;
    state
        .review_settings_repo
        .update_settings(&review_settings)
        .await
        .expect("review settings should update");

    let conversation_id = ChatConversationId::from_string("repair-attempt-continuation-gate");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.auto_publish_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "publish failure",
        ),
    )
    .await
    .expect("attempt should start")
    .into_attempt();

    let mut changed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    changed.auto_publish_enabled = true;
    changed.pr_auto_merge_desired = true;
    state
        .agent_conversation_workspace_repo
        .create_or_update(changed)
        .await
        .expect("changed preferences should persist");

    let outcome = continue_agent_workspace_repair_at_boundary(
        &state,
        attempt,
        AgentWorkspaceRepairPhase::Requested,
        "repair completed",
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    .expect("continuation boundary should use current preferences");
    let crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairTransitionOutcome::Applied(
        continued,
    ) = outcome
    else {
        panic!("continuation should own the requested attempt");
    };
    assert_eq!(
        continued.phase,
        AgentWorkspaceRepairPhase::ContinuationPending
    );
    assert!(continued.auto_publish_enabled);
    assert!(continued.auto_merge_desired);
}

#[tokio::test]
async fn continuation_boundary_honors_persisted_explicit_publish_consent() {
    let state = AppState::new_test();
    let mut review_settings = state
        .review_settings_repo
        .get_settings()
        .await
        .expect("review settings should load");
    review_settings.require_workspace_review = false;
    state
        .review_settings_repo
        .update_settings(&review_settings)
        .await
        .expect("review settings should update");

    let conversation_id = ChatConversationId::from_string("repair-attempt-explicit-consent");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.auto_publish_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut request = repair_start_request(
        conversation_id,
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "explicit publish failure",
    );
    request.explicit_publish_requested = true;
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        request,
    )
    .await
    .expect("attempt should start")
    .into_attempt();

    let outcome = continue_agent_workspace_repair_at_boundary(
        &state,
        attempt,
        AgentWorkspaceRepairPhase::Requested,
        "repair completed",
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    .expect("persisted consent should authorize the continuation");
    let AgentWorkspaceRepairTransitionOutcome::Applied(continued) = outcome else {
        panic!("current repair generation should continue");
    };
    assert_eq!(
        continued.phase,
        AgentWorkspaceRepairPhase::ContinuationPending
    );
    assert!(continued.explicit_publish_requested);
    assert!(!continued.auto_publish_enabled);
}

#[tokio::test]
async fn inactive_repair_lease_review_wait_restart_and_pass_are_fenced_once() {
    let temp = tempfile::tempdir().expect("review boundary tempdir should be created");
    let state = AppState::new_test();
    let attempt = workspace_review_boundary_context(&state, temp.path(), true).await;
    let (attempt, target_identity, initial_epoch) =
        checkpoint_workspace_review_boundary_lease(&state, attempt).await;
    let starter = RecordingDurableRepairWorkspaceReviewStarter::new();

    let outcome = continue_agent_workspace_repair_at_boundary_with_review_starter(
        &state,
        attempt.clone(),
        AgentWorkspaceRepairPhase::Requested,
        "repair completed",
        false,
        PublishAuthority::VerifiedAutomation,
        &starter,
    )
    .await
    .expect("current repair generation should start Workspace Review");
    let AgentWorkspaceRepairTransitionOutcome::Applied(awaiting_review) = outcome else {
        panic!("current repair generation should retain durable review authority");
    };
    assert_eq!(
        awaiting_review.phase,
        AgentWorkspaceRepairPhase::AwaitingReview
    );
    assert_repair_target_authority_is_cleared(&awaiting_review);
    let released_lease = state
        .branch_update_repo
        .get_target_lease(&target_identity)
        .await
        .expect("review boundary lease should load")
        .expect("review boundary lease should remain durable");
    assert!(released_lease.is_released());
    assert_eq!(released_lease.fencing_epoch(), initial_epoch);
    assert_eq!(
        starter.starts(),
        1,
        "the current repair starts one reviewer"
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&attempt.conversation_id)
            .await
            .expect("review events should load")
            .into_iter()
            .filter(|event| event.step == "workspace_review" && event.status == "reviewing")
            .count(),
        1,
        "the reviewer start must have one durable audit event"
    );

    let replay = continue_agent_workspace_repair_at_boundary_with_review_starter(
        &state,
        awaiting_review.clone(),
        AgentWorkspaceRepairPhase::AwaitingReview,
        "recovery replay",
        false,
        PublishAuthority::VerifiedAutomation,
        &starter,
    )
    .await
    .expect("replaying an active review handoff should be safe");
    assert!(matches!(
        replay,
        AgentWorkspaceRepairTransitionOutcome::Applied(_)
    ));
    assert_eq!(
        starter.starts(),
        1,
        "recovery replay must reuse the reviewer owned by the same generation"
    );

    let review_context = load_agent_workspace_review_context(
        &state,
        &state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&awaiting_review.conversation_id)
            .await
            .expect("review boundary workspace should reload")
            .expect("review boundary workspace should remain"),
    )
    .await
    .expect("review monitor should reload");
    let review_target = review_context
        .target
        .expect("started Workspace Review should retain its current target");
    let mut monitor = review_context.monitor;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("durable-repair-review-artifact"));
    monitor.review_artifact_version = Some(1);
    monitor.review_requested_changes_artifact_id = Some(ArtifactId::from_string(
        "durable-repair-requested-changes-artifact",
    ));
    monitor.review_requested_changes_artifact_version = Some(1);
    monitor.reviewed_target_scope = Some(review_target.scope);
    monitor.reviewed_head_sha = review_target.head_sha.clone();
    monitor.reviewed_diff_fingerprint = Some(review_target.diff_fingerprint.clone());
    monitor.current_target_scope = Some(review_target.scope);
    monitor.current_diff_fingerprint = Some(review_target.diff_fingerprint.clone());
    monitor.workspace_head_sha = review_target.head_sha;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("passed review should persist");
    let resumed = resume_current_agent_workspace_repair_publish(
        &state,
        &awaiting_review.conversation_id,
        "resume after persisted review pass",
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    .expect("passed review must reacquire the canonical target before continuation");
    let AgentWorkspaceRepairPublishResumeOutcome::Continue(resumed) = resumed else {
        panic!("passed review should continue the existing durable generation: {resumed:?}");
    };
    assert_eq!(
        resumed.phase,
        AgentWorkspaceRepairPhase::ContinuationPending
    );
    assert_eq!(
        resumed.git_common_dir.as_deref(),
        Some(target_identity.git_common_dir().to_string_lossy().as_ref())
    );
    assert_eq!(
        resumed.target_ref.as_deref(),
        Some(target_identity.full_ref())
    );
    assert_eq!(
        resumed.target_identity_version,
        Some(
            crate::application::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION
        )
    );
    let resumed_epoch = resumed
        .target_lease_epoch
        .expect("resumed continuation should checkpoint a new lease epoch");
    assert!(resumed_epoch > initial_epoch);
    let duplicate_resume = resume_current_agent_workspace_repair_publish(
        &state,
        &resumed.conversation_id,
        "duplicate resume after persisted review pass",
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    .expect("duplicate review resume should remain side-effect free");
    assert_eq!(
        duplicate_resume,
        AgentWorkspaceRepairPublishResumeOutcome::Busy
    );
    assert!(
        state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&resumed.id)
            .await
            .expect("repair effects should load")
            .is_none(),
        "review restart/pass must not create a Git or publication effect before the publisher owns it"
    );

    let stale = continue_agent_workspace_repair_at_boundary_with_review_starter(
        &state,
        attempt,
        AgentWorkspaceRepairPhase::Requested,
        "stale completion",
        false,
        PublishAuthority::VerifiedAutomation,
        &starter,
    )
    .await
    .expect("stale completion should be rejected without reviewer side effects");
    assert!(matches!(
        stale,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
    ));
    assert_eq!(
        starter.starts(),
        1,
        "a stale repair generation cannot start a second reviewer"
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&ChatConversationId::from_string("repair-review-boundary"))
            .await
            .expect("review events should remain stable")
            .into_iter()
            .filter(|event| event.step == "workspace_review" && event.status == "reviewing")
            .count(),
        1,
        "duplicate and stale completions must not append a second reviewer event"
    );
}

#[tokio::test]
async fn workspace_review_start_outcomes_keep_or_block_the_exact_repair_generation() {
    for (status, expected_phase) in [
        (
            AgentWorkspaceReviewGateStatus::Passed,
            AgentWorkspaceRepairPhase::ContinuationPending,
        ),
        (
            AgentWorkspaceReviewGateStatus::Reviewing,
            AgentWorkspaceRepairPhase::AwaitingReview,
        ),
        (
            AgentWorkspaceReviewGateStatus::Blocking,
            AgentWorkspaceRepairPhase::Blocked,
        ),
        (
            AgentWorkspaceReviewGateStatus::Required,
            AgentWorkspaceRepairPhase::Blocked,
        ),
    ] {
        let temp = tempfile::tempdir().expect("review outcome tempdir");
        let state = AppState::new_test();
        let attempt = workspace_review_boundary_context(&state, temp.path(), true).await;
        let (attempt, target_identity, _) =
            checkpoint_workspace_review_boundary_lease(&state, attempt).await;
        let starter = FixedDurableRepairWorkspaceReviewStarter { status };

        let outcome = continue_agent_workspace_repair_at_boundary_with_review_starter(
            &state,
            attempt,
            AgentWorkspaceRepairPhase::Requested,
            "repair completed",
            false,
            PublishAuthority::VerifiedAutomation,
            &starter,
        )
        .await
        .expect("review start outcome must settle durably");
        let AgentWorkspaceRepairTransitionOutcome::Applied(current) = outcome else {
            panic!("current review handoff must retain the exact generation");
        };

        assert_eq!(current.phase, expected_phase);
        if expected_phase == AgentWorkspaceRepairPhase::ContinuationPending {
            assert_eq!(
                current.target_ref.as_deref(),
                Some(target_identity.full_ref()),
                "a passed review must reacquire exact target authority"
            );
        } else {
            assert_repair_target_authority_is_cleared(&current);
            assert!(
                state
                    .branch_update_repo
                    .get_target_lease(&target_identity)
                    .await
                    .expect("review lease lookup")
                    .expect("review lease remains durable")
                    .is_released(),
                "every inactive review boundary must release its Git target lease"
            );
        }
        if expected_phase == AgentWorkspaceRepairPhase::Blocked {
            assert!(
                current
                    .blocker
                    .as_deref()
                    .is_some_and(|blocker| blocker.contains("Workspace Review")),
                "non-progressing review starts need an actionable durable blocker"
            );
        } else {
            assert!(current.blocker.is_none());
        }
    }
}

#[tokio::test]
async fn inactive_repair_lease_ready_manual_publish_reacquires_before_continuation() {
    let temp = tempfile::tempdir().expect("ready boundary tempdir should be created");
    let state = AppState::new_test();
    let attempt = workspace_review_boundary_context(&state, temp.path(), false).await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await
        .expect("ready boundary workspace should load")
        .expect("ready boundary workspace should exist");
    workspace.auto_publish_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("ready boundary should persist disabled Auto Publish");
    let (attempt, target_identity, released_epoch) =
        checkpoint_workspace_review_boundary_lease(&state, attempt).await;

    let ready = continue_agent_workspace_repair_at_boundary(
        &state,
        attempt,
        AgentWorkspaceRepairPhase::Requested,
        "repair is ready for user-selected publication",
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    .expect("disabled Auto Publish should park the exact repair generation at Ready");
    let AgentWorkspaceRepairTransitionOutcome::Applied(ready) = ready else {
        panic!("ready boundary should retain the current durable repair generation");
    };
    assert_eq!(
        ready.phase,
        AgentWorkspaceRepairPhase::Ready,
        "disabled Auto Publish should park Ready: {ready:?}"
    );
    assert_repair_target_authority_is_cleared(&ready);
    assert!(
        state
            .branch_update_repo
            .get_target_lease(&target_identity)
            .await
            .expect("ready boundary lease should load")
            .expect("ready boundary lease should exist")
            .is_released(),
        "parking at Ready must release its exact canonical target lease"
    );

    let resumed = resume_current_agent_workspace_repair_publish(
        &state,
        &ready.conversation_id,
        "user selected Commit & Publish",
        true,
        PublishAuthority::UserExplicit,
    )
    .await
    .expect("manual publication should reacquire its canonical target authority");
    let AgentWorkspaceRepairPublishResumeOutcome::Continue(resumed) = resumed else {
        panic!("manual publication should continue the same durable repair generation");
    };
    assert_eq!(
        resumed.phase,
        AgentWorkspaceRepairPhase::ContinuationPending
    );
    assert!(resumed.explicit_publish_requested);
    assert_eq!(
        resumed.target_ref.as_deref(),
        Some(target_identity.full_ref())
    );
    let resumed_epoch = resumed
        .target_lease_epoch
        .expect("manual publication should checkpoint its newly acquired lease epoch");
    assert!(resumed_epoch > released_epoch);
    let lease = state
        .branch_update_repo
        .get_target_lease(&target_identity)
        .await
        .expect("resumed lease should load")
        .expect("resumed lease should exist");
    assert!(!lease.is_released());
    assert_eq!(
        lease.owner(),
        &GitTargetLeaseOwner::agent_workspace_repair(resumed.id.as_str())
    );
    assert_eq!(lease.fencing_epoch(), resumed_epoch);

    let duplicate = resume_current_agent_workspace_repair_publish(
        &state,
        &resumed.conversation_id,
        "duplicate manual Commit & Publish",
        true,
        PublishAuthority::UserExplicit,
    )
    .await
    .expect("duplicate manual publish must not take a second target lease or effect");
    assert_eq!(duplicate, AgentWorkspaceRepairPublishResumeOutcome::Busy);
    assert!(
        state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&resumed.id)
            .await
            .expect("manual publish effects should load")
            .is_none(),
        "a duplicated manual resume must not create a Git/GitHub effect before the publisher owns it"
    );
}

#[tokio::test]
async fn inactive_repair_lease_ready_resume_rejects_open_effect_without_reacquiring() {
    let temp = tempfile::tempdir().expect("open effect boundary tempdir should be created");
    let state = AppState::new_test();
    let attempt = workspace_review_boundary_context(&state, temp.path(), false).await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await
        .expect("open effect workspace should load")
        .expect("open effect workspace should exist");
    workspace.auto_publish_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("open effect boundary should persist disabled Auto Publish");
    let (attempt, target_identity, released_epoch) =
        checkpoint_workspace_review_boundary_lease(&state, attempt).await;
    let ready = continue_agent_workspace_repair_at_boundary(
        &state,
        attempt,
        AgentWorkspaceRepairPhase::Requested,
        "repair is ready while a previous external effect remains unresolved",
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    .expect("ready boundary should settle before the open effect is observed");
    let AgentWorkspaceRepairTransitionOutcome::Applied(ready) = ready else {
        panic!("open-effect fixture should retain the current durable generation");
    };
    assert_eq!(ready.phase, AgentWorkspaceRepairPhase::Ready);
    assert_repair_target_authority_is_cleared(&ready);

    let effect = AgentWorkspaceRepairEffect::new(
        ready.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "open-effect-blocks-ready-resume",
        chrono::Utc::now(),
    );
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: ready.id.clone(),
                generation: ready.generation,
                expected_phase: AgentWorkspaceRepairPhase::Ready,
                expected_attempt_updated_at: ready.updated_at,
                effect,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("open repair effect should persist"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    let error = resume_current_agent_workspace_repair_publish(
        &state,
        &ready.conversation_id,
        "manual publish must not overtake an open effect",
        true,
        PublishAuthority::UserExplicit,
    )
    .await
    .expect_err("an open effect must block Ready resume before target reacquisition");
    assert!(error.to_string().contains("effect"));
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&ready.conversation_id)
        .await
        .expect("open effect repair should reload")
        .expect("open effect repair should remain current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert_repair_target_authority_is_cleared(&current);
    assert!(
        state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&current.id)
            .await
            .expect("open effect should remain durable")
            .is_some(),
        "Ready resume must not replace or settle an external effect it does not own"
    );
    let lease = state
        .branch_update_repo
        .get_target_lease(&target_identity)
        .await
        .expect("released target lease should remain readable")
        .expect("released target lease should remain durable");
    assert!(lease.is_released());
    assert_eq!(lease.fencing_epoch(), released_epoch);
}

#[tokio::test]
async fn inactive_repair_lease_ready_resume_fails_closed_for_successor_target() {
    let temp = tempfile::tempdir().expect("busy ready boundary tempdir should be created");
    let state = AppState::new_test();
    let attempt = workspace_review_boundary_context(&state, temp.path(), false).await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await
        .expect("busy ready workspace should load")
        .expect("busy ready workspace should exist");
    workspace.auto_publish_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("busy ready boundary should persist disabled Auto Publish");
    let (attempt, target_identity, _) =
        checkpoint_workspace_review_boundary_lease(&state, attempt).await;
    let ready = continue_agent_workspace_repair_at_boundary(
        &state,
        attempt,
        AgentWorkspaceRepairPhase::Requested,
        "repair is ready for user-selected publication",
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    .expect("ready boundary should settle before the foreign writer arrives");
    let AgentWorkspaceRepairTransitionOutcome::Applied(ready) = ready else {
        panic!("ready boundary should retain the current durable repair generation");
    };
    assert_eq!(
        ready.phase,
        AgentWorkspaceRepairPhase::Ready,
        "disabled Auto Publish should park Ready: {ready:?}"
    );
    let foreign_owner = GitTargetLeaseOwner::branch_update("successor-task", "successor-update");
    assert!(matches!(
        state
            .branch_update_repo
            .acquire_target_lease(AcquireGitTargetLease {
                identity: target_identity.clone(),
                owner: foreign_owner.clone(),
            })
            .await
            .expect("successor should acquire the released canonical target"),
        AcquireGitTargetLeaseOutcome::Acquired { .. }
    ));

    let error = resume_current_agent_workspace_repair_publish(
        &state,
        &ready.conversation_id,
        "manual publish must not overtake a successor",
        true,
        PublishAuthority::UserExplicit,
    )
    .await
    .expect_err("a busy canonical target must block the stale Ready resume before effects");
    assert!(error.to_string().contains("owned") || error.to_string().contains("busy"));
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&ready.conversation_id)
        .await
        .expect("ready repair should reload")
        .expect("ready repair should remain current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert_repair_target_authority_is_cleared(&current);
    assert!(
        state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&current.id)
            .await
            .expect("busy resume effects should load")
            .is_none(),
        "a busy target must prevent push, PR, review, and effect creation"
    );
    let lease = state
        .branch_update_repo
        .get_target_lease(&target_identity)
        .await
        .expect("successor lease should load")
        .expect("successor lease should remain");
    assert_eq!(lease.owner(), &foreign_owner);
    assert!(!lease.is_released());
}

#[tokio::test]
async fn verified_base_advance_updates_one_active_generation_without_replacing_run_owner() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("repair-attempt-base-advance");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let started = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::BaseUpdate,
            AgentWorkspaceRepairContinuation::UpdateOnly,
            "base update",
        ),
    )
    .await
    .unwrap()
    .into_attempt();
    let run_id = AgentRunId::from_string("repair-owner-run");
    let bound = crate::domain::repositories::BindAgentWorkspaceRepairAttemptRun {
        attempt_id: started.id.clone(),
        generation: started.generation,
        expected_phase: AgentWorkspaceRepairPhase::Requested,
        expected_updated_at: started.updated_at,
        run_id: run_id.clone(),
        updated_at: chrono::Utc::now(),
    };
    repair_repo.bind_repair_attempt_run(bound).await.unwrap();

    let mut newer = repair_start_request(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "newer base",
    );
    newer.target_base_commit = Some("base-b".to_string());
    newer.verified_newer_base = true;
    let joined = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        newer,
    )
    .await
    .unwrap()
    .into_attempt();

    assert_eq!(joined.id, started.id);
    assert_eq!(joined.generation, started.generation);
    assert_eq!(joined.target_base_commit.as_deref(), Some("base-b"));
    assert_eq!(joined.reserved_agent_run_id, Some(run_id));
}

#[tokio::test]
async fn blocked_retry_coalesces_to_one_successor_generation_and_projects_requested_state() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-state-common"),
        "refs/heads/ralphx/repair-state",
    )
    .expect("valid canonical repair target identity");
    let conversation_id = ChatConversationId::from_string("repair-attempt-blocked-retry");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let started = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "publish failure",
        ),
    )
    .await
    .unwrap()
    .into_attempt();
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        target_identity.clone(),
        started,
        AgentRunId::from_string("repair-attempt-blocked-retry-run"),
        "dispatching repair",
        None,
    )
    .await
    .unwrap();
    let AgentWorkspaceRepairDispatchOutcome::Reserved(dispatch) = dispatch else {
        panic!("first repair generation should reserve a run");
    };
    settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        dispatch,
        AgentWorkspaceRepairDispatchSettlement::NonRetryableFailure,
        "repair dispatch failed",
        None,
    )
    .await
    .unwrap();

    assert!(
        branch_update_repo
            .get_target_lease(&target_identity)
            .await
            .expect("read dispatched repair lease")
            .expect("dispatch should acquire a lease")
            .is_released(),
        "a failed dispatch must release the exact durable repair lease"
    );

    let mut retry = repair_start_request(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "retry publish repair",
    );
    retry.retry_blocked = true;
    let left = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        retry.clone(),
    );
    let right = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        retry,
    );
    let (left, right) = tokio::join!(left, right);
    let outcomes = [left.unwrap(), right.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(
                outcome,
                AgentWorkspaceRepairStartOutcome::SuccessorStarted(_)
            ))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AgentWorkspaceRepairStartOutcome::Joined(_)))
            .count(),
        1
    );
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .unwrap()
        .expect("retry should leave one successor current");
    assert_eq!(current.generation, 2);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Requested);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("retry should retain its workspace projection");
    assert_eq!(
        workspace.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("fixing"));
}

#[tokio::test]
async fn retryable_dispatch_failure_persists_one_due_retry_and_blocks_not_due_replay() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-dispatch-due-retry");
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-dispatch-due-retry"),
        "refs/heads/ralphx/repair-state",
    )
    .expect("valid canonical repair target identity");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist repair workspace");
    let started = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "delivery failed",
        ),
    )
    .await
    .expect("start repair attempt")
    .into_attempt();
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        target_identity.clone(),
        started,
        AgentRunId::from_string("repair-dispatch-due-retry-first"),
        "dispatch repair",
        None,
    )
    .await
    .expect("reserve first repair delivery");
    let AgentWorkspaceRepairDispatchOutcome::Reserved(dispatch) = dispatch else {
        panic!("first repair delivery must reserve its run");
    };
    let before_schedule = chrono::Utc::now();
    let scheduled = settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        dispatch,
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        "repair delivery transport failed",
        None,
    )
    .await
    .expect("schedule retryable repair delivery");
    let AgentWorkspaceRepairTransitionOutcome::Applied(scheduled) = scheduled else {
        panic!("exact first delivery failure must schedule one retry");
    };
    assert_eq!(scheduled.phase, AgentWorkspaceRepairPhase::Requested);
    assert_eq!(scheduled.dispatch_count, 1);
    assert!(scheduled.reserved_agent_run_id.is_none());
    assert!(scheduled.next_dispatch_at.expect("retry due time") > before_schedule);
    assert!(
        !branch_update_repo
            .get_target_lease(&target_identity)
            .await
            .expect("load durable retry lease")
            .expect("retry keeps exact target lease")
            .is_released(),
        "a due retry must retain its original canonical repair authority"
    );
    let before_events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load retry events");
    let replay = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        target_identity,
        scheduled.clone(),
        AgentRunId::from_string("repair-dispatch-due-retry-replay"),
        "must not dispatch before due",
        None,
    )
    .await
    .expect("not-due replay is a harmless stale outcome");
    assert!(matches!(
        replay,
        AgentWorkspaceRepairDispatchOutcome::Stale(ref attempt)
            if attempt.id == scheduled.id && attempt.updated_at == scheduled.updated_at
    ));
    assert_eq!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("reload retry events"),
        before_events,
        "not-due replay must not append another repair delivery event"
    );
}

#[tokio::test]
async fn immediate_start_rejection_defers_recovery_redelivery_without_consuming_dispatch_retries() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let conversation_id =
        ChatConversationId::from_string("repair-dispatch-immediate-start-deferral");
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-dispatch-immediate-start-deferral"),
        "refs/heads/ralphx/repair-state",
    )
    .expect("valid canonical repair target identity");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist repair workspace");
    let mut current = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "conversation is busy",
        ),
    )
    .await
    .expect("start repair attempt")
    .into_attempt();

    for delivery in 0..=MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES {
        if current.next_dispatch_at.is_some() {
            let expected_updated_at = current.updated_at;
            current.next_dispatch_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
            current.updated_at += chrono::Duration::microseconds(1);
            let due = repair_repo
                .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                    attempt: current,
                    expected_phase: AgentWorkspaceRepairPhase::Requested,
                    expected_updated_at,
                    next_phase: AgentWorkspaceRepairPhase::Requested,
                    compatibility_projection: None,
                    events: Vec::new(),
                })
                .await
                .expect("make deferred recovery delivery due");
            let AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) = due else {
                panic!("due checkpoint must retain current attempt authority");
            };
            current = attempt;
        }
        let dispatch = reserve_agent_workspace_repair_dispatch(
            Arc::clone(&repair_repo),
            Arc::clone(&branch_update_repo),
            target_identity.clone(),
            current,
            AgentRunId::from_string(format!("repair-dispatch-immediate-start-{delivery}")),
            "dispatch busy repair",
            None,
        )
        .await
        .expect("reserve deferred delivery");
        let AgentWorkspaceRepairDispatchOutcome::Reserved(dispatch) = dispatch else {
            panic!("each due deferred delivery must reserve exactly one run");
        };
        let immediate_start_rejection =
            ChatServiceError::ImmediateStartRejected("another agent run is active".to_string());
        let settlement = classify_agent_workspace_repair_delivery(
            Err(&immediate_start_rejection),
            &conversation_id,
            dispatch
                .reserved_agent_run_id
                .as_ref()
                .expect("reserved delivery has its exact run identity"),
        );
        assert_eq!(
            settlement,
            AgentWorkspaceRepairDispatchSettlement::DeferredQueued,
            "every busy immediate-start rejection must stay outside retry exhaustion"
        );
        let settled = settle_agent_workspace_repair_dispatch_outcome(
            Arc::clone(&repair_repo),
            Arc::clone(&branch_update_repo),
            dispatch,
            settlement,
            "Workspace repair delivery is waiting for the conversation to become available.",
            None,
        )
        .await
        .expect("settle busy delivery through the same durable path as recovery");
        let AgentWorkspaceRepairTransitionOutcome::Applied(next) = settled else {
            panic!("current busy settlement must apply");
        };
        assert_eq!(next.phase, AgentWorkspaceRepairPhase::Requested);
        assert_eq!(
            next.dispatch_count, 0,
            "busy delivery is not a transport retry"
        );
        assert!(next.reserved_agent_run_id.is_none());
        assert!(
            next.next_dispatch_at.is_some(),
            "recovery must revisit the deferred attempt"
        );
        assert!(
            next.blocker.is_none(),
            "a busy conversation is not user-actionable"
        );
        current = next;
    }

    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load busy delivery events");
    assert!(
        events
            .iter()
            .all(|event| { event.step != REPAIR_SENT_STEP || event.status != "failed" }),
        "busy immediate-start rejections must not publish a failed repair_sent event"
    );
}

#[tokio::test]
async fn exhausted_or_nonretryable_dispatch_failure_blocks_once_and_releases_lease() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-dispatch-exhaustion");
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-dispatch-exhaustion"),
        "refs/heads/ralphx/repair-state",
    )
    .expect("valid canonical repair target identity");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist repair workspace");
    let mut current = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "delivery exhaustion",
        ),
    )
    .await
    .expect("start repair attempt")
    .into_attempt();

    for retry in 0..=MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES {
        if current.next_dispatch_at.is_some() {
            let expected_updated_at = current.updated_at;
            current.next_dispatch_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
            current.updated_at += chrono::Duration::microseconds(1);
            let due = repair_repo
                .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                    attempt: current,
                    expected_phase: AgentWorkspaceRepairPhase::Requested,
                    expected_updated_at,
                    next_phase: AgentWorkspaceRepairPhase::Requested,
                    compatibility_projection: None,
                    events: Vec::new(),
                })
                .await
                .expect("make the durable retry due");
            let AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) = due else {
                panic!("due checkpoint must preserve current retry authority");
            };
            current = attempt;
        }
        let dispatch = reserve_agent_workspace_repair_dispatch(
            Arc::clone(&repair_repo),
            Arc::clone(&branch_update_repo),
            target_identity.clone(),
            current,
            AgentRunId::from_string(format!("repair-dispatch-exhaustion-{retry}")),
            "dispatch repair",
            None,
        )
        .await
        .expect("reserve due retry delivery");
        let AgentWorkspaceRepairDispatchOutcome::Reserved(dispatch) = dispatch else {
            panic!("due retry must reserve exactly one run");
        };
        let spawn_failed = ChatServiceError::SpawnFailed("process start interrupted".to_string());
        let settlement = classify_agent_workspace_repair_delivery(
            Err(&spawn_failed),
            &conversation_id,
            dispatch
                .reserved_agent_run_id
                .as_ref()
                .expect("reserved retry has its exact run identity"),
        );
        assert_eq!(
            settlement,
            AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
            "a genuine spawn failure must retain bounded retry behavior"
        );
        let settled = settle_agent_workspace_repair_dispatch_outcome(
            Arc::clone(&repair_repo),
            Arc::clone(&branch_update_repo),
            dispatch,
            settlement,
            "delivery remained unavailable",
            None,
        )
        .await
        .expect("settle retry attempt");
        let AgentWorkspaceRepairTransitionOutcome::Applied(next) = settled else {
            panic!("exact retry settlement must apply");
        };
        current = next;
    }

    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert_eq!(
        current.dispatch_count, MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES,
        "exhaustion must not increment beyond the bounded budget"
    );
    assert!(current.next_dispatch_at.is_none());
    let events_before_replay = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load exhausted events");
    let duplicate = settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        current.clone(),
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        "duplicate exhausted retry",
        None,
    )
    .await;
    assert!(
        duplicate.is_err(),
        "released target lease rejects stale replay"
    );
    assert_eq!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("reload exhausted events"),
        events_before_replay,
        "exhaustion must record its blocker once"
    );
    assert!(
        branch_update_repo
            .get_target_lease(&target_identity)
            .await
            .expect("load exhausted lease")
            .expect("lease exists")
            .is_released(),
        "terminal dispatch exhaustion releases only the exact repair lease"
    );
}

#[tokio::test]
async fn foreign_canonical_target_owner_rejects_repair_dispatch_before_run_binding_or_events() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-dispatch-foreign-owner");
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-dispatch-foreign-owner"),
        "refs/heads/ralphx/repair-state",
    )
    .expect("valid canonical repair target identity");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("workspace should persist");
    let started = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "publish failure",
        ),
    )
    .await
    .expect("repair attempt should start")
    .into_attempt();
    let foreign_owner = GitTargetLeaseOwner::branch_update("foreign-task", "foreign-update");
    assert!(matches!(
        branch_update_repo
            .acquire_target_lease(AcquireGitTargetLease {
                identity: target_identity.clone(),
                owner: foreign_owner.clone(),
            })
            .await
            .expect("foreign lease acquisition"),
        AcquireGitTargetLeaseOutcome::Acquired { .. }
    ));

    let error = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        target_identity.clone(),
        started.clone(),
        AgentRunId::from_string("repair-dispatch-foreign-owner-run"),
        "dispatching repair",
        None,
    )
    .await
    .expect_err("a foreign canonical target owner must reject repair dispatch");
    assert!(error.to_string().contains("owned"));

    let current = repair_repo
        .get_repair_attempt(&started.id)
        .await
        .expect("repair attempt should load")
        .expect("started repair attempt should remain");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Requested);
    assert!(current.reserved_agent_run_id.is_none());
    assert!(current.target_lease_epoch.is_none());
    assert!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("publication events should load")
            .is_empty(),
        "foreign authority must stop repair dispatch before any repair-send event"
    );
    let lease = branch_update_repo
        .get_target_lease(&target_identity)
        .await
        .expect("foreign lease should load")
        .expect("foreign lease should remain");
    assert_eq!(lease.owner(), &foreign_owner);
    assert!(!lease.is_released());
}

#[tokio::test]
async fn exact_run_authority_distinguishes_current_stale_completed_and_blocked_attempts() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("repair-attempt-authority");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::PrAutofix,
            AgentWorkspaceRepairContinuation::ResumePrSupervision,
            "autofix",
        ),
    )
    .await
    .unwrap()
    .into_attempt();
    let owner_run = AgentRunId::new();
    repair_repo
        .bind_repair_attempt_run(
            crate::domain::repositories::BindAgentWorkspaceRepairAttemptRun {
                attempt_id: attempt.id.clone(),
                generation: attempt.generation,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at: attempt.updated_at,
                run_id: owner_run.clone(),
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .unwrap();

    assert!(matches!(
        classify_agent_workspace_repair_completion_authority(
            Arc::clone(&repair_repo),
            &conversation_id,
            &owner_run,
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairCompletionAuthority::Current(_)
    ));
    assert_eq!(
        classify_agent_workspace_repair_completion_authority(
            Arc::clone(&repair_repo),
            &conversation_id,
            &AgentRunId::new(),
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairCompletionAuthority::Superseded
    );

    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .unwrap()
        .expect("current bound attempt");
    let settled_at = current.updated_at + chrono::Duration::microseconds(1);
    assert!(matches!(
        repair_repo
            .settle_repair_attempt(SettleAgentWorkspaceRepairAttempt {
                attempt_id: current.id.clone(),
                generation: current.generation,
                expected_phase: current.phase,
                expected_updated_at: current.updated_at,
                outcome: AgentWorkspaceRepairOutcome::Succeeded,
                settled_at,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .unwrap(),
        SettleAgentWorkspaceRepairAttemptOutcome::Applied(_)
    ));
    assert_eq!(
        classify_agent_workspace_repair_completion_authority(
            Arc::clone(&repair_repo),
            &conversation_id,
            &owner_run,
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairCompletionAuthority::AlreadyCompleted
    );

    let successor = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "successor",
        ),
    )
    .await
    .unwrap()
    .into_attempt();
    let successor_run = AgentRunId::new();
    let successor = match repair_repo
        .bind_repair_attempt_run(
            crate::domain::repositories::BindAgentWorkspaceRepairAttemptRun {
                attempt_id: successor.id.clone(),
                generation: successor.generation,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                expected_updated_at: successor.updated_at,
                run_id: successor_run.clone(),
                updated_at: successor.updated_at + chrono::Duration::microseconds(1),
            },
        )
        .await
        .unwrap()
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected bound successor, got {outcome:?}"),
    };
    assert_eq!(
        classify_agent_workspace_repair_completion_authority(
            Arc::clone(&repair_repo),
            &conversation_id,
            &owner_run,
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairCompletionAuthority::Superseded
    );
    assert!(matches!(
        repair_repo
            .settle_repair_attempt(SettleAgentWorkspaceRepairAttempt {
                attempt_id: successor.id.clone(),
                generation: successor.generation,
                expected_phase: successor.phase,
                expected_updated_at: successor.updated_at,
                outcome: AgentWorkspaceRepairOutcome::Failed,
                settled_at: successor.updated_at + chrono::Duration::microseconds(1),
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .unwrap(),
        SettleAgentWorkspaceRepairAttemptOutcome::Applied(_)
    ));
    assert_eq!(
        classify_agent_workspace_repair_completion_authority(
            Arc::clone(&repair_repo),
            &conversation_id,
            &successor_run,
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairCompletionAuthority::Superseded
    );
    assert_eq!(
        classify_agent_workspace_repair_completion_authority(
            repair_repo,
            &ChatConversationId::from_string("repair-attempt-authority-missing"),
            &AgentRunId::new(),
        )
        .await
        .unwrap(),
        AgentWorkspaceRepairCompletionAuthority::Invalid
    );
}

#[test]
fn fresh_deferred_repair_is_not_failed_by_the_run_it_is_waiting_on() {
    let conversation_id = ChatConversationId::from_string("repair-deferred-lineage");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_updated_at = Some(
        chrono::Utc::now()
            - chrono::Duration::seconds(DEFERRED_REPAIR_WAIT_TIMEOUT_SECS as i64 + 2),
    );

    let mut terminal_run = AgentRun::new(conversation_id.clone());
    terminal_run.started_at = chrono::Utc::now() - chrono::Duration::seconds(10);
    terminal_run.completed_at = Some(chrono::Utc::now());
    let mut deferred_event = AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        "repair_deferred",
        "started",
        "Waiting for the active workspace agent turn to finish before sending repair",
        Some("agent_fixable".to_string()),
    );
    deferred_event.created_at = chrono::Utc::now() - chrono::Duration::seconds(1);

    assert!(!terminal_run_authorizes_repair_recovery(
        &workspace,
        &[deferred_event.clone()],
        &terminal_run,
    ));

    deferred_event.created_at = chrono::Utc::now()
        - chrono::Duration::seconds(DEFERRED_REPAIR_WAIT_TIMEOUT_SECS as i64 + 1);
    terminal_run.completed_at = Some(chrono::Utc::now());
    assert!(terminal_run_authorizes_repair_recovery(
        &workspace,
        &[deferred_event],
        &terminal_run,
    ));
}

#[tokio::test]
async fn claim_is_atomic_idempotent_and_stale_failure_cannot_downgrade_new_claim() {
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-claim-1");
    repo.create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let first = claim_agent_workspace_repair(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        "Repair requested.",
        None,
    )
    .await
    .unwrap()
    .expect("first claim");
    assert!(claim_agent_workspace_repair(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        "Duplicate repair.",
        None,
    )
    .await
    .unwrap()
    .is_none());
    assert!(settle_agent_workspace_repair_failure(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &first,
        "Dispatch failed.",
    )
    .await
    .unwrap());

    let second = claim_agent_workspace_repair(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        "Retry requested.",
        None,
    )
    .await
    .unwrap()
    .expect("second claim");
    assert!(!settle_agent_workspace_repair_failure(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &first,
        "Late first failure.",
    )
    .await
    .unwrap());
    let current = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(
        current.pr_supervision_updated_at,
        second.guard.pr_supervision_updated_at
    );
}

#[tokio::test]
async fn active_reconciliation_requires_current_successful_lifecycle_evidence() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-reconcile-1");
    let workspace = repair_workspace(conversation_id.clone());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let active_run = run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .unwrap();

    let mut old_event = AgentConversationWorkspacePublicationEvent::new(
        conversation_id.clone(),
        "repair_sent",
        "succeeded",
        "Old repair",
        Some("agent_fixable".to_string()),
    );
    old_event.created_at = active_run.started_at - chrono::Duration::seconds(1);
    assert!(!repair_event_authorizes_active_run(
        &[old_event.clone()],
        &active_run
    ));
    workspace_repo
        .append_publication_event(old_event)
        .await
        .unwrap();
    assert!(!reconcile_active_agent_workspace_repair(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&run_repo) as Arc<dyn AgentRunRepository>,
        &workspace,
    )
    .await
    .unwrap());

    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "repair_sent",
            "succeeded",
            "Current repair",
            Some("agent_fixable".to_string()),
        ))
        .await
        .unwrap();
    assert!(reconcile_active_agent_workspace_repair(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&run_repo) as Arc<dyn AgentRunRepository>,
        &workspace,
    )
    .await
    .unwrap());
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .pr_supervision_status
            .as_deref(),
        Some("fixing")
    );
}

#[tokio::test]
async fn stale_completion_claim_cannot_overwrite_a_failed_attempt() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-completion-1");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .unwrap();
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "repair_sent",
            "succeeded",
            "Current repair",
            Some("agent_fixable".to_string()),
        ))
        .await
        .unwrap();
    let claim = current_agent_workspace_repair_claim_for_completion(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&run_repo) as Arc<dyn AgentRunRepository>,
        &workspace,
    )
    .await
    .unwrap()
    .expect("current completion claim");
    assert!(settle_agent_workspace_repair_failure(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &claim,
        "Dispatch failed",
    )
    .await
    .unwrap());
    assert!(!complete_agent_workspace_repair_claim(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &claim,
        "new-base",
        Some("monitoring"),
        Some("Repair completed"),
    )
    .await
    .unwrap());

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
    assert_eq!(current.base_commit.as_deref(), Some("base"));
}

#[tokio::test]
async fn completion_requires_dispatch_evidence_for_the_current_claim() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-completion-current-claim");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();
    run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .unwrap();
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "repair_sent",
            "succeeded",
            "Older repair dispatch",
            Some("agent_fixable".to_string()),
        ))
        .await
        .unwrap();

    let claim = claim_agent_workspace_repair(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        "New repair claim",
        None,
    )
    .await
    .unwrap()
    .expect("new claim");
    let claimed_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();

    assert!(current_agent_workspace_repair_claim_for_completion(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&run_repo) as Arc<dyn AgentRunRepository>,
        &claimed_workspace,
    )
    .await
    .unwrap()
    .is_none());

    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_sent",
            "succeeded",
            "Current repair dispatch",
            Some("agent_fixable".to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(
        current_agent_workspace_repair_claim_for_completion(
            workspace_repo as Arc<dyn AgentConversationWorkspaceRepository>,
            run_repo as Arc<dyn AgentRunRepository>,
            &claimed_workspace,
        )
        .await
        .unwrap()
        .expect("current dispatch authorizes completion"),
        claim
    );
}

#[tokio::test]
async fn pr_fix_completion_and_review_handoff_are_exact_once_for_current_claim() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("pr-fix-completion-current");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let claim =
        crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairClaim {
            conversation_id: conversation_id.clone(),
            guard: crate::domain::repositories::AgentWorkspaceRepairStateGuard::from_workspace(
                &workspace,
            ),
        };

    let review_claim = complete_agent_workspace_pr_fix_claim(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &claim,
        "Resolved conflicts",
        true,
        true,
    )
    .await
    .unwrap()
    .expect("current claim accepted");
    assert!(complete_agent_workspace_pr_fix_claim(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &claim,
        "Duplicate completion",
        true,
        true,
    )
    .await
    .unwrap()
    .is_none());
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "pr_autofix_completed")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "pr_autofix_workspace_review")
            .count(),
        1
    );

    abort_agent_workspace_pr_fix_review_handoff(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &review_claim,
        "Review could not start",
    )
    .await
    .unwrap()
    .expect("handoff abort accepted");
    assert!(abort_agent_workspace_pr_fix_review_handoff(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &review_claim,
        "Duplicate abort",
    )
    .await
    .unwrap()
    .is_none());
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("blocked"));
    assert_eq!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap()
            .iter()
            .filter(|event| event.step == "pr_autofix_workspace_review_aborted")
            .count(),
        1
    );
}

#[tokio::test]
async fn pr_fix_blocker_requires_current_claim_but_not_a_commit_sha() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("pr-fix-blocker-current");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let claim =
        crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairClaim {
            conversation_id: conversation_id.clone(),
            guard: crate::domain::repositories::AgentWorkspaceRepairStateGuard::from_workspace(
                &workspace,
            ),
        };

    block_agent_workspace_pr_fix_claim(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &claim,
        "Maintainer decision required",
    )
    .await
    .unwrap()
    .expect("current blocker accepted");
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .pr_supervision_status
            .as_deref(),
        Some("blocked")
    );
    assert_eq!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap()
            .iter()
            .filter(|event| event.step == "pr_autofix_blocked")
            .count(),
        1
    );
}

#[tokio::test]
async fn block_needs_human_persists_marker_and_blocks_repair() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let conversation_id = ChatConversationId::from_string("needs-human-repair");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "needs human",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();

    let result = block_agent_workspace_repair_needs_human(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        attempt,
        "CI failure requires human intervention",
        None,
    )
    .await
    .expect("block as needs-human");

    let AgentWorkspaceRepairTransitionOutcome::Applied(blocked) = result else {
        panic!("needs-human block must apply");
    };
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(
        blocked
            .pending_reasons
            .contains(&NEEDS_HUMAN_REPAIR_REASON.to_string()),
        "the needs-human marker must be persisted in pending_reasons"
    );
    assert!(
        blocked.blocker.is_some(),
        "a blocked attempt must carry a blocker message"
    );
}

#[tokio::test]
async fn block_needs_human_is_idempotent_on_pending_reasons() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let branch_update_repo: Arc<dyn BranchUpdateRepository> =
        Arc::new(MemoryBranchUpdateRepository::new());
    let conversation_id = ChatConversationId::from_string("needs-human-idempotent");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let mut attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "needs human idempotent",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    attempt
        .pending_reasons
        .push(NEEDS_HUMAN_REPAIR_REASON.to_string());

    let result = block_agent_workspace_repair_needs_human(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        attempt,
        "already marked",
        None,
    )
    .await
    .expect("block as needs-human again");

    let AgentWorkspaceRepairTransitionOutcome::Applied(blocked) = result else {
        panic!("idempotent needs-human block must apply");
    };
    assert_eq!(
        blocked
            .pending_reasons
            .iter()
            .filter(|r| *r == NEEDS_HUMAN_REPAIR_REASON)
            .count(),
        1,
        "the marker must not be duplicated"
    );
}

#[tokio::test]
async fn reserve_pre_existing_on_base_settles_to_ready_with_marker() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("pre-existing-on-base");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let mut attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "pre-existing failure",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    attempt.pr_autofix_health_fingerprint = Some("fp-abc123".to_string());

    let result = reserve_agent_workspace_pre_existing_on_base(
        Arc::clone(&repair_repo),
        attempt,
        "codecov failure pre-exists on base",
        None,
    )
    .await
    .expect("reserve pre-existing on base");

    let AgentWorkspaceRepairTransitionOutcome::Applied(settled) = result else {
        panic!("pre-existing-on-base reservation must apply");
    };
    assert_eq!(settled.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(
        settled
            .pending_reasons
            .contains(&PRE_EXISTING_ON_BASE_REPAIR_REASON.to_string()),
        "the pre-existing-on-base marker must be persisted"
    );
    assert!(
        settled.blocker.is_none(),
        "Ready phase must not carry a blocker"
    );
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace projection lookup")
            .expect("workspace exists")
            .pr_supervision_status
            .as_deref(),
        Some("held")
    );
}

#[tokio::test]
async fn held_pr_autofix_retry_starts_one_fenced_successor_with_carryover_evidence() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("held-pr-autofix-retry");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.auto_publish_enabled = true;
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");

    let mut attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::PrAutofix,
            AgentWorkspaceRepairContinuation::ResumePrSupervision,
            "held PR autofix",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    attempt.pr_autofix_dispatch_head_commit = Some("held-head".to_string());
    attempt.pr_autofix_health_fingerprint = Some("checks:held-fingerprint".to_string());
    let held = match reserve_agent_workspace_pre_existing_on_base(
        Arc::clone(&repair_repo),
        attempt,
        "failure already exists on base",
        Some(false),
    )
    .await
    .expect("reserve held repair")
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("held transition must apply, got {outcome:?}"),
    };

    let outcome = retry_agent_workspace_pr_autofix_hold_override(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        &held.id,
        held.generation,
        held.updated_at,
    )
    .await
    .expect("retry held repair");
    let AgentWorkspacePrAutofixHoldActionOutcome::Applied(successor) = outcome else {
        panic!("exact retry must start a successor");
    };
    assert_eq!(successor.generation, held.generation + 1);
    assert_eq!(successor.phase, AgentWorkspaceRepairPhase::Requested);
    assert!(!successor.explicit_publish_requested);
    assert_eq!(
        successor.pr_autofix_dispatch_head_commit.as_deref(),
        Some("held-head")
    );
    assert_eq!(
        successor.pr_autofix_health_fingerprint.as_deref(),
        Some("checks:held-fingerprint")
    );
    assert!(successor.operation_snapshot().hold_reason.is_none());

    let replay = retry_agent_workspace_pr_autofix_hold_override(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        &held.id,
        held.generation,
        held.updated_at,
    )
    .await
    .expect("replayed retry should be rejected without side effects");
    assert!(matches!(
        replay,
        AgentWorkspacePrAutofixHoldActionOutcome::Stale(_)
    ));
    assert_eq!(
        repair_repo
            .list_repair_attempts_for_conversation(&conversation_id)
            .await
            .expect("list repair generations")
            .len(),
        2,
        "a replayed retry must not spend another generation"
    );
}

#[tokio::test]
async fn held_pr_autofix_stop_is_exact_and_disables_automation() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("held-pr-autofix-stop");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.auto_publish_enabled = true;
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");

    let mut attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::PrAutofix,
            AgentWorkspaceRepairContinuation::ResumePrSupervision,
            "held PR autofix",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    attempt.pr_autofix_health_fingerprint = Some("checks:stop-fingerprint".to_string());
    let held = match reserve_agent_workspace_pre_existing_on_base(
        Arc::clone(&repair_repo),
        attempt,
        "failure already exists on base",
        Some(true),
    )
    .await
    .expect("reserve held repair")
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("held transition must apply, got {outcome:?}"),
    };

    let stale = stop_agent_workspace_pr_autofix_for_hold(
        Arc::clone(&repair_repo),
        &conversation_id,
        &held.id,
        held.generation,
        held.updated_at + chrono::Duration::microseconds(1),
    )
    .await
    .expect("stale stop should be rejected");
    assert!(matches!(
        stale,
        AgentWorkspacePrAutofixHoldActionOutcome::Stale(_)
    ));
    let before = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace exists");
    assert!(before.pr_autofix_enabled);
    assert!(before.pr_auto_merge_desired);
    assert_eq!(before.pr_auto_merge_current, Some(true));

    let outcome = stop_agent_workspace_pr_autofix_for_hold(
        Arc::clone(&repair_repo),
        &conversation_id,
        &held.id,
        held.generation,
        held.updated_at,
    )
    .await
    .expect("stop held repair");
    assert!(matches!(
        outcome,
        AgentWorkspacePrAutofixHoldActionOutcome::Applied(_)
    ));
    let stopped = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace exists");
    assert!(!stopped.pr_autofix_enabled);
    assert!(!stopped.pr_auto_merge_desired);
    assert_eq!(stopped.pr_auto_merge_current, Some(false));
    assert!(repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("current repair lookup")
        .is_none());
}

#[tokio::test]
async fn base_stale_hold_uses_base_tip_authority_and_preserves_ci_hold_on_release() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("base-stale-hold");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let mut attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id,
            AgentWorkspaceRepairSource::PrAutofix,
            AgentWorkspaceRepairContinuation::ResumePrSupervision,
            "PR is behind base",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    attempt.ci_rerun_count = 1;
    attempt.ci_rerun_fingerprint = Some("ci-hold:v1:repair-head:901".to_string());
    attempt.pending_reasons.push(
        crate::application::agent_workspace_publish_repair_state::AWAITING_CI_REPAIR_REASON
            .to_string(),
    );
    assert!(attempt.pr_autofix_health_fingerprint.is_none());
    let stale_pre_reservation = attempt.clone();

    let AgentWorkspaceRepairTransitionOutcome::Applied(reserved) =
        reserve_agent_workspace_base_update(
            Arc::clone(&repair_repo),
            attempt,
            "observed-base-tip",
            "reserve base refresh",
            None,
        )
        .await
        .expect("reserve base stale hold")
    else {
        panic!("current attempt must CAS-reserve the base tip");
    };
    assert_eq!(reserved.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(reserved.base_update_target_commit, None);
    assert!(!reserved.pending_reasons.contains(
        &crate::application::agent_workspace_publish_repair_state::BASE_STALE_AFTER_UPDATE_REPAIR_REASON
            .to_string()
    ));

    assert!(matches!(
        mark_agent_workspace_base_update_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>,
            stale_pre_reservation,
            "observed-base-tip",
            "stale marker must not apply",
            None,
        )
        .await
        .expect("stale marker transition is harmless"),
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
    ));
    assert_eq!(
        repair_repo
            .get_current_repair_attempt(&reserved.conversation_id)
            .await
            .expect("load reserved attempt")
            .expect("reserved attempt exists")
            .base_update_target_commit,
        None,
        "a stale pre-reservation snapshot cannot claim the update ran"
    );

    let AgentWorkspaceRepairTransitionOutcome::Applied(marked) =
        mark_agent_workspace_base_update_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>,
            reserved,
            "observed-base-tip",
            "base update route completed",
            None,
        )
        .await
        .expect("mark completed base update")
    else {
        panic!("the reserved attempt must accept its completed update marker");
    };
    assert_eq!(
        marked.base_update_target_commit.as_deref(),
        Some("observed-base-tip")
    );

    for invalid_observed_tip in ["", "different-base-tip"] {
        assert!(matches!(
            reserve_agent_workspace_base_stale_hold(
                Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>,
                marked.clone(),
                invalid_observed_tip,
                "invalid base authority must not hold",
                None,
            )
            .await
            .expect("invalid base authority returns stale"),
            AgentWorkspaceRepairTransitionOutcome::Stale(_)
        ));
    }

    let AgentWorkspaceRepairTransitionOutcome::Applied(held) =
        reserve_agent_workspace_base_stale_hold(
            Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>,
            marked,
            "observed-base-tip",
            "base update did not take",
            None,
        )
        .await
        .expect("reserve base-stale hold")
    else {
        panic!("current reserved attempt must accept the base-stale hold");
    };
    assert!(held.pending_reasons.contains(
        &crate::application::agent_workspace_publish_repair_state::BASE_STALE_AFTER_UPDATE_REPAIR_REASON
            .to_string()
    ));
    assert!(agent_workspace_repair_is_base_stale_held(&held));
    assert!(agent_workspace_repair_is_ci_held(&held));

    let AgentWorkspaceRepairTransitionOutcome::Applied(released) =
        release_agent_workspace_base_stale_hold(
            Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>,
            held,
            "base stale condition cleared",
            None,
        )
        .await
        .expect("release base-stale marker")
    else {
        panic!("current base-stale marker release must apply");
    };
    assert!(!agent_workspace_repair_is_base_stale_held(&released));
    assert!(
        agent_workspace_repair_is_ci_held(&released),
        "releasing base_stale must preserve the underlying CI hold"
    );
    assert!(released.pending_reasons.iter().any(|reason| {
        reason
            == crate::application::agent_workspace_publish_repair_state::AWAITING_CI_REPAIR_REASON
    }));
}

#[tokio::test]
async fn reserve_pre_existing_on_base_rejects_without_health_fingerprint() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("pre-existing-no-fp");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "no fingerprint",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    assert!(attempt.pr_autofix_health_fingerprint.is_none());

    let result = reserve_agent_workspace_pre_existing_on_base(
        Arc::clone(&repair_repo),
        attempt,
        "should reject",
        None,
    )
    .await
    .expect("returns stale, not error");

    assert!(
        matches!(result, AgentWorkspaceRepairTransitionOutcome::Stale(_)),
        "missing health fingerprint must return Stale"
    );
}

#[tokio::test]
async fn reserve_ci_rerun_increments_count_and_settles_to_ready() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("ci-rerun-reserve");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "ci rerun",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    assert_eq!(attempt.ci_rerun_count, 0);

    let result = reserve_agent_workspace_ci_rerun(
        Arc::clone(&repair_repo),
        attempt,
        "fp-ci-abc",
        "rerunning failed CI jobs",
        None,
    )
    .await
    .expect("reserve ci rerun");

    let AgentWorkspaceRepairTransitionOutcome::Applied(rerun) = result else {
        panic!("first ci rerun reservation must apply");
    };
    assert_eq!(rerun.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(rerun.ci_rerun_count, 1);
    assert_eq!(rerun.ci_rerun_fingerprint.as_deref(), Some("fp-ci-abc"));
    assert!(rerun.blocker.is_none());
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace projection lookup")
            .expect("workspace exists")
            .pr_supervision_status
            .as_deref(),
        Some("held")
    );
}

#[tokio::test]
async fn reserve_ci_rerun_rejects_after_max_retries() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("ci-rerun-exhausted");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let mut attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "ci rerun exhausted",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    attempt.ci_rerun_count = MAX_AGENT_WORKSPACE_CI_RERUN_RETRIES;

    let result = reserve_agent_workspace_ci_rerun(
        Arc::clone(&repair_repo),
        attempt,
        "fp-ci-exhausted",
        "should reject",
        None,
    )
    .await
    .expect("returns stale, not error");

    assert!(
        matches!(result, AgentWorkspaceRepairTransitionOutcome::Stale(_)),
        "exhausted ci rerun budget must return Stale"
    );
}

#[tokio::test]
async fn reserve_ci_await_parks_the_current_attempt_without_spending_rerun_budget() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("ci-await-reserve");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id,
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "await CI completion",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();

    let result = reserve_agent_workspace_ci_await(
        Arc::clone(&repair_repo),
        attempt,
        "ci-hold:v1:head:17",
        "waiting for the in-progress workflow run",
        None,
    )
    .await
    .expect("reserve CI await");

    let AgentWorkspaceRepairTransitionOutcome::Applied(awaiting_ci) = result else {
        panic!("current CI await reservation must apply");
    };
    assert_eq!(awaiting_ci.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(
        awaiting_ci.ci_rerun_count, 0,
        "awaiting must not spend rerun budget"
    );
    assert_eq!(
        awaiting_ci.ci_rerun_fingerprint.as_deref(),
        Some("ci-hold:v1:head:17")
    );
    assert!(awaiting_ci
        .pending_reasons
        .iter()
        .any(|reason| reason == AWAITING_CI_REPAIR_REASON));
    assert!(agent_workspace_repair_is_ci_held(&awaiting_ci));
    assert!(
        !agent_workspace_repair_is_health_held(&awaiting_ci),
        "a CI await must not enter the classification-equality health hold"
    );
    assert_eq!(
        awaiting_ci.summary.as_deref(),
        Some("waiting for the in-progress workflow run")
    );
    assert!(awaiting_ci.blocker.is_none());
}

#[tokio::test]
async fn reserve_ci_await_rejects_a_stale_attempt_without_overwriting_the_current_hold() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("ci-await-stale");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id,
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "await CI completion",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();

    let first = reserve_agent_workspace_ci_await(
        Arc::clone(&repair_repo),
        attempt.clone(),
        "ci-hold:v1:head:18",
        "first await reservation",
        None,
    )
    .await
    .expect("first reservation");
    assert!(matches!(
        first,
        AgentWorkspaceRepairTransitionOutcome::Applied(_)
    ));

    let stale = reserve_agent_workspace_ci_await(
        Arc::clone(&repair_repo),
        attempt,
        "ci-hold:v1:head:19",
        "stale await reservation",
        None,
    )
    .await
    .expect("stale reservation returns an outcome");
    assert!(matches!(
        stale,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
    ));
    let current = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("current repair attempt should load")
        .expect("current repair attempt should remain durable");
    assert_eq!(
        current.ci_rerun_fingerprint.as_deref(),
        Some("ci-hold:v1:head:18"),
        "a stale transition must not overwrite the current CI hold identity"
    );
}

#[tokio::test]
async fn reserve_ci_await_is_idempotent_for_the_await_reason_and_budget() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let repair_repo = Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    let conversation_id = ChatConversationId::from_string("ci-await-idempotent");
    workspace_repo
        .create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let attempt = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        repair_start_request(
            conversation_id,
            AgentWorkspaceRepairSource::Publish,
            AgentWorkspaceRepairContinuation::Publish,
            "await CI completion",
        ),
    )
    .await
    .expect("start repair")
    .into_attempt();
    let AgentWorkspaceRepairTransitionOutcome::Applied(first) = reserve_agent_workspace_ci_await(
        Arc::clone(&repair_repo),
        attempt,
        "ci-hold:v1:head:20",
        "first await reservation",
        None,
    )
    .await
    .expect("first reservation") else {
        panic!("first CI await reservation must apply");
    };

    let AgentWorkspaceRepairTransitionOutcome::Applied(replayed) =
        reserve_agent_workspace_ci_await(
            Arc::clone(&repair_repo),
            first,
            "ci-hold:v1:head:20",
            "replayed await reservation",
            None,
        )
        .await
        .expect("replayed reservation")
    else {
        panic!("replayed CI await reservation must apply");
    };

    assert_eq!(replayed.ci_rerun_count, 0);
    assert_eq!(
        replayed
            .pending_reasons
            .iter()
            .filter(|reason| reason.as_str() == AWAITING_CI_REPAIR_REASON)
            .count(),
        1,
        "a replay must not duplicate the durable await marker"
    );
}
