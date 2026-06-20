use super::*;
<<<<<<< HEAD
use crate::application::{
    LinearIntegrationSettings, TicketingMutationResult, TicketingTicketIdentity,
    TicketingTransitionOption,
};
use crate::domain::integrations::{
    AtlassianIntegrationSettings, IntegrationValidationStatus, ProviderTicketOperation,
    ProviderTicketOperationKind, ProviderTicketOperationStatus,
};
=======
use std::sync::Arc;

use crate::application::{AppState, LinearIntegrationSettings, TeamService, TeamStateTracker};
use crate::commands::unified_chat_commands::StartAgentConversationInput;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, ChatConversationId, Project,
    ProjectId,
};
use crate::domain::integrations::{AtlassianIntegrationSettings, IntegrationValidationStatus};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;
>>>>>>> ralphx/ralphx/agent-8e4ac713

#[test]
fn provider_summaries_reflect_existing_integration_settings() {
    let jira = AtlassianIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Valid,
        jira_available: true,
        ..Default::default()
    };

    let linear = LinearIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Invalid,
        last_error: Some("Token rejected".to_string()),
        ..Default::default()
    };

    let jira_summary = jira_provider_summary(&jira);
    let linear_summary = linear_provider_summary(&linear);

    assert_eq!(jira_summary.provider, "jira");
    assert_eq!(jira_summary.connection_status, "connected");
    assert!(jira_summary.capabilities.supports_kanban);
    assert!(jira_summary.capabilities.status_write);
    assert!(jira_summary.capabilities.assignment_write);
    assert!(jira_summary.capabilities.comment_write);
    assert_eq!(linear_summary.provider, "linear");
    assert_eq!(linear_summary.connection_status, "error");
    assert!(!linear_summary.capabilities.status_write);
    assert_eq!(
        linear_summary.error_message.as_deref(),
        Some("Token rejected")
    );
}

#[test]
fn ticketing_columns_return_provider_neutral_defaults() {
    let columns = default_ticketing_columns();

    assert_eq!(columns.len(), 4);
    assert_eq!(columns[0].id, "todo");
    assert_eq!(columns[1].category, "in_progress");
    assert_eq!(columns[2].category, "done");
}

#[test]
fn provider_validation_rejects_unknown_ticketing_provider() {
    let error = validate_provider("github").expect_err("unknown provider should fail");

    assert!(error.contains("Unknown ticketing provider"));
}

<<<<<<< HEAD
#[test]
fn ticket_identity_preserves_project_scope_for_mutation_service() {
    let identity = ticket_identity(
        "linear",
        &TicketRefInput {
            provider: "linear".to_string(),
            id: "issue-1".to_string(),
            key: Some("LIN-1".to_string()),
        },
        Some("project-1".to_string()),
    );

    assert_eq!(identity.provider, "linear");
    assert_eq!(identity.id, "issue-1");
    assert_eq!(identity.key.as_deref(), Some("LIN-1"));
    assert_eq!(identity.local_project_id.as_deref(), Some("project-1"));
}

#[test]
fn mutation_response_maps_operation_status_and_linked_flag() {
    let now = chrono::Utc::now();
    let response = ticket_mutation_response(TicketingMutationResult {
        ticket: TicketingTicketIdentity {
            provider: "jira".to_string(),
            id: "10001".to_string(),
            key: Some("JRA-1".to_string()),
            local_project_id: Some("project-1".to_string()),
        },
        operation: ProviderTicketOperation {
            id: "operation-1".to_string(),
            provider: "jira".to_string(),
            external_kind: "jira".to_string(),
            external_id: "JRA-1".to_string(),
            external_key: Some("JRA-1".to_string()),
            link_id: Some("link-1".to_string()),
            local_project_id: Some("project-1".to_string()),
            operation: ProviderTicketOperationKind::Transition,
            client_operation_id: "client-op-1".to_string(),
            status: ProviderTicketOperationStatus::Succeeded,
            provider_operation_id: Some("31".to_string()),
            error_message: None,
            metadata_json: None,
            last_attempt_at: Some(now),
            completed_at: Some(now),
            created_at: now,
            updated_at: now,
        },
        idempotent: false,
        transition: Some(TicketingTransitionOption {
            to_state_id: "done".to_string(),
            provider_transition_id: Some("31".to_string()),
            name: "Done".to_string(),
            category: "done".to_string(),
            disabled_reason: None,
        }),
        comment: None,
    });

    assert_eq!(response.ticket_ref.provider, "jira");
    assert_eq!(response.ticket_ref.key.as_deref(), Some("JRA-1"));
    assert_eq!(response.operation.operation, "transition");
    assert_eq!(response.operation.status, "succeeded");
    assert!(response.operation.linked);
    assert_eq!(
        response.transition.unwrap().provider_transition_id.as_deref(),
        Some("31")
    );
=======
fn build_ticketing_start_app(
    state: AppState,
    execution_state: Arc<ExecutionState>,
) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(execution_state)
        .manage(Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        ))))
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

