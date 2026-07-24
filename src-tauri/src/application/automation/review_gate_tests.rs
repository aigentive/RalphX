use chrono::Utc;

use crate::application::automation::review_gate::{
    pause_automation_for_blocked_workspace_review, run_is_workspace_review_blocked,
    WORKSPACE_REVIEW_BLOCKED_REASON_CODE,
};
use crate::application::AppState;
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversationId, ProjectId,
};

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

fn automation_run(
    id: &str,
    automation_id: &AutomationId,
    run_index: i64,
    status: AutomationRunStatus,
    conversation_id: &ChatConversationId,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: automation_id.clone(),
        run_index,
        status,
        judge_state: AutomationJudgeState::None,
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
        conversation_id: Some(conversation_id.clone()),
        run_prompt: "Run prompt".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: Some("ralphx/run-1".to_string()),
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
        finished_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn pause_helper_pauses_automation_and_terminalizes_run() {
    let state = AppState::new_test();
    let conversation = ChatConversationId::from_string("conv-blocked");
    let active = automation("automation-1", AutomationStatus::Active);
    state.automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Running,
        &conversation,
    );
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();

    let paused = pause_automation_for_blocked_workspace_review(
        &state,
        &conversation,
        Some("Workspace review blocked (artifact art-1): needs changes"),
    )
    .await
    .unwrap();
    assert!(paused, "an automation-owned conversation should be paused");

    let automation = state
        .automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Paused);
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(WORKSPACE_REVIEW_BLOCKED_REASON_CODE)
    );

    let terminal_run = state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(terminal_run.status, AutomationRunStatus::AgentFailed);
    assert_eq!(
        terminal_run.error_code.as_deref(),
        Some(WORKSPACE_REVIEW_BLOCKED_REASON_CODE)
    );
    assert!(terminal_run.finished_at.is_some());
    assert!(run_is_workspace_review_blocked(&terminal_run));
}

#[tokio::test]
async fn pause_helper_is_noop_for_non_automation_conversation() {
    let state = AppState::new_test();
    let conversation = ChatConversationId::from_string("conv-interactive");

    let paused = pause_automation_for_blocked_workspace_review(&state, &conversation, None)
        .await
        .unwrap();
    assert!(
        !paused,
        "a non-automation conversation must not trigger a pause"
    );
}

#[tokio::test]
async fn pause_helper_soft_noops_when_automation_already_paused() {
    let state = AppState::new_test();
    let conversation = ChatConversationId::from_string("conv-double");
    // Automation already Paused: pause() will conflict, but the run must still be terminalized.
    let mut paused_automation = automation("automation-1", AutomationStatus::Paused);
    paused_automation.paused_reason_code = Some(WORKSPACE_REVIEW_BLOCKED_REASON_CODE.to_string());
    state
        .automation_repo
        .create(paused_automation.clone())
        .await
        .unwrap();
    let run = automation_run(
        "run-1",
        &paused_automation.id,
        1,
        AutomationRunStatus::Running,
        &conversation,
    );
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();

    let handled = pause_automation_for_blocked_workspace_review(&state, &conversation, None)
        .await
        .unwrap();
    assert!(handled);
    let terminal_run = state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(terminal_run.status, AutomationRunStatus::AgentFailed);
    assert_eq!(
        terminal_run.error_code.as_deref(),
        Some(WORKSPACE_REVIEW_BLOCKED_REASON_CODE)
    );
}
