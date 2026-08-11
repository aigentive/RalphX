use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::application::agent_workspace_review::{
    AgentWorkspaceReviewPacket, AgentWorkspaceReviewTarget,
};
use crate::application::agent_workspace_review_auto_merge::REVIEW_AUTO_MERGE_PAUSED_SUMMARY;
use crate::application::agent_workspace_review_publish_handoff::{
    has_open_pr_fix_workspace_review_publish_handoff,
    has_pending_pr_fix_workspace_review_publish_handoff,
    pr_fix_publish_can_resume_after_workspace_review,
    pr_supervision_block_is_workspace_review_gate,
    resume_pr_fix_publish_after_passed_workspace_review, workspace_review_authorization_kind,
    workspace_review_monitor_keeps_pr_fix_publish_handoff, PrFixReviewPublishResumeOutcome,
    WorkspaceReviewAuthorizationKind,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind,
    ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository;

fn conversation_id() -> ChatConversationId {
    ChatConversationId::from_string("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
}

fn project_id() -> ProjectId {
    ProjectId::from_string("project-review-publish-handoff".to_string())
}

fn publication_event(
    step: &'static str,
    status: &'static str,
    classification: Option<&'static str>,
) -> AgentConversationWorkspacePublicationEvent {
    AgentConversationWorkspacePublicationEvent::new(
        conversation_id(),
        step,
        status,
        "event summary",
        classification.map(str::to_string),
    )
}

fn review_target() -> AgentWorkspaceReviewTarget {
    AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        base_ref: "main".to_string(),
        base_sha: Some("base-sha".to_string()),
        head_ref: "ralphx/test/review-publish-handoff".to_string(),
        head_sha: Some("head-current".to_string()),
        diff_fingerprint: "diff-current".to_string(),
        working_directory: PathBuf::from("/tmp/ralphx-test-review-publish-handoff"),
        source_pull_request_number: None,
        review_packet: AgentWorkspaceReviewPacket::default(),
    }
}

fn selected_source_review_target() -> AgentWorkspaceReviewTarget {
    AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        base_ref: "main".to_string(),
        base_sha: Some("base-sha".to_string()),
        head_ref: "feature".to_string(),
        head_sha: Some("selected-head".to_string()),
        diff_fingerprint: "selected-diff".to_string(),
        working_directory: PathBuf::from("/tmp/ralphx-test-review-publish-handoff"),
        source_pull_request_number: Some(684),
        review_packet: AgentWorkspaceReviewPacket::default(),
    }
}

fn current_reviewing_monitor(target: &AgentWorkspaceReviewTarget) -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id(), project_id());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor
}

fn current_passed_monitor(target: &AgentWorkspaceReviewTarget) -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id(), project_id());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-current"));
    monitor.review_artifact_version = Some(2);
    monitor.review_requested_changes_artifact_id = Some(ArtifactId::from_string(
        "requested-changes-artifact-current",
    ));
    monitor.review_requested_changes_artifact_version = Some(2);
    monitor.reviewed_target_scope = Some(target.scope);
    monitor.reviewed_head_sha = target.head_sha.clone();
    monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor
}

fn current_bypassed_monitor(target: &AgentWorkspaceReviewTarget) -> AgentWorkspaceReviewMonitor {
    let mut monitor = current_passed_monitor(target);
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_blocking_summary = Some("The reviewer blocker remains.".to_string());
    monitor.review_gate_bypassed_at = Some(chrono::Utc::now());
    monitor.review_gate_bypassed_target_scope = Some(target.scope);
    monitor.review_gate_bypassed_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.review_gate_bypassed_artifact_id = monitor.review_artifact_id.clone();
    monitor.review_gate_bypassed_artifact_version = monitor.review_artifact_version;
    monitor
}

fn pr_fix_workspace() -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id(),
        project_id(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/review-publish-handoff".to_string(),
        "/tmp/ralphx-test-review-publish-handoff".to_string(),
    );
    workspace.publication_pr_number = Some(684);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/684".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("failed".to_string());
    workspace.auto_publish_enabled = true;
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary =
        Some("Recovered stale PR autofix state; no active fixer run is running.".to_string());
    workspace
}

