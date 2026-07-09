use std::sync::{Arc, Mutex};

use chrono::Utc;
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;

use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, AutomationTransitionService,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
};
use crate::domain::repositories::{AutomationRepository, AutomationRunRepository};
use crate::error::AppError;
use crate::infrastructure::memory::{MemoryAutomationRepository, MemoryAutomationRunRepository};

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
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Run prompt".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: String::new(),
        base_from_run_id: None,
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
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let commands =
        std::fs::read_to_string(manifest_dir.join("src/commands/automation_commands.rs")).unwrap();
    let delete =
        std::fs::read_to_string(manifest_dir.join("src/application/automation/delete.rs")).unwrap();
    let api =
        std::fs::read_to_string(manifest_dir.join("src/application/automation/api.rs")).unwrap();

    assert!(
        !commands.contains("NoopAutomationEventEmitter"),
        "automation commands must use the shared transition-service factory"
    );
    assert!(
        !delete.contains("NoopAutomationEventEmitter"),
        "automation delete must use the shared transition-service factory"
    );
    assert_eq!(
        api.matches("Arc::new(NoopAutomationEventEmitter)").count(),
        1,
        "automation API should own the single Noop event-emitter fallback"
    );
}

#[tokio::test]
async fn transition_service_emits_after_successful_automation_status_cas() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service =
        AutomationTransitionService::new(automation_repo.clone(), run_repo, emitter.clone());
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
    let service = AutomationTransitionService::new(automation_repo, run_repo, emitter.clone());

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
    let service =
        AutomationTransitionService::new(automation_repo, run_repo.clone(), emitter.clone());
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
async fn transition_service_emits_after_successful_merge_metadata_cas() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service =
        AutomationTransitionService::new(automation_repo, run_repo.clone(), emitter.clone());
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
async fn transition_service_validates_judge_lifecycle_before_cas() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let emitter = Arc::new(RecordingEmitter::default());
    let service =
        AutomationTransitionService::new(automation_repo, run_repo.clone(), emitter.clone());
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
