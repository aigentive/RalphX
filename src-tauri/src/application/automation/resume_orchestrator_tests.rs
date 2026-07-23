use super::reopen_tests::{run, setup_smart_resume_fixture, RecordingRedriver, ReopenFixture};
use super::resume_orchestrator::resume_automation_smart_with_redriver;
use crate::domain::entities::{AutomationJudgeState, AutomationRunStatus, AutomationStatus};
use crate::error::AppError;

async fn seed_prior_failures(fixture: &ReopenFixture, count: i64) {
    for run_index in 1..=count {
        let prior = run(
            &format!("run-prior-{run_index}"),
            &fixture.automation.id,
            run_index,
            AutomationRunStatus::AgentFailed,
            None,
        );
        fixture
            .state
            .automation_run_repo
            .create_run(prior)
            .await
            .expect("prior failed run");
    }
}

#[tokio::test]
async fn resume_automation_smart_reopens_latest_failed_run_despite_exhausted_retry_limit() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::AgentFailed,
        AutomationStatus::Paused,
        Some("judge_stopped_unmet"),
        true,
        3,
    )
    .await;
    seed_prior_failures(&fixture, 2).await;
    let redriver = RecordingRedriver::default();

    let resumed =
        resume_automation_smart_with_redriver(&fixture.state, &fixture.automation.id, &redriver)
            .await
            .expect("reopen should bypass retry-only consecutive failure guard");

    assert_eq!(resumed.status, AutomationStatus::Active);
    let reopened = fixture
        .state
        .automation_run_repo
        .get_by_id(&fixture.run.id)
        .await
        .expect("run read")
        .expect("latest run");
    assert_eq!(reopened.status, AutomationRunStatus::Running);
    assert_eq!(reopened.judge_state, AutomationJudgeState::None);
    assert!(reopened.judge_verdict_json.is_none());
    assert_eq!(
        redriver.redrives(),
        vec![(
            fixture.conversation_id,
            super::reopen::AUTOMATION_RUN_CONTINUATION_PROMPT.to_string(),
        )]
    );
}

#[tokio::test]
async fn resume_automation_smart_spawns_retry_when_latest_failure_has_no_conversation() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::AgentFailed,
        AutomationStatus::Paused,
        Some("judge_stopped_unmet"),
        false,
        1,
    )
    .await;
    let resumed = resume_automation_smart_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &RecordingRedriver::default(),
    )
    .await
    .expect("non-reopenable latest run should use retry fallback");

    assert_eq!(resumed.status, AutomationStatus::Active);
    let runs = fixture
        .state
        .automation_run_repo
        .list_for_automation(&fixture.automation.id)
        .await
        .expect("automation runs");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].status, AutomationRunStatus::Pending);
    assert_eq!(runs[1].base_from_run_id, Some(fixture.run.id));
}

#[tokio::test]
async fn resume_automation_smart_returns_clear_error_when_retry_limit_is_exhausted() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::AgentFailed,
        AutomationStatus::Paused,
        Some("judge_stopped_unmet"),
        false,
        3,
    )
    .await;
    seed_prior_failures(&fixture, 2).await;
    let error = resume_automation_smart_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &RecordingRedriver::default(),
    )
    .await
    .expect_err("exhausted retry limit should fail clearly");
    let message = error.to_string();

    assert!(!message.trim().is_empty());
    assert!(message.contains("3 consecutive runs failed"));
    assert!(message.contains("limit 3"));
    assert_eq!(
        fixture
            .state
            .automation_repo
            .get_by_id(&fixture.automation.id)
            .await
            .expect("automation read")
            .expect("automation")
            .status,
        AutomationStatus::Paused
    );
}

#[tokio::test]
async fn resume_automation_smart_rejects_already_active_automation() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::Completed,
        AutomationStatus::Active,
        None,
        false,
        1,
    )
    .await;

    let error = resume_automation_smart_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &RecordingRedriver::default(),
    )
    .await
    .expect_err("active automation has nothing to resume");

    assert!(
        matches!(error, AppError::Conflict(ref detail) if detail == "automation is already active")
    );
}

#[tokio::test]
async fn resume_automation_smart_does_not_reopen_failed_run_for_completed_automation() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::AgentFailed,
        AutomationStatus::Completed,
        None,
        true,
        1,
    )
    .await;
    let redriver = RecordingRedriver::default();

    let error =
        resume_automation_smart_with_redriver(&fixture.state, &fixture.automation.id, &redriver)
            .await
            .expect_err("completed automation must remain terminal");

    assert!(
        matches!(error, AppError::Conflict(ref detail) if detail == "automation is completed and has no work to resume")
    );
    assert_eq!(
        fixture
            .state
            .automation_run_repo
            .get_by_id(&fixture.run.id)
            .await
            .expect("run read")
            .expect("latest run")
            .status,
        AutomationRunStatus::AgentFailed
    );
    assert!(redriver.redrives().is_empty());
}