#[test]
fn pending_handoff_state_closes_on_review_pass_publish_failure_or_publish_success() {
    assert!(has_pending_pr_fix_workspace_review_publish_handoff(&[
        publication_event(
            "pr_autofix_workspace_review",
            "pending",
            Some("workspace_review_pending"),
        ),
    ]));
    assert!(has_pending_pr_fix_workspace_review_publish_handoff(&[
        publication_event(
            "pr_autofix_workspace_review",
            "reviewing",
            Some("workspace_review_started"),
        ),
    ]));

    assert!(!has_pending_pr_fix_workspace_review_publish_handoff(&[
        publication_event(
            "pr_autofix_workspace_review",
            "reviewing",
            Some("workspace_review_started"),
        ),
        publication_event(
            "pr_autofix_workspace_review_passed",
            "publishing",
            Some("workspace_review_passed"),
        ),
    ]));

    assert!(!has_pending_pr_fix_workspace_review_publish_handoff(&[
        publication_event(
            "pr_autofix_workspace_review",
            "reviewing",
            Some("workspace_reviewing"),
        ),
        publication_event(
            "pr_autofix_publish_failed",
            "failed",
            Some("pr_autofix_publish_failed"),
        ),
    ]));

    assert!(!has_pending_pr_fix_workspace_review_publish_handoff(&[
        publication_event(
            "pr_autofix_workspace_review",
            "reviewing",
            Some("workspace_review_started"),
        ),
        publication_event("published", "succeeded", Some("published:684")),
    ]));

    assert!(!has_pending_pr_fix_workspace_review_publish_handoff(&[
        publication_event(
            "pr_autofix_workspace_review",
            "reviewing",
            Some("workspace_review_started"),
        ),
        publication_event("failed", "failed", Some("manual_publish_failed")),
    ]));

    assert!(!has_pending_pr_fix_workspace_review_publish_handoff(&[
        publication_event(
            "pr_autofix_workspace_review",
            "pending",
            Some("workspace_review_pending"),
        ),
        publication_event(
            "pr_autofix_workspace_review_aborted",
            "failed",
            Some("workspace_review_aborted"),
        ),
    ]));
}

#[test]
fn handoff_is_open_only_for_current_reviewing_or_current_passed_monitor() {
    let target = review_target();
    let events = [publication_event(
        "pr_autofix_workspace_review",
        "reviewing",
        Some("workspace_review_started"),
    )];
    let reviewing_monitor = current_reviewing_monitor(&target);
    assert!(workspace_review_monitor_keeps_pr_fix_publish_handoff(
        Some(&reviewing_monitor),
        Some(&target),
    ));
    assert!(has_open_pr_fix_workspace_review_publish_handoff(
        &events,
        Some(&reviewing_monitor),
        Some(&target),
    ));

    let passed_monitor = current_passed_monitor(&target);
    assert!(workspace_review_monitor_keeps_pr_fix_publish_handoff(
        Some(&passed_monitor),
        Some(&target),
    ));

    let bypassed_monitor = current_bypassed_monitor(&target);
    assert!(workspace_review_monitor_keeps_pr_fix_publish_handoff(
        Some(&bypassed_monitor),
        Some(&target),
    ));
    assert!(has_open_pr_fix_workspace_review_publish_handoff(
        &events,
        Some(&bypassed_monitor),
        Some(&target),
    ));

    let mut stale_passed_monitor = current_passed_monitor(&target);
    stale_passed_monitor.current_diff_fingerprint = Some("old-diff".to_string());
    assert!(!workspace_review_monitor_keeps_pr_fix_publish_handoff(
        Some(&stale_passed_monitor),
        Some(&target),
    ));

    let mut blocking_monitor = current_reviewing_monitor(&target);
    blocking_monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    blocking_monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    assert!(!workspace_review_monitor_keeps_pr_fix_publish_handoff(
        Some(&blocking_monitor),
        Some(&target),
    ));

    let selected_target = selected_source_review_target();
    let mut selected_monitor = current_reviewing_monitor(&selected_target);
    selected_monitor.workspace_head_sha = None;
    selected_monitor.selected_source_head_sha = selected_target.head_sha.clone();
    assert!(workspace_review_monitor_keeps_pr_fix_publish_handoff(
        Some(&selected_monitor),
        Some(&selected_target),
    ));
}

