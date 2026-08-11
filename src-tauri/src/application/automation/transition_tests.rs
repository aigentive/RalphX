use std::sync::{Arc, Mutex};

use chrono::Utc;
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;

use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, AutomationTransitionService,
};
use crate::application::notification_service::{NoopNotificationEventEmitter, NotificationService};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
};
use crate::domain::repositories::{
    AutomationRepository, AutomationRunRepository, NotificationRepository,
};
use crate::error::AppError;
use crate::infrastructure::memory::{
    MemoryAutomationRepository, MemoryAutomationRunRepository, MemoryNotificationRepository,
};

#[derive(Default)]
struct RecordingEmitter {
    events: Mutex<Vec<AutomationEvent>>,
}

impl RecordingEmitter {
    fn events(&self) -> Vec<AutomationEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl AutomationEventEmitter for RecordingEmitter {
    fn emit(&self, event: AutomationEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn notification_service() -> Arc<NotificationService> {
    Arc::new(NotificationService::new(
        Arc::new(MemoryNotificationRepository::new()) as Arc<dyn NotificationRepository>,
        Arc::new(NoopNotificationEventEmitter),
    ))
}

fn notification_service_with_repo() -> (Arc<NotificationService>, Arc<MemoryNotificationRepository>)
{
    let repo = Arc::new(MemoryNotificationRepository::new());
    let service = Arc::new(NotificationService::new(
        repo.clone() as Arc<dyn NotificationRepository>,
        Arc::new(NoopNotificationEventEmitter),
    ));
    (service, repo)
}

fn automation(id: &str, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: ProjectId::from_string("project-1".to_string()),
        name: format!("Automation {id}"),
        status,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Goal".to_string(),
        setup_conversation_id: None,
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: None,
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: String::new(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

fn run(id: &str, status: AutomationRunStatus, judge_state: AutomationJudgeState) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: AutomationId::from_string("automation-1"),
        run_index: 1,
        status,
        judge_state,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Run prompt".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: String::new(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: None,
        pr_number: None,
        pr_url: None,
        pr_title: None,
        pr_head_ref_name: None,
        pr_base_ref_name: None,
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: None,
        judge_verdict_json: None,
        judge_model_id: None,
        error_code: None,
        error_detail: None,
        signal_check_failures: 0,
        started_at: None,
        finished_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn production_automation_transition_services_use_shared_event_emitter_factory() {
    const API_SOURCE: &str = include_str!("api.rs");
    const PRODUCTION_SOURCES: &[(&str, &str)] = &[
        (
            "src/application/automation/delete.rs",
            include_str!("delete.rs"),
        ),
        (
            "src/application/automation/judge.rs",
            include_str!("judge.rs"),
        ),
        (
            "src/application/automation/plan_gate.rs",
            include_str!("plan_gate.rs"),
        ),
        (
            "src/application/automation/plan_judge.rs",
            include_str!("plan_judge.rs"),
        ),
        (
            "src/application/automation/provisioning.rs",
            include_str!("provisioning.rs"),
        ),
        (
            "src/application/automation/review_gate.rs",
            include_str!("review_gate.rs"),
        ),
        (
            "src/application/automation/scheduler.rs",
            include_str!("scheduler.rs"),
        ),
        (
            "src/application/automation/service.rs",
            include_str!("service.rs"),
        ),
        (
            "src/commands/automation_commands.rs",
            include_str!("../../commands/automation_commands.rs"),
        ),
    ];

    let offenders: Vec<_> = PRODUCTION_SOURCES
        .iter()
        .filter_map(|(path, source)| {
            source
                .contains("NoopAutomationEventEmitter")
                .then_some(*path)
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "NoopAutomationEventEmitter construction must stay in the API fallback; offenders: {offenders:?}"
    );
    assert_eq!(
        API_SOURCE
            .matches("Arc::new(NoopAutomationEventEmitter)")
            .count(),
        1,
        "automation API should own the single Noop event-emitter fallback"
    );
}

#[test]
fn automation_event_names_are_stable_for_ui_subscriptions() {
    assert_eq!(
        (AutomationEvent::AutomationUpdated {
            automation_id: AutomationId::from_string("automation-1")
        })
        .event_name(),
        "automation:updated"
    );
    assert_eq!(
        (AutomationEvent::AutomationRunUpdated {
            automation_id: AutomationId::from_string("automation-1"),
            run_id: AutomationRunId::from_string("run-1")
        })
        .event_name(),
        "automation:run:updated"
    );
    assert_eq!(
        (AutomationEvent::AutomationDeleted {
            automation_id: AutomationId::from_string("automation-1"),
            project_id: ProjectId::from_string("project-1".to_string())
        })
        .event_name(),
        "automation:deleted"
    );
}

#[tokio::test]
async fn transition_service_emits_after_successful_automation_status_cas() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo.clone(),
        run_repo,
        emitter.clone(),
        notification_service(),
    );
    let automation = automation("automation-1", AutomationStatus::Draft);
    automation_repo.create(automation.clone()).await.unwrap();

    assert!(service
        .transition_automation_status(
            &automation.id,
            AutomationStatus::Draft,
            AutomationStatus::Active,
            None,
            None,
        )
        .await
        .unwrap());

    assert_eq!(
        automation_repo
            .get_by_id(&automation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Active
    );
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationUpdated {
            automation_id: automation.id
        }]
    );
}

#[tokio::test]
async fn transition_service_rejects_invalid_automation_transition_without_emit() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo,
        run_repo,
        emitter.clone(),
        notification_service(),
    );

    let error = service
        .transition_automation_status(
            &AutomationId::from_string("automation-1"),
            AutomationStatus::Completed,
            AutomationStatus::Active,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidTransition { .. }));
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn transition_service_emits_only_when_run_status_cas_wins() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo,
        run_repo.clone(),
        emitter.clone(),
        notification_service(),
    );
    let run = run(
        "run-1",
        AutomationRunStatus::Pending,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    assert!(!service
        .transition_run_status(
            &run.id,
            AutomationRunStatus::Running,
            AutomationRunStatus::Published,
            None,
            None,
        )
        .await
        .unwrap());
    assert!(emitter.events().is_empty());

    assert!(service
        .transition_run_status(
            &run.id,
            AutomationRunStatus::Pending,
            AutomationRunStatus::Provisioning,
            None,
            None,
        )
        .await
        .unwrap());
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationRunUpdated {
            automation_id: run.automation_id,
            run_id: run.id,
        }]
    );
}

