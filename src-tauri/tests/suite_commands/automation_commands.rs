use chrono::Utc;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::automation_commands::{
    cancel_automation_run, create_automation_draft, delete_automation, get_automation,
    list_automations, pause_automation, resume_automation, skip_automation_judge, stop_automation,
    trigger_automation_run_now, update_automation_settings, AutomationIdInput,
    AutomationRunScopedInput, CreateAutomationDraftInput, ListAutomationsInput,
    PauseAutomationInput, UpdateAutomationSettingsInput,
};
use ralphx_lib::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
};
use serde_json::json;
use tauri::Manager;

fn automation_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

fn active_automation(id: &str) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: ProjectId::from_string("project-1".to_string()),
        name: format!("Automation {id}"),
        status: AutomationStatus::Active,
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
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1 prompt".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        created_at: now,
        updated_at: now,
    }
}

fn automation_run(
    id: &str,
    automation_id: &AutomationId,
    run_index: i64,
    status: AutomationRunStatus,
    judge_state: AutomationJudgeState,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: automation_id.clone(),
        run_index,
        status,
        judge_state,
        judge_lease_expires_at: None,
        conversation_id: None,
        run_prompt: format!("Run {run_index} prompt"),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: String::new(),
        base_from_run_id: None,
        branch_name: Some(format!("ralphx/run-{run_index}")),
        pr_number: Some(100 + run_index),
        pr_url: None,
        pr_title: None,
        pr_head_ref_name: Some(format!("ralphx/run-{run_index}")),
        pr_base_ref_name: Some("main".to_string()),
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

fn continue_verdict(next_prompt: &str) -> String {
    json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "Continue with the next scoped automation run.",
        "confidence": 0.8,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": next_prompt,
        "nextBaseBranch": "automation_base"
    })
    .to_string()
}

#[tokio::test]
async fn ipc_contract_automation_command_wrappers_drive_draft_listing_and_controls() {
    let app = automation_command_app();

    let draft = create_automation_draft(
        CreateAutomationDraftInput {
            project_id: " project-1 ".to_string(),
            name: Some("Nightly automation".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("draft should create");
    assert_eq!(draft.automation.name, "Nightly automation");

    let listed = list_automations(
        Some(ListAutomationsInput {
            project_id: Some(" project-1 ".to_string()),
        }),
        app.state::<AppState>(),
    )
    .await
    .expect("list should succeed");
    assert_eq!(listed.len(), 1);

    let detail = get_automation(
        AutomationIdInput {
            id: format!(" {} ", draft.automation.id),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("detail should load");
    assert_eq!(detail.automation.id, draft.automation.id);

    let updated = update_automation_settings(
        UpdateAutomationSettingsInput {
            id: draft.automation.id.clone(),
            name: Some("Renamed nightly automation".to_string()),
            max_runs: Some(9),
            max_consecutive_failures: Some(2),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("settings should update");
    assert_eq!(updated.name, "Renamed nightly automation");
    assert_eq!(updated.max_runs, 9);

    let active = active_automation("automation-controls");
    app.state::<AppState>()
        .automation_repo
        .create(active.clone())
        .await
        .expect("active automation persists");
    let paused = pause_automation(
        PauseAutomationInput {
            id: active.id.as_str().to_string(),
            reason_code: None,
            reason_detail: Some("user requested pause".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("pause should succeed");
    assert_eq!(paused.status, "paused");
    assert_eq!(paused.paused_reason_code.as_deref(), Some("user_paused"));

    let resumed = resume_automation(
        AutomationIdInput {
            id: active.id.as_str().to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("resume should succeed");
    assert_eq!(resumed.status, "active");

    let stopped = stop_automation(
        AutomationIdInput {
            id: active.id.as_str().to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("stop should succeed");
    assert_eq!(stopped.status, "stopped");

    delete_automation(
        AutomationIdInput {
            id: active.id.as_str().to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("terminal automation should delete");
}

#[tokio::test]
async fn ipc_contract_automation_command_wrappers_drive_run_controls_and_scheduling() {
    let app = automation_command_app();
    let state = app.state::<AppState>();

    let cancel_target = active_automation("automation-cancel");
    state
        .automation_repo
        .create(cancel_target.clone())
        .await
        .expect("automation persists");
    let pending = automation_run(
        "run-cancel",
        &cancel_target.id,
        1,
        AutomationRunStatus::Pending,
        AutomationJudgeState::None,
    );
    state
        .automation_run_repo
        .create_run(pending.clone())
        .await
        .expect("run persists");
    let cancelled = cancel_automation_run(
        AutomationRunScopedInput {
            id: cancel_target.id.as_str().to_string(),
            run_id: pending.id.as_str().to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("cancel should succeed");
    assert_eq!(cancelled.status, "cancelled");

    let skip_target = active_automation("automation-skip");
    state
        .automation_repo
        .create(skip_target.clone())
        .await
        .expect("automation persists");
    let terminal = automation_run(
        "run-skip",
        &skip_target.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    state
        .automation_run_repo
        .create_run(terminal.clone())
        .await
        .expect("run persists");
    let skipped = skip_automation_judge(
        AutomationRunScopedInput {
            id: skip_target.id.as_str().to_string(),
            run_id: terminal.id.as_str().to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("skip judge should schedule successor");
    assert!(skipped.scheduled);

    let run_now_target = active_automation("automation-run-now");
    state
        .automation_repo
        .create(run_now_target.clone())
        .await
        .expect("automation persists");
    let mut judged = automation_run(
        "run-now-1",
        &run_now_target.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    judged.judge_verdict_json = Some(continue_verdict(
        "Implement the next automation item with focused tests and publish the follow-up PR.",
    ));
    state
        .automation_run_repo
        .create_run(judged)
        .await
        .expect("run persists");

    let run_now = trigger_automation_run_now(
        AutomationIdInput {
            id: run_now_target.id.as_str().to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("run now should consume stored verdict");
    assert!(run_now.scheduled);
}