async fn seed_ticketing_project(state: &AppState, id: &str) -> ProjectId {
    let project_id = ProjectId::from_string(id.to_string());
    let mut project = Project::new(
        format!("{id} project"),
        format!("/tmp/{id}-project-worktree"),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    project_id
}

fn ticket_start_input(
    project_id: &ProjectId,
    ticket_ref: TicketRefInput,
) -> StartRalphxWorkFromTicketInput {
    StartRalphxWorkFromTicketInput {
        start: StartAgentConversationInput {
            project_id: project_id.as_str().to_string(),
            content: "Start work from the ticket".to_string(),
            conversation_id: None,
            provider_harness: None,
            model_override: None,
            logical_effort: None,
            mode: Some("chat".to_string()),
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
        },
        ticket_ref,
    }
}

#[tokio::test]
async fn start_work_from_ticket_queues_message_and_links_jira_after_successful_start() {
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-start-jira").await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_ticketing_start_app(state, Arc::clone(&execution_state));

    let response = start_ralphx_work_from_ticket(
        ticket_start_input(
            &project_id,
            TicketRefInput {
                provider: "jira".to_string(),
                id: "10001".to_string(),
                key: Some("RAL-42".to_string()),
            },
        ),
        app.state(),
        app.state(),
        app.state(),
        app.handle().clone(),
    )
    .await
    .expect("ticket start should succeed while paused by queuing the send");

    assert_eq!(response.conversation.context_id, project_id.as_str());
    assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
    assert!(response.workspace.is_none());
    assert!(response.send_result.was_queued);
    let queued = app
        .state::<AppState>()
        .message_queue
        .get_queued(ChatContextType::Project, response.conversation.id.as_str());
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].composer_integration_references.len(), 1);
    assert_eq!(
        queued[0].composer_integration_references[0].provider,
        "atlassian"
    );
    assert_eq!(queued[0].composer_integration_references[0].kind, "jira");
    assert_eq!(
        queued[0].composer_integration_references[0].key.as_deref(),
        Some("RAL-42")
    );

    let conversation_id = ChatConversationId::from_string(response.conversation.id.clone());
    let linked = app
        .state::<AppState>()
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("jira link lookup should succeed")
        .expect("jira issue should be linked after start succeeds");
    assert_eq!(linked.issue_key, "RAL-42");
    assert_eq!(linked.issue_id.as_deref(), Some("10001"));
    assert!(linked.manually_assigned);
}

#[tokio::test]
async fn start_work_from_ticket_does_not_link_when_existing_conversation_is_invalid() {
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-start-link-rollback").await;
    let other_project_id = seed_ticketing_project(&state, "ticket-start-link-rollback-other").await;
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(other_project_id))
        .await
        .expect("conversation should be created");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_ticketing_start_app(state, execution_state);
    let mut input = ticket_start_input(
        &project_id,
        TicketRefInput {
            provider: "jira".to_string(),
            id: "10002".to_string(),
            key: Some("RAL-43".to_string()),
        },
    );
    input.start.conversation_id = Some(conversation.id.as_str().to_string());

    let error = start_ralphx_work_from_ticket(
        input,
        app.state(),
        app.state(),
        app.state(),
        app.handle().clone(),
    )
    .await
    .expect_err("start should fail before ticket link upsert");

    assert!(error.contains("does not belong to project"));
    let linked = app
        .state::<AppState>()
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("jira link lookup should succeed");
    assert!(linked.is_none());
}

#[tokio::test]
async fn get_ticket_associations_returns_linked_agent_conversations() {
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-associations-jira").await;
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.set_title("Started from RX-77");
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be created");
    let ticket_ref = TicketRefInput {
        provider: "jira".to_string(),
        id: "10077".to_string(),
        key: Some("RX-77".to_string()),
    };
    let ticket_reference = ticket_ref_to_composer_reference("jira", &ticket_ref);
    link_started_ticket_to_conversation(
        &state,
        "jira",
        &conversation.id,
        &project_id,
        &ticket_reference,
    )
    .await
    .expect("ticket link should be persisted");
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let associations = get_ticket_associations(
        "jira".to_string(),
        ticket_ref,
        project_id.as_str().to_string(),
        app.state(),
    )
    .await
    .expect("ticket associations should load");

    assert_eq!(associations.conversations.len(), 1);
    let linked = &associations.conversations[0];
    assert_eq!(linked.id, conversation.id.as_str());
    assert_eq!(linked.title, "Started from RX-77");
    assert_eq!(linked.status.as_deref(), Some("edit"));
    assert!(linked.active);
    assert_eq!(linked.deep_link.view, "agents");
    assert_eq!(linked.deep_link.id, conversation.id.as_str());
>>>>>>> ralphx/ralphx/agent-8e4ac713
}
