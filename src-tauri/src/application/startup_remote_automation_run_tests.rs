use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use chrono::{Duration, Utc};

use super::{
    automation::{
        actions::dispatch_automation_run_now_action_with_spawner, api::automation_service_for_state,
    },
    startup_background::dispatch_one_remote_automation_run,
    AppState,
};
use crate::application::automation::plan_gate::PLAN_JUDGE_FAILED_PAUSED_REASON_CODE;
use crate::application::remote_automation_run_intent::{
    REMOTE_AUTOMATION_RUN_ALREADY_SETTLED, REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED,
    REMOTE_AUTOMATION_RUN_RUN_CHANGED, REMOTE_AUTOMATION_RUN_RUN_IN_FLIGHT,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId, RemoteAutomationRunKind,
    RemoteAutomationRunRequest, RemoteAutomationRunRequestStatus,
};
use crate::infrastructure::memory::{MemoryAutomationRepository, MemoryAutomationRunRepository};

fn automation(id: &str) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: ProjectId::from_string("dispatcher-project".to_string()),
        name: "Dispatcher automation".into(),
        status: AutomationStatus::Active,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Goal".into(),
        setup_conversation_id: None,
        provider_harness: "claude".into(),
        model_id: "sonnet".into(),
        logical_effort: None,
        run_mode: "edit".into(),
        base_ref_kind: "project_default".into(),
        base_ref: String::new(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".into(),
        completion_signal: "pr_merged".into(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run".into()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

fn run(
    automation_id: &AutomationId,
    id: &str,
    status: AutomationRunStatus,
    judge_state: AutomationJudgeState,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: automation_id.clone(),
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
        run_prompt: "Run".into(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".into(),
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
        started_at: Some(now),
        finished_at: Some(now),
        created_at: now,
        updated_at: now,
    }
}

async fn seed_request(
    state: &AppState,
    automation_id: &AutomationId,
    kind: RemoteAutomationRunKind,
    expected_run_id: Option<&str>,
) -> RemoteAutomationRunRequest {
    let now = Utc::now();
    state
        .remote_automation_run_request_repo
        .create_remote_automation_run_request(RemoteAutomationRunRequest {
            id: uuid::Uuid::new_v4().to_string(),
            automation_id: automation_id.as_str().into(),
            kind,
            expected_run_id: expected_run_id.map(str::to_string),
            status: RemoteAutomationRunRequestStatus::Pending,
            error_code: None,
            result: None,
            claimed_at: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap()
}

async fn read_request(state: &AppState, id: &str) -> RemoteAutomationRunRequest {
    state
        .remote_automation_run_request_repo
        .get(id)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn run_now_success_creates_exactly_one_run_completes_and_reentry_is_noop() {
    let state = AppState::new_test();
    let automation = automation("run-now-success");
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run(
            &automation.id,
            "cancelled-run",
            AutomationRunStatus::Cancelled,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();
    let request = seed_request(
        &state,
        &automation.id,
        RemoteAutomationRunKind::RunNow,
        Some("cancelled-run"),
    )
    .await;

    dispatch_one_remote_automation_run(&state).await.unwrap();
    let completed = read_request(&state, &request.id).await;
    assert_eq!(
        completed.status,
        RemoteAutomationRunRequestStatus::Completed
    );
    assert_eq!(
        state
            .automation_run_repo
            .list_for_automation(&automation.id)
            .await
            .unwrap()
            .len(),
        2
    );

    dispatch_one_remote_automation_run(&state).await.unwrap();
    assert_eq!(read_request(&state, &request.id).await, completed);
    assert_eq!(
        state
            .automation_run_repo
            .list_for_automation(&automation.id)
            .await
            .unwrap()
            .len(),
        2,
        "re-entry must not create another run"
    );
}

#[tokio::test]
async fn changed_cancelled_latest_run_fails_without_creating_a_new_run() {
    let state = AppState::new_test();
    let automation = automation("run-changed");
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run(
            &automation.id,
            "cancelled-current",
            AutomationRunStatus::Cancelled,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();
    let request = seed_request(
        &state,
        &automation.id,
        RemoteAutomationRunKind::RunNow,
        Some("stale-run-id"),
    )
    .await;

    dispatch_one_remote_automation_run(&state).await.unwrap();
    let failed = read_request(&state, &request.id).await;
    assert_eq!(failed.status, RemoteAutomationRunRequestStatus::Failed);
    assert_eq!(
        failed.error_code.as_deref(),
        Some(REMOTE_AUTOMATION_RUN_RUN_CHANGED)
    );
    assert_eq!(
        state
            .automation_run_repo
            .list_for_automation(&automation.id)
            .await
            .unwrap()
            .len(),
        1,
        "stale expectedRunId must suppress the cancelled-run replacement path"
    );
}

#[tokio::test]
async fn run_in_flight_and_retry_already_settled_are_benign_completions() {
    let state = AppState::new_test();
    let in_flight = automation("in-flight");
    state
        .automation_repo
        .create(in_flight.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run(
            &in_flight.id,
            "running",
            AutomationRunStatus::Running,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();
    let in_flight_request = seed_request(
        &state,
        &in_flight.id,
        RemoteAutomationRunKind::RunNow,
        Some("running"),
    )
    .await;
    dispatch_one_remote_automation_run(&state).await.unwrap();
    let completed = read_request(&state, &in_flight_request.id).await;
    assert_eq!(
        completed.status,
        RemoteAutomationRunRequestStatus::Completed
    );
    assert_eq!(
        completed
            .result
            .as_ref()
            .and_then(|value| value["code"].as_str()),
        Some(REMOTE_AUTOMATION_RUN_RUN_IN_FLIGHT)
    );
    assert_eq!(completed.result.as_ref().unwrap()["scheduled"], false);

    let settled = automation("settled");
    state.automation_repo.create(settled.clone()).await.unwrap();
    state
        .automation_run_repo
        .create_run(run(
            &settled.id,
            "healthy-judge",
            AutomationRunStatus::Merged,
            AutomationJudgeState::Done,
        ))
        .await
        .unwrap();
    let retry = seed_request(
        &state,
        &settled.id,
        RemoteAutomationRunKind::RetryJudge,
        Some("healthy-judge"),
    )
    .await;
    dispatch_one_remote_automation_run(&state).await.unwrap();
    let completed = read_request(&state, &retry.id).await;
    assert_eq!(
        completed.status,
        RemoteAutomationRunRequestStatus::Completed
    );
    assert_eq!(
        completed
            .result
            .as_ref()
            .and_then(|value| value["code"].as_str()),
        Some(REMOTE_AUTOMATION_RUN_ALREADY_SETTLED)
    );
}

#[tokio::test]
async fn plan_gate_failure_is_distinct() {
    let state = AppState::new_test();
    let mut automation = automation("plan-gate");
    automation.status = AutomationStatus::Paused;
    automation.paused_reason_code = Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.into());
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let request = seed_request(
        &state,
        &automation.id,
        RemoteAutomationRunKind::RunNow,
        None,
    )
    .await;
    dispatch_one_remote_automation_run(&state).await.unwrap();
    let failed = read_request(&state, &request.id).await;
    assert_eq!(failed.status, RemoteAutomationRunRequestStatus::Failed);
    assert_eq!(
        failed.error_code.as_deref(),
        Some(REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED)
    );
}

#[tokio::test]
async fn post_claim_repository_error_propagates_and_leaves_claim_starting() {
    let mut state = AppState::new_test();
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    state.automation_repo = automation_repo;
    state.automation_run_repo = run_repo.clone();
    let automation = automation("repo-error");
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let request = seed_request(
        &state,
        &automation.id,
        RemoteAutomationRunKind::RunNow,
        None,
    )
    .await;
    run_repo.fail_next_latest_lookup();

    assert!(dispatch_one_remote_automation_run(&state).await.is_err());
    let claimed = read_request(&state, &request.id).await;
    assert_eq!(claimed.status, RemoteAutomationRunRequestStatus::Starting);
    assert!(
        claimed.error_code.is_none(),
        "repo failure must not forge a terminal write"
    );
    assert!(claimed.result.is_none());
}

#[tokio::test]
async fn stale_claim_is_failed_stale_and_never_dispatched() {
    let state = AppState::new_test();
    let automation = automation("stale");
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let request = seed_request(
        &state,
        &automation.id,
        RemoteAutomationRunKind::RunNow,
        None,
    )
    .await;
    state
        .remote_automation_run_request_repo
        .claim_pending(Utc::now() - Duration::seconds(301))
        .await
        .unwrap();
    assert_eq!(
        state
            .remote_automation_run_request_repo
            .fail_stale(Utc::now() - Duration::seconds(300), Utc::now())
            .await
            .unwrap(),
        1
    );
    dispatch_one_remote_automation_run(&state).await.unwrap();
    assert_eq!(
        read_request(&state, &request.id).await.status,
        RemoteAutomationRunRequestStatus::FailedStale
    );
    assert!(state
        .automation_run_repo
        .list_for_automation(&automation.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn injectable_run_now_spawner_records_exactly_one_judge_schedule() {
    let state = AppState::new_test();
    let automation = automation("record-spawn");
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run(
            &automation.id,
            "failed-judge",
            AutomationRunStatus::Merged,
            AutomationJudgeState::Failed,
        ))
        .await
        .unwrap();
    let service = automation_service_for_state(&state);
    let action = service
        .trigger_run_now_action(&automation.id)
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let recorded = calls.clone();
    let outcome = dispatch_automation_run_now_action_with_spawner(
        &automation.id,
        &state,
        service,
        action,
        move |_| {
            recorded.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await
    .unwrap();
    assert!(outcome.scheduled);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        state
            .automation_run_repo
            .get_by_id(&AutomationRunId::from_string("failed-judge"))
            .await
            .unwrap()
            .unwrap()
            .judge_state,
        AutomationJudgeState::InProgress
    );
}