#[tokio::test]
async fn reopen_run_corrective_is_the_only_agent_failed_to_running_seam() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo,
        run_repo.clone(),
        emitter.clone(),
        notification_service(),
    );
    let mut failed = run(
        "run-reopen-corrective",
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    failed.finished_at = Some(Utc::now());
    failed.error_code = Some("agent_failed".to_string());
    failed.error_detail = Some("interrupted".to_string());
    run_repo.create_run(failed.clone()).await.unwrap();

    let normal_error = service
        .transition_run_status(
            &failed.id,
            AutomationRunStatus::AgentFailed,
            AutomationRunStatus::Running,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(normal_error, AppError::InvalidTransition { .. }));
    assert!(emitter.events().is_empty());

    let corrective_misuse = service
        .reopen_run_corrective(&failed.id, AutomationRunStatus::Completed)
        .await
        .unwrap_err();
    assert!(matches!(
        corrective_misuse,
        AppError::InvalidTransition { .. }
    ));
    assert_eq!(
        run_repo
            .get_by_id(&failed.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationRunStatus::AgentFailed
    );
    assert!(emitter.events().is_empty());

    assert!(service
        .reopen_run_corrective(&failed.id, AutomationRunStatus::AgentFailed)
        .await
        .unwrap());
    let reopened = run_repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(reopened.status, AutomationRunStatus::Running);
    assert!(reopened.error_code.is_none());
    assert!(reopened.error_detail.is_none());
    assert!(reopened.agent_phase_started_at.is_some());
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationRunUpdated {
            automation_id: failed.automation_id,
            run_id: failed.id.clone(),
        }]
    );

    assert!(!service
        .reopen_run_corrective(&failed.id, AutomationRunStatus::AgentFailed)
        .await
        .unwrap());
    assert_eq!(emitter.events().len(), 1);
}