#[test]
fn resume_predicate_requires_current_passed_review_and_publishable_pr_fix_state() {
    let target = review_target();
    let monitor = current_passed_monitor(&target);
    let events = [publication_event(
        "pr_autofix_workspace_review",
        "reviewing",
        Some("workspace_review_started"),
    )];
    let workspace = pr_fix_workspace();

    assert!(pr_fix_publish_can_resume_after_workspace_review(
        &workspace,
        &monitor,
        Some(&target),
        &events,
    ));
    assert!(!pr_fix_publish_can_resume_after_workspace_review(
        &workspace, &monitor, None, &events,
    ));
    let mut plan_workspace = workspace.clone();
    plan_workspace.mode = AgentConversationWorkspaceMode::Plan;
    assert!(!pr_fix_publish_can_resume_after_workspace_review(
        &plan_workspace,
        &monitor,
        Some(&target),
        &events,
    ));
    assert!(pr_supervision_block_is_workspace_review_gate(&{
        let mut gated = workspace.clone();
        gated.pr_supervision_summary = Some("Workspace Review is still running".to_string());
        gated
    }));
    assert!(!pr_supervision_block_is_workspace_review_gate(&{
        let mut ungated = workspace.clone();
        ungated.pr_supervision_summary = None;
        ungated
    }));

    let mut desired_auto_merge = workspace.clone();
    desired_auto_merge.pr_autofix_enabled = false;
    desired_auto_merge.pr_auto_merge_desired = true;
    desired_auto_merge.pr_auto_merge_current = None;
    assert!(pr_fix_publish_can_resume_after_workspace_review(
        &desired_auto_merge,
        &monitor,
        Some(&target),
        &events,
    ));

    let mut current_auto_merge = workspace.clone();
    current_auto_merge.pr_autofix_enabled = false;
    current_auto_merge.pr_auto_merge_desired = false;
    current_auto_merge.pr_auto_merge_current = Some(true);
    assert!(pr_fix_publish_can_resume_after_workspace_review(
        &current_auto_merge,
        &monitor,
        Some(&target),
        &events,
    ));

    let mut no_pr_fix_policy = workspace.clone();
    no_pr_fix_policy.pr_autofix_enabled = false;
    no_pr_fix_policy.pr_auto_merge_desired = false;
    no_pr_fix_policy.pr_auto_merge_current = None;
    assert!(!pr_fix_publish_can_resume_after_workspace_review(
        &no_pr_fix_policy,
        &monitor,
        Some(&target),
        &events,
    ));

    let mut disabled = workspace.clone();
    disabled.auto_publish_enabled = false;
    assert!(!pr_fix_publish_can_resume_after_workspace_review(
        &disabled,
        &monitor,
        Some(&target),
        &events,
    ));

    let mut terminal = workspace.clone();
    terminal.publication_pr_status = Some("merged".to_string());
    assert!(!pr_fix_publish_can_resume_after_workspace_review(
        &terminal,
        &monitor,
        Some(&target),
        &events,
    ));

    let mut unrelated_block = workspace;
    unrelated_block.pr_supervision_summary = Some("GitHub check failed".to_string());
    assert!(!pr_fix_publish_can_resume_after_workspace_review(
        &unrelated_block,
        &monitor,
        Some(&target),
        &[],
    ));

    let mut unrelated_status = unrelated_block;
    unrelated_status.pr_supervision_status = Some("monitoring".to_string());
    assert!(!pr_fix_publish_can_resume_after_workspace_review(
        &unrelated_status,
        &monitor,
        Some(&target),
        &events,
    ));
}

#[test]
fn resume_accepts_review_paused_with_review_guard_summary() {
    let target = review_target();
    let monitor = current_passed_monitor(&target);
    let mut workspace = pr_fix_workspace();
    workspace.pr_supervision_status = Some("review_paused".to_string());
    workspace.pr_supervision_summary = Some(REVIEW_AUTO_MERGE_PAUSED_SUMMARY.to_string());

    assert!(pr_fix_publish_can_resume_after_workspace_review(
        &workspace,
        &monitor,
        Some(&target),
        &[],
    ));
}

#[test]
fn resume_rejects_review_paused_with_other_summary() {
    let target = review_target();
    let monitor = current_passed_monitor(&target);
    let mut workspace = pr_fix_workspace();
    workspace.pr_supervision_status = Some("review_paused".to_string());
    workspace.pr_supervision_summary =
        Some("GitHub auto-merge was paused because the review was cancelled.".to_string());

    assert!(!pr_fix_publish_can_resume_after_workspace_review(
        &workspace,
        &monitor,
        Some(&target),
        &[],
    ));
}

#[test]
fn resume_predicate_and_classification_accept_exact_human_bypass() {
    let target = review_target();
    let monitor = current_bypassed_monitor(&target);
    let workspace = pr_fix_workspace();
    let events = [publication_event(
        "pr_autofix_workspace_review",
        "reviewing",
        Some("workspace_review_started"),
    )];

    assert!(pr_fix_publish_can_resume_after_workspace_review(
        &workspace,
        &monitor,
        Some(&target),
        &events,
    ));
    assert_eq!(
        workspace_review_authorization_kind(&monitor, &target),
        Some(WorkspaceReviewAuthorizationKind::HumanBypass)
    );
}

#[tokio::test]
async fn human_bypass_resumes_pr_fix_with_distinct_audit_classification() {
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace = pr_fix_workspace();
    let target = review_target();
    let monitor = current_bypassed_monitor(&target);
    repo.create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    repo.append_publication_event(publication_event(
        "pr_autofix_workspace_review",
        "reviewing",
        Some("workspace_review_started"),
    ))
    .await
    .expect("seed pending review event");

    let outcome = resume_pr_fix_publish_after_passed_workspace_review(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &workspace.conversation_id,
        &workspace,
        &monitor,
        Some(&target),
        |_conversation_id| async { Ok(Some(true)) },
    )
    .await
    .expect("resume publish");

    assert_eq!(outcome, PrFixReviewPublishResumeOutcome::Published);
    let events = repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("list events");
    assert!(events.iter().any(|event| {
        event.step == "pr_autofix_workspace_review_passed"
            && event.status == "publishing"
            && event.classification.as_deref() == Some("workspace_review_approved_anyway")
            && event.summary.contains("approved anyway")
    }));
}

