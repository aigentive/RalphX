use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    Json,
};
use chrono::Utc;

use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationStatus,
    ChatConversation, ChatConversationId, ProjectId,
};

fn caller_headers(conversation_id: &ChatConversationId) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        CALLER_SESSION_ID_HEADER,
        HeaderValue::from_str(&conversation_id.as_str()).unwrap(),
    );
    headers
}

fn automation(id: &AutomationId, project_id: ProjectId, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    Automation {
        id: id.clone(),
        project_id,
        name: "Automation 1".to_string(),
        status,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Implement the plan".to_string(),
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

async fn seed_bound_conversation(
    app_state: &AppState,
) -> (ProjectId, AutomationId, ChatConversation) {
    let project_id = ProjectId::new();
    let automation_id = AutomationId::from_string("automation-1");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.automation_id = Some(automation_id.clone());
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    (project_id, automation_id, conversation)
}

#[tokio::test]
async fn get_and_update_automation_use_server_bound_conversation() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(conversation.id);
    app_state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());

    let Json(detail) = get_automation(State(state.clone()), caller_headers(&conversation.id))
        .await
        .unwrap();
    assert_eq!(detail.automation.id, automation_id.as_str());
    assert!(detail.runs.is_empty());

    let Json(updated) = update_automation(
        State(state),
        caller_headers(&conversation.id),
        Json(UpdateAutomationRequest {
            name: Some("Renamed automation".to_string()),
            max_runs: Some(9),
            max_consecutive_failures: None,
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert_eq!(updated.name, "Renamed automation");
    assert_eq!(updated.max_runs, 9);
    assert_eq!(
        app_state
            .automation_repo
            .get_by_id(&automation_id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Renamed automation"
    );
    assert_eq!(
        app_state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap()
            .title
            .as_deref(),
        Some("Renamed automation")
    );
}

#[tokio::test]
async fn update_automation_persists_config_fields_for_bound_conversation() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(conversation.id);
    automation.goal_prompt = String::new();
    automation.first_run_prompt = None;
    automation.base_ref = String::new();
    app_state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());

    let Json(updated) = update_automation(
        State(state),
        caller_headers(&conversation.id),
        Json(UpdateAutomationRequest {
            goal_prompt: Some("Ship the migration".to_string()),
            first_run_prompt: Some("Implement item 1 in a scoped PR.".to_string()),
            base_ref_kind: Some("local_branch".to_string()),
            base_ref: Some("main".to_string()),
            goal_items_json: Some(
                r#"[{"id":"phase-1","title":"Build shared context model","status":"pending"}]"#
                    .to_string(),
            ),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert_eq!(updated.goal_prompt, "Ship the migration");
    assert_eq!(
        updated.first_run_prompt.as_deref(),
        Some("Implement item 1 in a scoped PR.")
    );
    assert_eq!(updated.base_ref_kind, "local_branch");
    assert_eq!(updated.base_ref, "main");
    assert_eq!(
        updated.goal_items_json.as_deref(),
        Some(r#"[{"id":"phase-1","title":"Build shared context model","status":"pending"}]"#),
    );

    let stored = app_state
        .automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.goal_prompt, "Ship the migration");
    assert_eq!(
        stored.first_run_prompt.as_deref(),
        Some("Implement item 1 in a scoped PR.")
    );
    assert_eq!(stored.base_ref, "main");
    assert_eq!(
        stored.goal_items_json.as_deref(),
        Some(r#"[{"id":"phase-1","title":"Build shared context model","status":"pending"}]"#),
    );
}

#[tokio::test]
async fn update_automation_persists_plan_gate_settings_for_bound_conversation() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(conversation.id);
    app_state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());

    let Json(updated) = update_automation(
        State(state),
        caller_headers(&conversation.id),
        Json(UpdateAutomationRequest {
            plan_approval_mode: Some("automatic".to_string()),
            pr_merge_mode: Some("automatic".to_string()),
            plan_deep_verification: Some(true),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert_eq!(updated.plan_approval_mode, "automatic");
    assert_eq!(updated.pr_merge_mode, "automatic");
    assert!(updated.plan_deep_verification);

    let stored = app_state
        .automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.plan_approval_mode,
        AutomationPlanApprovalMode::Automatic
    );
    assert_eq!(stored.pr_merge_mode, AutomationPrMergeMode::Automatic);
    assert!(stored.plan_deep_verification);
}

#[tokio::test]
async fn update_automation_materializes_spec_content_for_bound_conversation() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(conversation.id.clone());
    app_state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());

    let Json(updated) = update_automation(
        State(state),
        caller_headers(&conversation.id),
        Json(UpdateAutomationRequest {
            spec_content: Some("# Automation spec\n\nPhase 1: implement it.".to_string()),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    let spec_id = updated.spec_artifact_id.expect("spec artifact linked");
    let artifact = app_state
        .artifact_repo
        .get_by_id(&crate::domain::entities::ArtifactId::from_string(
            spec_id.clone(),
        ))
        .await
        .unwrap()
        .expect("spec artifact persisted");
    match &artifact.content {
        crate::domain::entities::ArtifactContent::Inline { text } => {
            assert!(text.contains("Phase 1: implement it."))
        }
        other => panic!("expected inline spec content, got {other:?}"),
    }

    let stored = app_state
        .automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.spec_artifact_id.as_deref(), Some(spec_id.as_str()));
}

#[tokio::test]
async fn update_automation_config_rejects_mismatched_conversation_binding() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    // Binding points at a different setup conversation than the caller.
    automation.setup_conversation_id = Some(ChatConversationId::new());
    app_state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());

    let error = update_automation(
        State(state),
        caller_headers(&conversation.id),
        Json(UpdateAutomationRequest {
            goal_prompt: Some("Should not persist".to_string()),
            ..Default::default()
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.status, StatusCode::FORBIDDEN);
    assert!(error
        .message
        .as_deref()
        .unwrap_or("")
        .contains("automation_conversation_mismatch"));
    // Config write must not have leaked through the rejected binding.
    assert_eq!(
        app_state
            .automation_repo
            .get_by_id(&automation_id)
            .await
            .unwrap()
            .unwrap()
            .goal_prompt,
        "Implement the plan"
    );
}

#[tokio::test]
async fn finalize_rejects_mismatched_conversation_binding() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(ChatConversationId::new());
    app_state.automation_repo.create(automation).await.unwrap();
    let state = HttpServerState::new_test(app_state);

    let error = finalize_automation(State(state), caller_headers(&conversation.id))
        .await
        .unwrap_err();

    assert_eq!(error.status, StatusCode::FORBIDDEN);
    assert!(error
        .message
        .as_deref()
        .unwrap_or("")
        .contains("automation_conversation_mismatch"));
}

#[tokio::test]
async fn finalize_activates_complete_bound_draft() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(conversation.id);
    app_state.automation_repo.create(automation).await.unwrap();
    let state = HttpServerState::new_test(app_state.clone());

    let Json(finalized) = finalize_automation(State(state), caller_headers(&conversation.id))
        .await
        .unwrap();

    assert_eq!(finalized.status, "active");
    assert_eq!(
        app_state
            .automation_repo
            .get_by_id(&automation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Active
    );
}
