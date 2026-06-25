use super::*;
use crate::application::{AppState, TeamService, TeamStateTracker};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceFollowupProvenance,
    AgentWorkspaceSourcePullRequest, ChatConversation, IdeationAnalysisBaseRefKind, ProjectId,
    Task, TaskId,
};
use crate::http_server::types::HttpServerState;
use axum::{extract::State, Json};
use std::sync::Arc;

fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

fn followup_request(
    origin_conversation_id: Option<String>,
) -> CreateFollowupAgentConversationRequest {
    CreateFollowupAgentConversationRequest {
        origin_conversation_id,
        source_task_id: Some(" task-1 ".to_string()),
        source_context_type: Some(" review ".to_string()),
        source_context_id: Some(" review-1 ".to_string()),
        source_agent_name: Some(" ralphx-execution-reviewer ".to_string()),
        title: "Investigate drift".to_string(),
        description: Some("Use this description".to_string()),
        initial_prompt: Some("Use this prompt".to_string()),
        spawn_reason: Some(" plan_drift ".to_string()),
        blocker_fingerprint: Some(" drift:task-1 ".to_string()),
        provider_harness: None,
        model_override: None,
        logical_effort: None,
    }
}

fn test_workspace(
    conversation: &ChatConversation,
    project_id: &ProjectId,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("abc123".to_string()),
        format!("agent/{}", conversation.id.as_str()),
        "/tmp/ralphx-followup-test".to_string(),
    )
}

async fn seed_project_conversation(app_state: &AppState) -> (ProjectId, ChatConversation) {
    let project_id = ProjectId::new();
    let conversation = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    (project_id, conversation)
}

#[test]
fn helper_normalizes_origin_and_source_pull_request_input() {
    assert_eq!(trim_optional(Some("  value  ")).as_deref(), Some("value"));
    assert_eq!(trim_optional(Some("   ")), None);

    let mut req = followup_request(None);
    req.source_context_type = Some("agent_conversation".to_string());
    req.source_context_id = Some(" conversation-1 ".to_string());
    assert_eq!(
        request_origin_conversation_id(&req).as_deref(),
        Some("conversation-1")
    );

    let input = source_pull_request_input(Some(AgentWorkspaceSourcePullRequest {
        number: 489,
        url: Some("https://github.test/pr/489".to_string()),
        title: Some("Patch coverage".to_string()),
        head_ref_name: "feature".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("abc123".to_string()),
    }))
    .unwrap();
    assert_eq!(input.number, 489);
    assert_eq!(input.head_ref_name, "feature");
    assert_eq!(input.base_ref_name.as_deref(), Some("main"));
}

#[test]
fn followup_prompt_records_origin_and_branch_context() {
    let project_id = ProjectId::new();
    let origin = ChatConversation::new_project(project_id);
    let prompt = followup_prompt(
        &followup_request(Some(origin.id.as_str())),
        &origin,
        Some("drift:task-1"),
    );

    assert!(prompt.contains("Create a visible follow-up Agent conversation in Ideation mode."));
    assert!(prompt.contains(&format!("Origin Agent conversation: {}", origin.id)));
    assert!(prompt.contains("Source agent: ralphx-execution-reviewer"));
    assert!(prompt.contains("Source task: task-1"));
    assert!(prompt.contains("Source context type: review"));
    assert!(prompt.contains("Source context ID: review-1"));
    assert!(prompt.contains("Reason: plan_drift"));
    assert!(prompt.contains("Blocker fingerprint: drift:task-1"));
    assert!(prompt.contains("Use this prompt"));
}

#[test]
fn followup_provenance_trims_optional_fields() {
    let project_id = ProjectId::new();
    let origin = ChatConversation::new_project(project_id);
    let provenance = followup_provenance(
        &followup_request(Some(origin.id.as_str())),
        &origin,
        Some("drift:task-1".to_string()),
    );

    assert_eq!(provenance.origin_conversation_id, origin.id);
    assert_eq!(provenance.source_task_id.as_deref(), Some("task-1"));
    assert_eq!(provenance.source_context_type.as_deref(), Some("review"));
    assert_eq!(provenance.source_context_id.as_deref(), Some("review-1"));
    assert_eq!(
        provenance.source_agent_name.as_deref(),
        Some("ralphx-execution-reviewer")
    );
    assert_eq!(provenance.spawn_reason.as_deref(), Some("plan_drift"));
    assert_eq!(
        provenance.blocker_fingerprint.as_deref(),
        Some("drift:task-1")
    );
}

#[tokio::test]
async fn create_followup_returns_existing_active_followup_for_same_blocker() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let task = app_state
        .task_repo
        .create(Task::new(project_id.clone(), "Source task".to_string()))
        .await
        .unwrap();
    let source_task_id = task.id.as_str().to_string();
    let followup_conversation = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(followup_conversation.clone())
        .await
        .unwrap();
    let followup_workspace = test_workspace(&followup_conversation, &project_id);
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(followup_workspace)
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .save_followup_provenance(
            &followup_conversation.id,
            AgentWorkspaceFollowupProvenance {
                origin_conversation_id: origin.id.clone(),
                source_task_id: Some(source_task_id.clone()),
                source_context_type: Some("review".to_string()),
                source_context_id: Some("review-1".to_string()),
                source_agent_name: Some("ralphx-execution-reviewer".to_string()),
                spawn_reason: Some("plan_drift".to_string()),
                blocker_fingerprint: Some("drift:task-1".to_string()),
            },
        )
        .await
        .unwrap();

    let response = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(Some(origin.id.as_str()));
        req.source_task_id = Some(source_task_id);
        req
    })
    .await
    .unwrap();

    assert!(response.reused_existing);
    assert_eq!(response.origin_conversation_id, origin.id.as_str());
    assert_eq!(response.conversation.id, followup_conversation.id.as_str());
    assert!(response.workspace.is_some());
    assert!(response.send_result.is_none());
}

#[tokio::test]
async fn create_followup_validates_origin_before_spawning_new_branch() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let missing_context = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(None);
        req.source_task_id = None;
        req
    })
    .await
    .err()
    .expect("missing context should be rejected");
    assert_eq!(missing_context.0, StatusCode::BAD_REQUEST);

    let task_conversation = ChatConversation::new_task(TaskId::new());
    app_state
        .chat_conversation_repo
        .create(task_conversation.clone())
        .await
        .unwrap();
    let wrong_context = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(Some(task_conversation.id.as_str()));
        req.source_task_id = None;
        req
    })
    .await
    .err()
    .expect("task conversation should be rejected as origin");
    assert_eq!(wrong_context.0, StatusCode::BAD_REQUEST);

    let (_project_id, origin) = seed_project_conversation(&app_state).await;
    let unavailable = create_followup_agent_conversation(
        State(state),
        Json({
            let mut req = followup_request(Some(origin.id.as_str()));
            req.source_task_id = None;
            req
        }),
    )
    .await
    .err()
    .expect("new branch creation needs an initialized app handle");
    assert_eq!(unavailable.0, StatusCode::SERVICE_UNAVAILABLE);
}
