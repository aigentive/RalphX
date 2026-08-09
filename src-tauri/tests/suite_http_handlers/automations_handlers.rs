use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
};
use chrono::Utc;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Automation, AutomationId,
    AutomationJudgeState, AutomationPlanApprovalMode, AutomationPlanJudgeState,
    AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, ChatConversation, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use ralphx_lib::http_server::delegation::DelegationService;
use ralphx_lib::http_server::handlers::automations::{
    get_automation_publish_status, restart_automation_for_setup_agent, CALLER_SESSION_ID_HEADER,
};
use ralphx_lib::http_server::types::HttpServerState;

fn test_state() -> HttpServerState {
    HttpServerState {
        app_state: Arc::new(AppState::new_test()),
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Arc::new(DelegationService::new()),
        external_mcp_supervisor: None,
    }
}

fn stopped_automation(
    id: &str,
    setup_conversation_id: ralphx_lib::domain::entities::ChatConversationId,
) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: ProjectId::from_string("project-1".to_string()),
        name: "Stopped automation".to_string(),
        status: AutomationStatus::Stopped,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Keep implementing the automation goal".to_string(),
        setup_conversation_id: Some(setup_conversation_id),
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: None,
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: "main".to_string(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: Some(
            r#"[{"id":"phase-1","title":"Run 1","status":"pending"}]"#.to_string(),
        ),
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        max_runs: 25,
        max_consecutive_failures: 3,
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        first_run_prompt: Some("Continue the automation".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

fn cancelled_run(automation_id: &AutomationId) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string("run-cancelled"),
        automation_id: automation_id.clone(),
        run_index: 1,
        status: AutomationRunStatus::Cancelled,
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
        conversation_id: None,
        run_prompt: "Continue the automation".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: "main".to_string(),
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
        finished_at: Some(now),
        created_at: now,
        updated_at: now,
    }
}

fn workspace(
    conversation_id: &ChatConversationId,
    branch_name: &str,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        *conversation_id,
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Automation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        branch_name.to_string(),
        format!("/tmp/{branch_name}"),
    )
}

async fn bound_stopped_automation(
    state: &HttpServerState,
) -> (AutomationId, ChatConversationId, HeaderMap) {
    let automation_id = AutomationId::from_string("automation-publish");
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.automation_id = Some(automation_id.clone());
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    state
        .app_state
        .automation_repo
        .create(stopped_automation(automation_id.as_str(), conversation.id))
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        CALLER_SESSION_ID_HEADER,
        HeaderValue::from_str(&conversation.id.as_str()).unwrap(),
    );
    (automation_id, conversation.id, headers)
}

#[tokio::test]
async fn restart_route_requires_injected_caller_identity() {
    let error = restart_automation_for_setup_agent(State(test_state()), HeaderMap::new())
        .await
        .expect_err("caller identity must be injected");

    assert_eq!(error.status, StatusCode::FORBIDDEN);
    assert!(error
        .message
        .as_deref()
        .is_some_and(|message| message.contains("automation_caller_missing")));
}

#[tokio::test]
async fn restart_route_resolves_bound_automation_and_creates_fresh_run() {
    let state = test_state();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    let automation_id = AutomationId::from_string("automation-1");
    conversation.automation_id = Some(automation_id.clone());
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    state
        .app_state
        .automation_repo
        .create(stopped_automation(automation_id.as_str(), conversation.id))
        .await
        .unwrap();
    state
        .app_state
        .automation_run_repo
        .create_run(cancelled_run(&automation_id))
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        CALLER_SESSION_ID_HEADER,
        HeaderValue::from_str(&conversation.id.as_str()).unwrap(),
    );

    let response = restart_automation_for_setup_agent(State(state.clone()), headers)
        .await
        .expect("bound setup conversation should restart its automation")
        .0;

    assert!(response.scheduled);
    let stored = state
        .app_state
        .automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Active);
    let runs = state
        .app_state
        .automation_run_repo
        .list_for_automation(&automation_id)
        .await
        .unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].status, AutomationRunStatus::Cancelled);
    assert_eq!(runs[1].status, AutomationRunStatus::Pending);
}

#[tokio::test]
async fn publish_status_prefers_bound_setup_workspace() {
    let state = test_state();
    let (_automation_id, setup_conversation_id, headers) = bound_stopped_automation(&state).await;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace(&setup_conversation_id, "automation-setup"))
        .await
        .unwrap();

    let response = get_automation_publish_status(State(state), headers)
        .await
        .expect("setup workspace should be the trusted publish target")
        .0;

    assert_eq!(
        response.workspace.conversation_id,
        setup_conversation_id.as_str()
    );
}

#[tokio::test]
async fn publish_status_falls_back_to_latest_run_workspace() {
    let state = test_state();
    let (automation_id, _setup_conversation_id, headers) = bound_stopped_automation(&state).await;
    let older_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "project-1".to_string(),
        )))
        .await
        .unwrap();
    let latest_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "project-1".to_string(),
        )))
        .await
        .unwrap();
    for (id, index, conversation_id, branch) in [
        ("run-older", 1, older_conversation.id, "automation-run-old"),
        (
            "run-latest",
            2,
            latest_conversation.id,
            "automation-run-latest",
        ),
    ] {
        let mut run = cancelled_run(&automation_id);
        run.id = AutomationRunId::from_string(id);
        run.run_index = index;
        run.conversation_id = Some(conversation_id);
        state
            .app_state
            .automation_run_repo
            .create_run(run)
            .await
            .unwrap();
        state
            .app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace(&conversation_id, branch))
            .await
            .unwrap();
    }

    let response = get_automation_publish_status(State(state), headers)
        .await
        .expect("latest run workspace should be the trusted fallback")
        .0;

    assert_eq!(
        response.workspace.conversation_id,
        latest_conversation.id.as_str()
    );
}

#[tokio::test]
async fn publish_status_fails_closed_without_bound_workspace() {
    let state = test_state();
    let (_automation_id, _setup_conversation_id, headers) = bound_stopped_automation(&state).await;

    let error = get_automation_publish_status(State(state), headers)
        .await
        .expect_err("no arbitrary publish target may be selected");

    assert_eq!(error.0, StatusCode::NOT_FOUND);
    assert_eq!(
        error.1 .0.get("error").and_then(serde_json::Value::as_str),
        Some("Automation has no publishable workspace")
    );
}