#[tokio::test]
async fn transition_service_records_automation_run_notifications_only_after_winning_cas() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let (notification_service, notification_repo) = notification_service_with_repo();
    let service = AutomationTransitionService::new(
        automation_repo.clone(),
        run_repo.clone(),
        Arc::new(RecordingEmitter::default()),
        notification_service,
    );
    let mut awaiting = run(
        "awaiting",
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    let mut failed = run(
        "failed",
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    let mut completed = run(
        "completed",
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    completed.id = AutomationRunId::from_string("completed");
    let mut merged = run(
        "merged",
        AutomationRunStatus::Published,
        AutomationJudgeState::None,
    );
    let mut closed = run(
        "closed",
        AutomationRunStatus::Published,
        AutomationJudgeState::None,
    );
    for (automation_id, run) in [
        ("automation-awaiting", &mut awaiting),
        ("automation-failed", &mut failed),
        ("automation-completed", &mut completed),
        ("automation-merged", &mut merged),
        ("automation-closed", &mut closed),
    ] {
        run.automation_id = AutomationId::from_string(automation_id);
        automation_repo
            .create(automation(automation_id, AutomationStatus::Active))
            .await
            .unwrap();
        run_repo.create_run(run.clone()).await.unwrap();
    }

    assert!(service
        .transition_run_status(
            &awaiting.id,
            AutomationRunStatus::Running,
            AutomationRunStatus::AwaitingPlanApproval,
            None,
            None
        )
        .await
        .unwrap());
    assert!(service
        .transition_run_status(
            &failed.id,
            AutomationRunStatus::Running,
            AutomationRunStatus::AgentFailed,
            Some("timeout".to_string()),
            None
        )
        .await
        .unwrap());
    assert!(service
        .transition_run_status(
            &completed.id,
            AutomationRunStatus::Running,
            AutomationRunStatus::Completed,
            None,
            None
        )
        .await
        .unwrap());
    assert!(service
        .transition_run_status_with_merge_metadata(
            &merged.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Merged,
            Some("sha".to_string()),
            Some(Utc::now())
        )
        .await
        .unwrap());
    assert!(service
        .transition_run_status(
            &closed.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::PrClosed,
            None,
            None
        )
        .await
        .unwrap());
    assert!(!service
        .transition_run_status(
            &awaiting.id,
            AutomationRunStatus::Running,
            AutomationRunStatus::AwaitingPlanApproval,
            None,
            None
        )
        .await
        .unwrap());

    let rows = notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications;
    assert_eq!(rows.len(), 5);
    assert!(rows
        .iter()
        .any(|row| row.dedupe_key.as_deref() == Some("run:awaiting:plan_approval")));
    assert!(rows
        .iter()
        .any(|row| row.dedupe_key.as_deref() == Some("run:failed:failed:timeout")));
    assert!(rows.iter().any(|row| {
        row.body.as_deref() == Some("Run #1 of “Automation automation-failed”: run timed out")
    }));
    assert!(rows
        .iter()
        .any(|row| row.dedupe_key.as_deref() == Some("run:completed:completed")));
    assert!(rows
        .iter()
        .any(|row| row.dedupe_key.as_deref() == Some("run:merged:completed")));
    assert!(rows
        .iter()
        .any(|row| row.dedupe_key.as_deref() == Some("run:closed:pr_closed")));
}

#[tokio::test]
async fn transition_service_records_only_actionable_automation_pauses() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let (notification_service, notification_repo) = notification_service_with_repo();
    let service = AutomationTransitionService::new(
        automation_repo.clone(),
        run_repo,
        Arc::new(RecordingEmitter::default()),
        notification_service,
    );
    let automation = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(automation.clone()).await.unwrap();

    assert!(service
        .transition_automation_status(
            &automation.id,
            AutomationStatus::Active,
            AutomationStatus::Paused,
            Some("user".to_string()),
            None
        )
        .await
        .unwrap());
    assert!(notification_repo
        .list(None, None, 10)
        .await
        .unwrap()
        .notifications
        .is_empty());
    assert!(service
        .transition_automation_status(
            &automation.id,
            AutomationStatus::Paused,
            AutomationStatus::Active,
            None,
            None
        )
        .await
        .unwrap());
    let actionable_reasons = [
        "signal_verification_failed",
        "judge_loop_suspected",
        "judge_stopped_unmet",
        "goal_replan_stale",
        "ideation_bridge_verification_failed",
        "ideation_bridge_missing_session",
    ];
    for reason in actionable_reasons {
        assert!(service
            .transition_automation_status(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some(reason.to_string()),
                None
            )
            .await
            .unwrap());
        assert!(service
            .transition_automation_status(
                &automation.id,
                AutomationStatus::Paused,
                AutomationStatus::Active,
                None,
                None
            )
            .await
            .unwrap());
    }
    assert_eq!(
        notification_repo
            .list(None, None, 10)
            .await
            .unwrap()
            .notifications
            .len(),
        actionable_reasons.len()
    );
}

#[tokio::test]
async fn transition_service_records_auto_merge_enable_warning_once_as_action_required() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let (notification_service, notification_repo) = notification_service_with_repo();
    let service = AutomationTransitionService::new(
        automation_repo.clone(),
        run_repo.clone(),
        Arc::new(RecordingEmitter::default()),
        notification_service,
    );
    let automation = automation("automation-auto-merge", AutomationStatus::Active);
    automation_repo.create(automation.clone()).await.unwrap();
    let mut published = run(
        "auto-merge-warning",
        AutomationRunStatus::Published,
        AutomationJudgeState::None,
    );
    published.automation_id = automation.id.clone();
    published.pr_number = Some(733);
    run_repo.create_run(published.clone()).await.unwrap();

    for _ in 0..2 {
        service
            .record_auto_merge_enable_warning(
                &automation,
                &published,
                "GitHub rejected automatic merge enablement",
            )
            .await;
    }

    let notifications = notification_repo
        .list(None, None, 10)
        .await
        .unwrap()
        .notifications;
    assert_eq!(notifications.len(), 1);
    assert_eq!(
        notifications[0].severity,
        crate::domain::entities::NotificationSeverity::ActionRequired
    );
    assert_eq!(
        notifications[0].category,
        crate::domain::entities::NotificationCategory::AutomationRunFailed
    );
    assert_eq!(
        notifications[0].dedupe_key.as_deref(),
        Some("run:auto-merge-warning:auto_merge_enable_failed")
    );
}

