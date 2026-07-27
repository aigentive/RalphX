use chrono::Utc;

use super::actions::{
    dispatch_automation_run_now_action_with_spawner, retry_automation_judge_for_state,
    retry_automation_plan_judge_for_state, trigger_automation_run_now_for_state,
};
use super::api::automation_service_for_state;
use crate::application::AppState;
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversationId, ProjectId,
};
use crate::error::AppError;

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
        goal_items_json: Some(
            r#"[{"id":"phase-1","title":"Run 1","status":"pending"}]"#.to_string(),
        ),
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

fn automation_run(
    id: &str,
    automation_id: &AutomationId,
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
        conversation_id: Some(ChatConversationId::from_string("conversation-1")),
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
        started_at: Some(now),
        finished_at: Some(now),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn trigger_run_now_for_state_returns_readiness_reason_without_dispatching() {
    let state = AppState::new_test();
    let stopped = automation("automation-1", AutomationStatus::Stopped);
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &stopped.id,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();

    let outcome = trigger_automation_run_now_for_state(&stopped.id, &state)
        .await
        .unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(outcome.reason.as_deref(), Some("automation is not active"));
    let updated = state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.judge_state, AutomationJudgeState::None);
    assert!(updated.judge_lease_expires_at.is_none());
}

#[tokio::test]
async fn dispatch_run_now_action_marks_judge_in_progress_before_spawning() {
    let state = AppState::new_test();
    let active = automation("automation-1", AutomationStatus::Active);
    state.automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();
    let service = automation_service_for_state(&state);
    let action = service.trigger_run_now_action(&active.id).await.unwrap();

    let outcome = dispatch_automation_run_now_action_with_spawner(
        &active.id,
        &state,
        service,
        action,
        |dispatch| {
            assert_eq!(dispatch.automation.id, active.id);
            assert_eq!(dispatch.run.id, run.id);
            assert_eq!(dispatch.runs.len(), 1);
            assert!(dispatch.judge_lease_expires_at > Utc::now());
        },
    )
    .await
    .unwrap();

    assert!(outcome.scheduled);
    assert_eq!(outcome.reason, None);
    let stored = state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.judge_state, AutomationJudgeState::InProgress);
    assert!(stored.judge_lease_expires_at.is_some());
}

#[tokio::test]
async fn retry_judge_for_state_returns_readiness_reason_without_dispatching() {
    let state = AppState::new_test();
    let active = automation("automation-1", AutomationStatus::Active);
    state.automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();

    let outcome = retry_automation_judge_for_state(&active.id, &state)
        .await
        .unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(
        outcome.reason.as_deref(),
        Some("latest judge is not failed")
    );
    let stored = state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.judge_state, AutomationJudgeState::None);
}

#[tokio::test]
async fn retry_plan_judge_for_state_requires_latest_run_context() {
    let state = AppState::new_test();
    let active = automation("automation-1", AutomationStatus::Active);
    state.automation_repo.create(active.clone()).await.unwrap();

    let no_runs = retry_automation_plan_judge_for_state(&active.id, &state)
        .await
        .unwrap_err();
    assert!(
        matches!(no_runs, AppError::Validation(message) if message == "automation has no runs")
    );

    let mut run_without_conversation = automation_run(
        "run-1",
        &active.id,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run_without_conversation.conversation_id = None;
    state
        .automation_run_repo
        .create_run(run_without_conversation)
        .await
        .unwrap();

    let no_conversation = retry_automation_plan_judge_for_state(&active.id, &state)
        .await
        .unwrap_err();
    assert!(
        matches!(no_conversation, AppError::Validation(message) if message == "latest automation run has no plan conversation")
    );
}

#[tokio::test]
async fn retry_plan_judge_for_state_requires_plan_workspace() {
    let state = AppState::new_test();
    let active = automation("automation-1", AutomationStatus::Active);
    state.automation_repo.create(active.clone()).await.unwrap();
    let mut run = automation_run(
        "run-1",
        &active.id,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::Failed;
    run.plan_last_parked_artifact_id = Some("plan-current".to_string());
    state.automation_run_repo.create_run(run).await.unwrap();

    let error = retry_automation_plan_judge_for_state(&active.id, &state)
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::NotFound(message) if message.contains("Automation plan workspace not found"))
    );
}
