use chrono::Utc;

use super::remote_automation_commands::{
    request_remote_automation_draft_for_state, request_remote_automation_run_for_state,
    RequestRemoteAutomationDraftInput, RequestRemoteAutomationRunInput,
    REMOTE_AUTOMATION_RUN_ALREADY_SETTLED, REMOTE_AUTOMATION_RUN_AUTHORITY_CHANGED,
    REMOTE_AUTOMATION_RUN_NOT_FOUND, REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED,
};
use crate::application::automation::plan_gate::PLAN_JUDGE_FAILED_PAUSED_REASON_CODE;
use crate::application::AppState;
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, Project, ProjectId,
    RemoteAutomationRunKind,
};

fn automation(id: &str, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: ProjectId::from_string("remote-automation-project".to_string()),
        name: "Remote automation".into(),
        status,
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

fn draft_input(project_id: &str, name: &str) -> RequestRemoteAutomationDraftInput {
    RequestRemoteAutomationDraftInput {
        project_id: project_id.into(),
        name: name.into(),
        authoring_mode: "reviewed".into(),
        base_ref_kind: "project_default".into(),
        base_branch_mode: "isolated".into(),
        base_branch: None,
    }
}

#[tokio::test]
async fn draft_request_validates_before_persist_and_requires_project() {
    let state = AppState::new_test();
    let mut input = draft_input("missing", "Nightly");
    assert!(
        request_remote_automation_draft_for_state(&state, input.clone())
            .await
            .is_err()
    );
    input.authoring_mode = "invalid".into();
    assert!(request_remote_automation_draft_for_state(&state, input)
        .await
        .is_err());
    let mut input = draft_input("missing", "Nightly");
    input.base_ref_kind = "invalid".into();
    assert!(request_remote_automation_draft_for_state(&state, input)
        .await
        .is_err());
    let mut input = draft_input("missing", "Nightly");
    input.base_branch_mode = "invalid".into();
    assert!(request_remote_automation_draft_for_state(&state, input)
        .await
        .is_err());
    assert!(
        request_remote_automation_draft_for_state(&state, draft_input("missing", "  "))
            .await
            .is_err()
    );
    assert!(state
        .remote_automation_draft_request_repo
        .claim_pending(Utc::now())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn draft_request_deduplicates_same_name_and_allows_independent_names() {
    let state = AppState::new_test();
    let project = Project::new("Remote drafts".into(), "/tmp/remote-drafts".into());
    state.project_repo.create(project.clone()).await.unwrap();
    let first = request_remote_automation_draft_for_state(
        &state,
        draft_input(project.id.as_str(), "Nightly"),
    )
    .await
    .unwrap();
    let duplicate = request_remote_automation_draft_for_state(
        &state,
        draft_input(project.id.as_str(), "Nightly"),
    )
    .await
    .unwrap();
    let independent = request_remote_automation_draft_for_state(
        &state,
        draft_input(project.id.as_str(), "Weekly"),
    )
    .await
    .unwrap();
    assert_eq!(duplicate.request_id, first.request_id);
    assert_eq!(duplicate.automation_id, first.automation_id);
    assert!(duplicate.deduplicated);
    assert_ne!(independent.request_id, first.request_id);
    assert_ne!(independent.automation_id, first.automation_id);
}

fn run(automation_id: &AutomationId, judge_state: AutomationJudgeState) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string("remote-run-1"),
        automation_id: automation_id.clone(),
        run_index: 1,
        status: AutomationRunStatus::Merged,
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

async fn request(
    state: &AppState,
    automation_id: &str,
    kind: RemoteAutomationRunKind,
) -> Result<super::remote_automation_commands::RemoteAutomationRunIntentResponse, String> {
    request_remote_automation_run_for_state(
        state,
        RequestRemoteAutomationRunInput {
            automation_id: automation_id.into(),
            kind,
            expected_run_id: None,
        },
    )
    .await
}

#[tokio::test]
async fn request_rejects_missing_wrong_status_and_plan_gate_without_persisting() {
    let state = AppState::new_test();
    assert_eq!(
        request(&state, "missing", RemoteAutomationRunKind::RunNow)
            .await
            .unwrap_err(),
        REMOTE_AUTOMATION_RUN_NOT_FOUND
    );
    let draft = automation("draft", AutomationStatus::Draft);
    state.automation_repo.create(draft.clone()).await.unwrap();
    assert_eq!(
        request(&state, draft.id.as_str(), RemoteAutomationRunKind::RunNow)
            .await
            .unwrap_err(),
        REMOTE_AUTOMATION_RUN_AUTHORITY_CHANGED
    );
    let mut gated = automation("gated", AutomationStatus::Paused);
    gated.paused_reason_code = Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.into());
    state.automation_repo.create(gated.clone()).await.unwrap();
    assert_eq!(
        request(&state, gated.id.as_str(), RemoteAutomationRunKind::RunNow)
            .await
            .unwrap_err(),
        REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED
    );
    assert!(state
        .remote_automation_run_request_repo
        .claim_pending(Utc::now())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn retry_judge_rejects_healthy_latest_run_without_persisting() {
    let state = AppState::new_test();
    let automation = automation("healthy", AutomationStatus::Active);
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run(&automation.id, AutomationJudgeState::Done))
        .await
        .unwrap();
    assert_eq!(
        request(
            &state,
            automation.id.as_str(),
            RemoteAutomationRunKind::RetryJudge
        )
        .await
        .unwrap_err(),
        REMOTE_AUTOMATION_RUN_ALREADY_SETTLED
    );
    assert!(state
        .remote_automation_run_request_repo
        .claim_pending(Utc::now())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn request_deduplicates_same_kind_but_allows_independent_kinds() {
    let state = AppState::new_test();
    let automation = automation("dedup", AutomationStatus::Active);
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run(&automation.id, AutomationJudgeState::Failed))
        .await
        .unwrap();
    let first = request(
        &state,
        automation.id.as_str(),
        RemoteAutomationRunKind::RunNow,
    )
    .await
    .unwrap();
    let duplicate = request(
        &state,
        automation.id.as_str(),
        RemoteAutomationRunKind::RunNow,
    )
    .await
    .unwrap();
    let retry = request(
        &state,
        automation.id.as_str(),
        RemoteAutomationRunKind::RetryJudge,
    )
    .await
    .unwrap();
    assert_eq!(duplicate.request_id, first.request_id);
    assert!(duplicate.deduplicated);
    assert_ne!(retry.request_id, first.request_id);
    assert!(!retry.deduplicated);
}