#[tokio::test]
async fn transition_service_emits_after_successful_merge_metadata_cas() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo,
        run_repo.clone(),
        emitter.clone(),
        notification_service(),
    );
    let run = run(
        "run-1",
        AutomationRunStatus::Published,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    run_repo.lose_next_published_to_merged_cas();
    assert!(!service
        .transition_run_status_with_merge_metadata(
            &run.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Merged,
            Some("lost-sha".to_string()),
            Some(Utc::now()),
        )
        .await
        .unwrap());
    assert!(emitter.events().is_empty());
    let unchanged = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(unchanged.status, AutomationRunStatus::Published);
    assert!(unchanged.merge_commit_sha.is_none());

    let merged_at = Utc::now();
    assert!(service
        .transition_run_status_with_merge_metadata(
            &run.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Merged,
            Some("merge-sha".to_string()),
            Some(merged_at),
        )
        .await
        .unwrap());
    let stored = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(stored.status, AutomationRunStatus::Merged);
    assert_eq!(stored.merge_commit_sha.as_deref(), Some("merge-sha"));
    assert_eq!(stored.pr_merged_at, Some(merged_at));
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationRunUpdated {
            automation_id: run.automation_id,
            run_id: run.id,
        }]
    );
}

#[tokio::test]
async fn transition_service_records_explicit_agent_phase_start_time() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo,
        run_repo.clone(),
        emitter.clone(),
        notification_service(),
    );
    let run = run(
        "run-1",
        AutomationRunStatus::Provisioning,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();
    let agent_phase_started_at = Utc::now();

    assert!(service
        .transition_run_status_with_agent_phase_started_at(
            &run.id,
            AutomationRunStatus::Provisioning,
            AutomationRunStatus::Running,
            agent_phase_started_at,
            None,
            None,
        )
        .await
        .unwrap());

    let stored = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(stored.status, AutomationRunStatus::Running);
    assert_eq!(stored.agent_phase_started_at, Some(agent_phase_started_at));
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationRunUpdated {
            automation_id: run.automation_id,
            run_id: run.id,
        }]
    );
}