#[tokio::test]
async fn resume_returns_skipped_without_mutating_when_review_is_not_publishable() {
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace = pr_fix_workspace();
    let target = review_target();
    let mut monitor = current_passed_monitor(&target);
    monitor.reviewed_diff_fingerprint = Some("stale-diff".to_string());
    repo.create_or_update(workspace.clone())
        .await
        .expect("seed workspace");

    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = resume_pr_fix_publish_after_passed_workspace_review(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &workspace.conversation_id,
        &workspace,
        &monitor,
        Some(&target),
        {
            let calls = Arc::clone(&calls);
            move |_conversation_id| async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(true))
            }
        },
    )
    .await
    .expect("resume check");

    assert_eq!(outcome, PrFixReviewPublishResumeOutcome::Skipped);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("list events")
        .is_empty());
}

#[tokio::test]
async fn persisted_plan_mode_cannot_resume_publish_from_retained_review_authority() {
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let mut workspace = pr_fix_workspace();
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    let target = review_target();
    let monitor = current_passed_monitor(&target);
    repo.create_or_update(workspace.clone())
        .await
        .expect("seed PLAN workspace");
    repo.append_publication_event(publication_event(
        "pr_autofix_workspace_review",
        "reviewing",
        Some("workspace_review_started"),
    ))
    .await
    .expect("seed retained review event");
    let calls = Arc::new(AtomicUsize::new(0));

    let outcome = resume_pr_fix_publish_after_passed_workspace_review(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &workspace.conversation_id,
        &workspace,
        &monitor,
        Some(&target),
        {
            let calls = Arc::clone(&calls);
            move |_conversation_id| async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(true))
            }
        },
    )
    .await
    .expect("PLAN publish handoff should skip cleanly");

    assert_eq!(outcome, PrFixReviewPublishResumeOutcome::Skipped);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        repo.list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should load")
            .len(),
        1,
        "PLAN must not append a publication-resume event"
    );
}

#[tokio::test]
async fn resume_publishes_and_marks_monitoring_after_current_passed_review() {
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace = pr_fix_workspace();
    let target = review_target();
    let monitor = current_passed_monitor(&target);
    repo.create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    repo.append_publication_event(publication_event(
        "pr_autofix_workspace_review",
        "reviewing",
        Some("workspace_review_started"),
    ))
    .await
    .expect("seed pending review event");

    let outcome = resume_pr_fix_publish_after_passed_workspace_review(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &workspace.conversation_id,
        &workspace,
        &monitor,
        Some(&target),
        |_conversation_id| async { Ok(Some(true)) },
    )
    .await
    .expect("resume publish");

    assert_eq!(outcome, PrFixReviewPublishResumeOutcome::Published);
    let updated = repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    assert_eq!(updated.pr_auto_merge_current, Some(true));
    let events = repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("list events");
    assert!(events.iter().any(|event| {
        event.step == "pr_autofix_workspace_review_passed"
            && event.status == "publishing"
            && event.classification.as_deref() == Some("workspace_review_passed")
    }));
}

#[tokio::test]
async fn resume_records_blocked_state_when_publish_fails_after_review_passes() {
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace = pr_fix_workspace();
    let target = review_target();
    let monitor = current_passed_monitor(&target);
    repo.create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    repo.append_publication_event(publication_event(
        "pr_autofix_workspace_review",
        "reviewing",
        Some("workspace_review_started"),
    ))
    .await
    .expect("seed pending review event");

    let outcome = resume_pr_fix_publish_after_passed_workspace_review(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &workspace.conversation_id,
        &workspace,
        &monitor,
        Some(&target),
        |_conversation_id| async { Err("push rejected".to_string()) },
    )
    .await
    .expect("resume publish failure");

    assert_eq!(
        outcome,
        PrFixReviewPublishResumeOutcome::Failed {
            error: "push rejected".to_string(),
        }
    );
    let updated = repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert_eq!(
        updated.pr_supervision_summary.as_deref(),
        Some("Workspace Review passed, but PR fix publish failed: push rejected")
    );
    let events = repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("list events");
    assert!(events.iter().any(|event| {
        event.step == "pr_autofix_publish_failed"
            && event.status == "failed"
            && event.summary == "push rejected"
    }));
}