#[tokio::test]
async fn transition_service_clears_plan_pending_instructions_on_retry_start() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo,
        run_repo.clone(),
        emitter.clone(),
        notification_service(),
    );
    let mut run = run(
        "run-1",
        AutomationRunStatus::Pending,
        AutomationJudgeState::None,
    );
    run.plan_pending_instructions = Some("Revise the plan before retrying".to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    assert!(service
        .transition_run_status_clearing_plan_pending_instructions(
            &run.id,
            AutomationRunStatus::Pending,
            AutomationRunStatus::Provisioning,
            None,
            None,
        )
        .await
        .unwrap());

    let stored = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(stored.status, AutomationRunStatus::Provisioning);
    assert!(stored.plan_pending_instructions.is_none());
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationRunUpdated {
            automation_id: run.automation_id,
            run_id: run.id,
        }]
    );
}

#[tokio::test]
async fn leaving_awaiting_plan_approval_resolves_only_that_run_notification() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let (notification_service, notification_repo) = notification_service_with_repo();
    let service = AutomationTransitionService::new(
        automation_repo.clone(),
        run_repo.clone(),
        emitter,
        notification_service,
    );
    automation_repo
        .create(automation("automation-1", AutomationStatus::Active))
        .await
        .unwrap();
    let run = run(
        "run-plan",
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    assert!(service
        .transition_run_status(
            &run.id,
            AutomationRunStatus::Running,
            AutomationRunStatus::AwaitingPlanApproval,
            None,
            None,
        )
        .await
        .unwrap());
    assert!(service
        .transition_run_status_clearing_plan_pending_instructions(
            &run.id,
            AutomationRunStatus::AwaitingPlanApproval,
            AutomationRunStatus::Running,
            None,
            None,
        )
        .await
        .unwrap());

    let rows = notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications;
    let plan_notification = rows
        .iter()
        .find(|row| row.dedupe_key.as_deref() == Some("run:run-plan:plan_approval"))
        .expect("plan approval notification");
    assert!(plan_notification.read_at.is_some());
}

#[tokio::test]
async fn transition_service_validates_judge_lifecycle_before_cas() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo,
        run_repo.clone(),
        emitter.clone(),
        notification_service(),
    );
    let run = run(
        "run-1",
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    let error = service
        .transition_judge_state(
            &run.id,
            AutomationJudgeState::None,
            AutomationJudgeState::Done,
            AutomationJudgeTransitionGuard::Dispatch,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidTransition { .. }));
    assert!(emitter.events().is_empty());

    assert!(service
        .transition_judge_state(
            &run.id,
            AutomationJudgeState::None,
            AutomationJudgeState::InProgress,
            AutomationJudgeTransitionGuard::Dispatch,
            None,
            None,
            Some(Utc::now()),
            None,
        )
        .await
        .unwrap());
    let settle_without_token = service
        .transition_judge_state(
            &run.id,
            AutomationJudgeState::InProgress,
            AutomationJudgeState::Done,
            AutomationJudgeTransitionGuard::Dispatch,
            Some(r#"{"decision":"stop"}"#.to_string()),
            Some("judge-model".to_string()),
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(settle_without_token, AppError::Validation(_)));
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationRunUpdated {
            automation_id: run.automation_id,
            run_id: run.id,
        }]
    );
}

#[tokio::test]
async fn transition_service_emits_after_successful_plan_judge_state_cas() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service = AutomationTransitionService::new(
        automation_repo,
        run_repo.clone(),
        emitter.clone(),
        notification_service(),
    );
    let run = run(
        "run-1",
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();
    let lease_expires_at = Utc::now();

    assert!(service
        .transition_plan_judge_state(
            &run.id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::InProgress,
            None,
            Some(lease_expires_at),
        )
        .await
        .unwrap());
    let stored = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(
        stored.plan_judge_state,
        AutomationPlanJudgeState::InProgress
    );
    assert_eq!(stored.plan_judge_lease_expires_at, Some(lease_expires_at));
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationRunUpdated {
            automation_id: run.automation_id,
            run_id: run.id,
        }]
    );
}
