use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime, State};

use crate::application::{
    agent_conversation_jira_issue, agent_conversation_linear_issue,
    agent_conversation_start_service::{
        AgentConversationStartDeps, AgentConversationStartService, StartAgentConversationInput,
    },
    AppState, AtlassianResourceContent, AtlassianResourceKind, AtlassianResourceSummary,
    LinearComment, LinearIntegrationSettings, LinearIssueContent, LinearIssueSummary,
    LinearWorkflowState,
    TauriTicketingEventSink, TeamService, TicketAssignRequest, TicketCommentRequest,
    TicketTransitionRequest,
    TicketingCommentResult, TicketingMutationResult, TicketingPersonResult, TicketingService,
    TicketingTicketIdentity, TicketingTransitionOption,
};
use crate::commands::unified_chat_commands::{
    agent_conversation_response_for_state, agent_workspace_response_for_state,
    SendAgentMessageResponse, StartAgentConversationResponse,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationJiraIssueLink, AgentConversationLinearIssueLink, ChatContextType,
    ChatConversation, ChatConversationId, ProjectId,
};
use crate::domain::integrations::{
    AtlassianIntegrationSettings, IntegrationValidationStatus, ProviderTicketOperation,
};
use crate::domain::services::{
    jira_reference_from_composer_reference, ComposerIntegrationReference,
    ComposerJiraReferenceMetadata,
};

const PROVIDER_JIRA: &str = "jira";
const PROVIDER_LINEAR: &str = "linear";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingCapabilitiesResponse {
    pub supports_boards: bool,
    pub supports_kanban: bool,
    pub kanban_write: bool,
    pub status_write: bool,
    pub assignment_write: bool,
    pub comment_write: bool,
    pub freshness: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingProviderSummaryResponse {
    pub provider: String,
    pub label: String,
    pub enabled: bool,
    pub connection_status: String,
    pub capabilities: TicketingCapabilitiesResponse,
    pub fetched_at: Option<String>,
    pub stale_at: Option<String>,
    pub permission_message: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingContainerResponse {
    pub provider: String,
    pub id: String,
    pub key: Option<String>,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub ticket_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingColumnResponse {
    pub id: String,
    pub name: String,
    pub category: String,
    pub order: usize,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketRefInput {
    pub provider: String,
    pub id: String,
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRalphxWorkFromTicketInput {
    #[serde(flatten)]
    pub start: StartAgentConversationInput,
    pub ticket_ref: TicketRefInput,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketStateResponse {
    pub id: String,
    pub name: String,
    pub category: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingPersonResponse {
    pub id: Option<String>,
    pub name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketSummaryResponse {
    #[serde(rename = "ref")]
    pub ref_: TicketRefInput,
    pub title: String,
    pub state: TicketStateResponse,
    pub assignee: Option<TicketingPersonResponse>,
    pub reporter: Option<TicketingPersonResponse>,
    pub labels: Vec<String>,
    pub project: Option<String>,
    pub priority: Option<String>,
    pub updated_at: String,
    pub url: Option<String>,
    pub association_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketCommentResponse {
    pub id: Option<String>,
    pub author: Option<TicketingPersonResponse>,
    pub body_markdown: String,
    pub body_text: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentResponse {
    pub id: Option<String>,
    pub filename: String,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketTransitionOptionResponse {
    pub to_state_id: String,
    pub provider_transition_id: Option<String>,
    pub name: String,
    pub category: String,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketDetailResponse {
    #[serde(flatten)]
    pub summary: TicketSummaryResponse,
    pub description_markdown: Option<String>,
    pub description_text: Option<String>,
    pub acceptance_criteria_markdown: Option<String>,
    pub comments: Vec<TicketCommentResponse>,
    pub attachments: Vec<TicketAttachmentResponse>,
    pub transitions: Vec<TicketTransitionOptionResponse>,
    pub fetched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketPageResponse {
    pub items: Vec<TicketSummaryResponse>,
    pub next_cursor: Option<String>,
    pub total: Option<usize>,
    pub fetched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketDeepLinkResponse {
    pub view: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAssociationItemResponse {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub status: Option<String>,
    pub active: bool,
    pub deep_link: TicketDeepLinkResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAssociationsResponse {
    pub tasks: Vec<TicketAssociationItemResponse>,
    pub proposals: Vec<TicketAssociationItemResponse>,
    pub sessions: Vec<TicketAssociationItemResponse>,
    pub conversations: Vec<TicketAssociationItemResponse>,
    pub pull_requests: Vec<TicketAssociationItemResponse>,
    pub checks: Vec<TicketAssociationItemResponse>,
    pub qa: Vec<TicketAssociationItemResponse>,
    pub specs: Vec<TicketAssociationItemResponse>,
    pub fetched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTicketsResponse {
    pub refreshed_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionTicketStatusInput {
    pub provider: String,
    pub ticket_ref: TicketRefInput,
    pub to_state_id: String,
    pub provider_transition_id: Option<String>,
    pub client_operation_id: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignTicketInput {
    pub provider: String,
    pub ticket_ref: TicketRefInput,
    pub client_operation_id: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddTicketCommentInput {
    pub provider: String,
    pub ticket_ref: TicketRefInput,
    pub body_markdown: String,
    pub client_operation_id: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketOperationResponse {
    pub id: String,
    pub operation: String,
    pub client_operation_id: String,
    pub status: String,
    pub provider_operation_id: Option<String>,
    pub error_message: Option<String>,
    pub linked: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketMutationResponse {
    pub ticket_ref: TicketRefInput,
    pub operation: TicketOperationResponse,
    pub idempotent: bool,
    pub transition: Option<TicketTransitionOptionResponse>,
    pub assignee: Option<TicketingPersonResponse>,
    pub comment: Option<TicketCommentResponse>,
    pub refreshed_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketFiltersInput {
    pub text: Option<String>,
    pub assignee: Option<String>,
    pub state_ids: Option<Vec<String>>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTicketsQuery {
    pub provider: String,
    pub project_id: Option<String>,
    pub container_id: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub filters: Option<TicketFiltersInput>,
    pub sort: Option<String>,
}

#[tauri::command]
pub async fn list_ticketing_providers(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TicketingProviderSummaryResponse>, String> {
    let _ = project_id;
    let jira = state.atlassian_integration_service.get_settings().await?;
    let linear = state.linear_integration_service.get_settings().await?;
    Ok(vec![
        jira_provider_summary(&jira),
        linear_provider_summary(&linear),
    ])
}

#[tauri::command]
pub fn list_ticketing_containers(
    provider: String,
    project_id: Option<String>,
) -> Result<Vec<TicketingContainerResponse>, String> {
    validate_provider(&provider)?;
    let _ = project_id;
    Ok(Vec::new())
}

#[tauri::command]
pub async fn list_ticketing_columns(
    provider: String,
    container_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TicketingColumnResponse>, String> {
    validate_provider(&provider)?;
    match provider.as_str() {
        PROVIDER_LINEAR => state
            .linear_integration_service
            .list_workflow_states(container_id.as_deref())
            .await
            .map(|states| {
                states
                    .into_iter()
                    .enumerate()
                    .map(|(index, state)| linear_workflow_state_to_column(state, index))
                    .collect()
            }),
        _ => {
            let _ = container_id;
            Ok(default_ticketing_columns())
        }
    }
}

#[tauri::command]
pub async fn list_tickets(
    query: ListTicketsQuery,
    state: State<'_, AppState>,
) -> Result<TicketPageResponse, String> {
    validate_provider(&query.provider)?;
    let _ = (
        &query.project_id,
        &query.container_id,
        &query.cursor,
        &query.sort,
    );
    let limit = query.limit.unwrap_or(25).clamp(1, 40);
    let text = query
        .filters
        .as_ref()
        .and_then(|filters| filters.text.as_deref())
        .unwrap_or("")
        .trim()
        .to_string();
    let fetched_at = now_string();
    let items: Vec<TicketSummaryResponse> = match query.provider.as_str() {
        PROVIDER_JIRA => state
            .atlassian_integration_service
            .search_resources(AtlassianResourceKind::Jira, &text, limit)
            .await?
            .into_iter()
            .map(jira_summary_to_ticket)
            .collect(),
        PROVIDER_LINEAR => state
            .linear_integration_service
            .search_issues(&text, limit)
            .await?
            .into_iter()
            .map(linear_summary_to_ticket)
            .collect(),
        _ => unreachable!("provider validated above"),
    };
    let items = filter_ticket_summaries(items, query.filters.as_ref());
    let items = hydrate_ticket_association_counts(
        state.inner(),
        &query.provider,
        query.project_id.as_deref(),
        items,
    )
    .await?;
    Ok(TicketPageResponse {
        total: Some(items.len()),
        items,
        next_cursor: None,
        fetched_at: Some(fetched_at),
    })
}

#[tauri::command]
pub async fn get_ticket_detail(
    provider: String,
    ticket_ref: TicketRefInput,
    state: State<'_, AppState>,
) -> Result<TicketDetailResponse, String> {
    validate_provider(&provider)?;
    let reference = ticket_ref_to_composer_reference(&provider, &ticket_ref);
    match provider.as_str() {
        PROVIDER_JIRA => state
            .atlassian_integration_service
            .fetch_resource_content(&reference)
            .await
            .map(jira_content_to_detail),
        PROVIDER_LINEAR => state
            .linear_integration_service
            .fetch_issue_content(&reference)
            .await
            .map(linear_content_to_detail),
        _ => unreachable!("provider validated above"),
    }
}

#[tauri::command]
pub async fn list_ticket_transitions(
    provider: String,
    ticket_ref: TicketRefInput,
    state: State<'_, AppState>,
) -> Result<Vec<TicketTransitionOptionResponse>, String> {
    validate_provider(&provider)?;
    ticketing_service_from_state(&state)
        .list_transitions(&ticket_identity(&provider, &ticket_ref, None))
        .await
        .map(|transitions| {
            transitions
                .into_iter()
                .map(ticket_transition_option_response)
                .collect()
        })
}

#[tauri::command]
pub async fn get_ticket_associations(
    provider: String,
    ticket_ref: TicketRefInput,
    project_id: String,
    state: State<'_, AppState>,
) -> Result<TicketAssociationsResponse, String> {
    validate_provider(&provider)?;
    let project_id = ProjectId::from_string(project_id);
    let ticket_reference = ticket_ref_to_composer_reference(&provider, &ticket_ref);
    let mut response = empty_associations();
    response.conversations = linked_agent_conversation_associations_for_ticket(
        state.inner(),
        &provider,
        &project_id,
        &ticket_reference,
    )
    .await?;
    Ok(response)
}

#[tauri::command]
pub async fn start_ralphx_work_from_ticket<R: Runtime + 'static>(
    mut input: StartRalphxWorkFromTicketInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    team_service: State<'_, Arc<TeamService>>,
    app: tauri::AppHandle<R>,
) -> Result<StartAgentConversationResponse, String> {
    let provider = input.ticket_ref.provider.clone();
    validate_provider(&provider)?;
    let project_id = ProjectId::from_string(input.start.project_id.clone());
    let ticket_reference = ticket_ref_to_composer_reference(&provider, &input.ticket_ref);
    let issue_reference = ensure_ticket_composer_reference(
        &mut input.start.composer_integration_references,
        ticket_reference,
    );

    let mut result = AgentConversationStartService::new(AgentConversationStartDeps {
        state: state.inner(),
        execution_state: execution_state.inner(),
        team_service: Some(team_service.inner().clone()),
        app_handle: app,
    })
    .start(input.start)
    .await?;

    link_started_ticket_to_conversation(
        state.inner(),
        &provider,
        &result.conversation.id,
        &project_id,
        &issue_reference,
    )
    .await?;

    let conversation_title = ticket_ref_label(&input.ticket_ref);
    state
        .chat_conversation_repo
        .update_title(&result.conversation.id, &conversation_title)
        .await
        .map_err(|error| error.to_string())?;
    result.conversation.title = Some(conversation_title);

    let workspace_response = match result.workspace {
        Some(workspace) => {
            Some(agent_workspace_response_for_state(state.inner(), workspace).await?)
        }
        None => None,
    };

    Ok(StartAgentConversationResponse {
        conversation: agent_conversation_response_for_state(state.inner(), result.conversation)
            .await?,
        workspace: workspace_response,
        send_result: SendAgentMessageResponse::from(result.send_result),
    })
}

#[tauri::command]
pub fn refresh_tickets(
    provider: String,
    container_id: Option<String>,
) -> Result<RefreshTicketsResponse, String> {
    validate_provider(&provider)?;
    let _ = container_id;
    Ok(RefreshTicketsResponse {
        refreshed_at: now_string(),
    })
}

fn ticket_ref_label(ticket_ref: &TicketRefInput) -> String {
    ticket_ref
        .key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(ticket_ref.id.as_str())
        .to_string()
}

#[tauri::command]
pub async fn transition_ticket_status(
    input: TransitionTicketStatusInput,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<TicketMutationResponse, String> {
    validate_provider(&input.provider)?;
    let result = ticketing_service_from_state_with_events(&state, app_handle)
        .transition_ticket_status(TicketTransitionRequest {
            ticket: ticket_identity(&input.provider, &input.ticket_ref, input.project_id.clone()),
            to_state_id: input.to_state_id,
            provider_transition_id: input.provider_transition_id,
            client_operation_id: input.client_operation_id,
        })
        .await?;
    Ok(ticket_mutation_response(result))
}

#[tauri::command]
pub async fn assign_ticket(
    input: AssignTicketInput,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<TicketMutationResponse, String> {
    validate_provider(&input.provider)?;
    let result = ticketing_service_from_state_with_events(&state, app_handle)
        .assign_ticket(TicketAssignRequest {
            ticket: ticket_identity(&input.provider, &input.ticket_ref, input.project_id.clone()),
            client_operation_id: input.client_operation_id,
        })
        .await?;
    Ok(ticket_mutation_response(result))
}

#[tauri::command]
pub async fn clear_ticket_assignee(
    input: AssignTicketInput,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<TicketMutationResponse, String> {
    validate_provider(&input.provider)?;
    let result = ticketing_service_from_state_with_events(&state, app_handle)
        .clear_ticket_assignee(TicketAssignRequest {
            ticket: ticket_identity(&input.provider, &input.ticket_ref, input.project_id.clone()),
            client_operation_id: input.client_operation_id,
        })
        .await?;
    Ok(ticket_mutation_response(result))
}

#[tauri::command]
pub async fn add_ticket_comment(
    input: AddTicketCommentInput,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<TicketMutationResponse, String> {
    validate_provider(&input.provider)?;
    let result = ticketing_service_from_state_with_events(&state, app_handle)
        .add_ticket_comment(TicketCommentRequest {
            ticket: ticket_identity(&input.provider, &input.ticket_ref, input.project_id.clone()),
            body_markdown: input.body_markdown,
            client_operation_id: input.client_operation_id,
        })
        .await?;
    Ok(ticket_mutation_response(result))
}

fn jira_provider_summary(
    settings: &AtlassianIntegrationSettings,
) -> TicketingProviderSummaryResponse {
    let is_valid = settings.validation_status == IntegrationValidationStatus::Valid;
    let connection_status = if !settings.enabled {
        "disconnected"
    } else if !is_valid {
        "error"
    } else if !settings.jira_available {
        "permission_limited"
    } else {
        "connected"
    };
    TicketingProviderSummaryResponse {
        provider: PROVIDER_JIRA.to_string(),
        label: "Jira".to_string(),
        enabled: settings.enabled && is_valid && settings.jira_available,
        connection_status: connection_status.to_string(),
        capabilities: if settings.enabled && is_valid && settings.jira_available {
            writable_capabilities("manual")
        } else {
            read_only_capabilities("manual")
        },
        fetched_at: Some(now_string()),
        stale_at: None,
        permission_message: (!settings.jira_available && is_valid)
            .then(|| "Jira issue search is not available for this connection.".to_string()),
        error_message: (!is_valid && settings.enabled).then(|| {
            settings
                .last_error
                .clone()
                .unwrap_or_else(|| "Jira integration is not valid.".to_string())
        }),
    }
}

fn linear_provider_summary(
    settings: &LinearIntegrationSettings,
) -> TicketingProviderSummaryResponse {
    let is_valid = settings.validation_status == IntegrationValidationStatus::Valid;
    let connection_status = if !settings.enabled {
        "disconnected"
    } else if !is_valid {
        "error"
    } else if !settings.issue_search_available {
        "permission_limited"
    } else {
        "connected"
    };
    TicketingProviderSummaryResponse {
        provider: PROVIDER_LINEAR.to_string(),
        label: "Linear".to_string(),
        enabled: settings.enabled && is_valid && settings.issue_search_available,
        connection_status: connection_status.to_string(),
        capabilities: if settings.enabled && is_valid && settings.issue_search_available {
            writable_capabilities("webhook")
        } else {
            read_only_capabilities("webhook")
        },
        fetched_at: Some(now_string()),
        stale_at: None,
        permission_message: (!settings.issue_search_available && is_valid)
            .then(|| "Linear issue search is not available for this connection.".to_string()),
        error_message: (!is_valid && settings.enabled).then(|| {
            settings
                .last_error
                .clone()
                .unwrap_or_else(|| "Linear integration is not valid.".to_string())
        }),
    }
}

fn read_only_capabilities(freshness: &str) -> TicketingCapabilitiesResponse {
    TicketingCapabilitiesResponse {
        supports_boards: false,
        supports_kanban: true,
        kanban_write: false,
        status_write: false,
        assignment_write: false,
        comment_write: false,
        freshness: freshness.to_string(),
    }
}

fn writable_capabilities(freshness: &str) -> TicketingCapabilitiesResponse {
    TicketingCapabilitiesResponse {
        supports_boards: false,
        supports_kanban: true,
        kanban_write: true,
        status_write: true,
        assignment_write: true,
        comment_write: true,
        freshness: freshness.to_string(),
    }
}

fn default_ticketing_columns() -> Vec<TicketingColumnResponse> {
    vec![
        ticketing_column("todo", "To Do", "todo", 0),
        ticketing_column("in_progress", "In Progress", "in_progress", 1),
        ticketing_column("done", "Done", "done", 2),
        ticketing_column("other", "Other", "other", 3),
    ]
}

fn ticketing_column(id: &str, name: &str, category: &str, order: usize) -> TicketingColumnResponse {
    TicketingColumnResponse {
        id: id.to_string(),
        name: name.to_string(),
        category: category.to_string(),
        order,
        color: None,
    }
}

fn linear_workflow_state_to_column(
    state: LinearWorkflowState,
    order: usize,
) -> TicketingColumnResponse {
    TicketingColumnResponse {
        id: state.id,
        name: state.name,
        category: state.category,
        order,
        color: state.color,
    }
}

fn jira_summary_to_ticket(summary: AtlassianResourceSummary) -> TicketSummaryResponse {
    let state = ticket_state("Provider result");
    TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_JIRA.to_string(),
            id: summary.id,
            key: summary.key,
        },
        title: summary.title,
        state,
        assignee: None,
        reporter: None,
        labels: Vec::new(),
        project: None,
        priority: None,
        updated_at: now_string(),
        url: summary.url,
        association_count: 0,
    }
}

fn linear_summary_to_ticket(summary: LinearIssueSummary) -> TicketSummaryResponse {
    let state_name = summary
        .state_name
        .unwrap_or_else(|| "Provider result".to_string());
    let state = TicketStateResponse {
        id: summary.state_id.unwrap_or_else(|| state_id(&state_name)),
        name: state_name.clone(),
        category: summary
            .state_category
            .unwrap_or_else(|| state_category(&state_name)),
        color: summary.state_color,
    };
    TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_LINEAR.to_string(),
            id: summary.id,
            key: summary.key,
        },
        title: summary.title,
        state,
        assignee: summary.assignee.as_deref().map(named_person),
        reporter: None,
        labels: summary.labels,
        project: summary.project,
        priority: None,
        updated_at: summary.updated_at.unwrap_or_else(now_string),
        url: summary.url,
        association_count: 0,
    }
}

async fn hydrate_ticket_association_counts(
    state: &AppState,
    provider: &str,
    project_id: Option<&str>,
    items: Vec<TicketSummaryResponse>,
) -> Result<Vec<TicketSummaryResponse>, String> {
    let Some(project_id) = project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| ProjectId::from_string(value.to_string()))
    else {
        return Ok(items);
    };

    let mut hydrated = Vec::with_capacity(items.len());
    for mut item in items {
        let reference = ticket_ref_to_composer_reference(provider, &item.ref_);
        item.association_count = linked_agent_conversation_associations_for_ticket(
            state,
            provider,
            &project_id,
            &reference,
        )
        .await?
        .len();
        hydrated.push(item);
    }
    Ok(hydrated)
}

fn filter_ticket_summaries(
    items: Vec<TicketSummaryResponse>,
    filters: Option<&TicketFiltersInput>,
) -> Vec<TicketSummaryResponse> {
    let Some(filters) = filters else {
        return items;
    };
    items
        .into_iter()
        .filter(|ticket| ticket_matches_filters(ticket, filters))
        .collect()
}

fn ticket_matches_filters(ticket: &TicketSummaryResponse, filters: &TicketFiltersInput) -> bool {
    if let Some(text) = filters.text.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        let needle = text.to_ascii_lowercase();
        let matches_text = ticket.title.to_ascii_lowercase().contains(&needle)
            || ticket
                .ref_
                .key
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains(&needle)
            || ticket.ref_.id.to_ascii_lowercase().contains(&needle);
        if !matches_text {
            return false;
        }
    }

    if let Some(state_ids) = filters.state_ids.as_ref().filter(|values| !values.is_empty()) {
        let ticket_state_id = ticket.state.id.as_str();
        let ticket_state_category = ticket.state.category.as_str();
        if !state_ids
            .iter()
            .any(|state_id| state_id == ticket_state_id || state_id == ticket_state_category)
        {
            return false;
        }
    }

    if let Some(assignee) = filters
        .assignee
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let Some(ticket_assignee) = ticket.assignee.as_ref() else {
            return false;
        };
        let assignee = assignee.to_ascii_lowercase();
        let matches_assignee = ticket_assignee.name.to_ascii_lowercase().contains(&assignee)
            || ticket_assignee
                .id
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains(&assignee)
            || ticket_assignee
                .email
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains(&assignee);
        if !matches_assignee {
            return false;
        }
    }

    if let Some(labels) = filters.labels.as_ref().filter(|values| !values.is_empty()) {
        if !labels.iter().all(|required_label| {
            ticket.labels.iter().any(|label| label.eq_ignore_ascii_case(required_label))
        }) {
            return false;
        }
    }

    true
}

fn jira_content_to_detail(content: AtlassianResourceContent) -> TicketDetailResponse {
    let summary = TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_JIRA.to_string(),
            id: content.id.clone(),
            key: content.key.clone(),
        },
        title: content.title.clone(),
        state: ticket_state(content.status.as_deref().unwrap_or("Provider result")),
        assignee: content.assignee.as_deref().map(named_person),
        reporter: content.reporter.as_deref().map(named_person),
        labels: Vec::new(),
        project: None,
        priority: None,
        updated_at: content.updated_at_remote.clone().unwrap_or_else(now_string),
        url: content.url.clone(),
        association_count: 0,
    };
    TicketDetailResponse {
        summary,
        description_markdown: content
            .description_markdown
            .clone()
            .or_else(|| Some(content.body.clone())),
        description_text: content.description_text.clone(),
        acceptance_criteria_markdown: content.acceptance_criteria_markdown.clone(),
        comments: content
            .comments
            .into_iter()
            .map(|comment| TicketCommentResponse {
                id: comment.id,
                author: comment.author.as_deref().map(named_person),
                body_markdown: comment.body_markdown,
                body_text: comment.body_text,
                created_at: comment.created_at,
                updated_at: comment.updated_at,
            })
            .collect(),
        attachments: content
            .attachments
            .into_iter()
            .map(|attachment| TicketAttachmentResponse {
                id: attachment.id,
                filename: attachment.filename,
                mime_type: attachment.mime_type,
                size: attachment.size,
                url: attachment.content_url,
            })
            .collect(),
        transitions: Vec::new(),
        fetched_at: Some(now_string()),
    }
}

fn linear_content_to_detail(content: LinearIssueContent) -> TicketDetailResponse {
    let summary = TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_LINEAR.to_string(),
            id: content.id.clone(),
            key: content.key.clone(),
        },
        title: content.title.clone(),
        state: ticket_state(content.state_name.as_deref().unwrap_or("Provider result")),
        assignee: content.assignee.as_deref().map(named_person),
        reporter: content.creator.as_deref().map(named_person),
        labels: content.labels.clone(),
        project: content.project.clone(),
        priority: None,
        updated_at: content.updated_at.clone().unwrap_or_else(now_string),
        url: content.url.clone(),
        association_count: 0,
    };
    TicketDetailResponse {
        summary,
        description_markdown: Some(content.body.clone()),
        description_text: Some(content.body),
        acceptance_criteria_markdown: None,
        comments: content
            .comments
            .into_iter()
            .map(ticket_comment_from_linear_comment)
            .collect(),
        attachments: Vec::new(),
        transitions: Vec::new(),
        fetched_at: Some(now_string()),
    }
}

fn named_person(name: &str) -> TicketingPersonResponse {
    TicketingPersonResponse {
        id: None,
        name: name.to_string(),
        email: None,
        avatar_url: None,
    }
}

fn ticket_state(name: &str) -> TicketStateResponse {
    let category = state_category(name);
    TicketStateResponse {
        id: state_id(name),
        name: name.to_string(),
        category,
        color: None,
    }
}

fn state_category(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.contains("done")
        || lower.contains("complete")
        || lower.contains("closed")
        || lower.contains("resolved")
    {
        "done".to_string()
    } else if lower.contains("progress")
        || lower.contains("review")
        || lower.contains("started")
        || lower.contains("active")
    {
        "in_progress".to_string()
    } else if lower.contains("todo") || lower.contains("to do") || lower.contains("backlog") {
        "todo".to_string()
    } else {
        "other".to_string()
    }
}

fn state_id(name: &str) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            last_was_separator = false;
        } else if !last_was_separator {
            output.push('_');
            last_was_separator = true;
        }
    }
    let trimmed = output.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "other".to_string()
    } else {
        trimmed
    }
}

fn ticket_ref_to_composer_reference(
    provider: &str,
    ticket_ref: &TicketRefInput,
) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: if provider == PROVIDER_JIRA {
            "atlassian".to_string()
        } else {
            PROVIDER_LINEAR.to_string()
        },
        kind: if provider == PROVIDER_JIRA {
            "jira".to_string()
        } else {
            PROVIDER_LINEAR.to_string()
        },
        id: ticket_ref.id.clone(),
        key: ticket_ref.key.clone(),
        title: None,
        url: None,
    }
}

fn ticketing_service_from_state(state: &AppState) -> TicketingService {
    TicketingService::new(
        Arc::clone(&state.atlassian_integration_service),
        Arc::clone(&state.linear_integration_service),
        Arc::clone(&state.external_issue_link_service),
    )
}

fn ticketing_service_from_state_with_events(
    state: &AppState,
    app_handle: AppHandle,
) -> TicketingService {
    let event_sink = Arc::new(TauriTicketingEventSink::new(app_handle));
    ticketing_service_from_state(state).with_event_sink(event_sink)
}

fn ticket_identity(
    provider: &str,
    ticket_ref: &TicketRefInput,
    project_id: Option<String>,
) -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: provider.to_string(),
        id: ticket_ref.id.clone(),
        key: ticket_ref.key.clone(),
        local_project_id: project_id,
    }
}

fn ticket_transition_option_response(
    transition: TicketingTransitionOption,
) -> TicketTransitionOptionResponse {
    TicketTransitionOptionResponse {
        to_state_id: transition.to_state_id,
        provider_transition_id: transition.provider_transition_id,
        name: transition.name,
        category: transition.category,
        disabled_reason: transition.disabled_reason,
    }
}

fn ticket_mutation_response(result: TicketingMutationResult) -> TicketMutationResponse {
    TicketMutationResponse {
        ticket_ref: TicketRefInput {
            provider: result.ticket.provider,
            id: result.ticket.id,
            key: result.ticket.key,
        },
        operation: ticket_operation_response(result.operation),
        idempotent: result.idempotent,
        transition: result.transition.map(ticket_transition_option_response),
        assignee: result.assignee.map(ticket_person_result_response),
        comment: result.comment.map(ticket_comment_response),
        refreshed_at: now_string(),
    }
}

fn ticket_person_result_response(person: TicketingPersonResult) -> TicketingPersonResponse {
    TicketingPersonResponse {
        id: person.id,
        name: person.name,
        email: None,
        avatar_url: None,
    }
}

fn ticket_operation_response(operation: ProviderTicketOperation) -> TicketOperationResponse {
    TicketOperationResponse {
        id: operation.id,
        operation: operation.operation.as_str().to_string(),
        client_operation_id: operation.client_operation_id,
        status: operation.status.as_str().to_string(),
        provider_operation_id: operation.provider_operation_id,
        error_message: operation.error_message,
        linked: operation.link_id.is_some(),
        created_at: operation.created_at.to_rfc3339(),
        updated_at: operation.updated_at.to_rfc3339(),
    }
}

fn ticket_comment_response(comment: TicketingCommentResult) -> TicketCommentResponse {
    TicketCommentResponse {
        id: comment.id,
        author: comment.author_name.as_deref().map(named_person),
        body_markdown: comment.body_markdown,
        body_text: comment.body_text,
        created_at: comment.created_at,
        updated_at: comment.updated_at,
    }
}

fn ticket_comment_from_linear_comment(comment: LinearComment) -> TicketCommentResponse {
    TicketCommentResponse {
        id: Some(comment.id),
        author: comment.author_name.as_deref().map(named_person),
        body_markdown: comment.body.clone(),
        body_text: comment.body,
        created_at: comment.created_at,
        updated_at: comment.updated_at,
    }
}

fn ensure_ticket_composer_reference(
    references: &mut Vec<ComposerIntegrationReference>,
    ticket_reference: ComposerIntegrationReference,
) -> ComposerIntegrationReference {
    if let Some(existing) = references
        .iter()
        .find(|reference| same_ticket_reference(reference, &ticket_reference))
    {
        return existing.clone();
    }
    references.push(ticket_reference.clone());
    ticket_reference
}

fn same_ticket_reference(
    left: &ComposerIntegrationReference,
    right: &ComposerIntegrationReference,
) -> bool {
    if left.provider != right.provider || left.kind != right.kind {
        return false;
    }
    if left.id == right.id {
        return true;
    }
    match (left.key.as_deref(), right.key.as_deref()) {
        (Some(left_key), Some(right_key)) => left_key.eq_ignore_ascii_case(right_key),
        _ => false,
    }
}

async fn link_started_ticket_to_conversation(
    state: &AppState,
    provider: &str,
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    reference: &ComposerIntegrationReference,
) -> Result<(), String> {
    match provider {
        PROVIDER_JIRA => {
            let reference = jira_reference_from_composer_reference(reference)
                .ok_or_else(|| "Invalid Jira ticket reference".to_string())?;
            let link = agent_conversation_jira_issue::manual_link_from_reference(
                conversation_id,
                project_id,
                reference,
                Utc::now(),
            );
            state
                .agent_conversation_jira_issue_repo
                .upsert(link)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        PROVIDER_LINEAR => {
            let reference =
                agent_conversation_linear_issue::linear_reference_from_composer_reference(
                    reference,
                )
                .ok_or_else(|| "Invalid Linear ticket reference".to_string())?;
            let link = agent_conversation_linear_issue::manual_link_from_reference(
                conversation_id,
                project_id,
                reference,
                Utc::now(),
            );
            state
                .agent_conversation_linear_issue_repo
                .upsert(link)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        _ => Err(format!("Unknown ticketing provider: {provider}")),
    }
}

async fn linked_agent_conversation_associations_for_ticket(
    state: &AppState,
    provider: &str,
    project_id: &ProjectId,
    reference: &ComposerIntegrationReference,
) -> Result<Vec<TicketAssociationItemResponse>, String> {
    let conversations = state
        .chat_conversation_repo
        .get_by_context_filtered(ChatContextType::Project, project_id.as_str(), true)
        .await
        .map_err(|error| error.to_string())?;
    let mut associations = Vec::new();

    match provider {
        PROVIDER_JIRA => {
            let reference = jira_reference_from_composer_reference(reference)
                .ok_or_else(|| "Invalid Jira ticket reference".to_string())?;
            for conversation in conversations {
                let link = state
                    .agent_conversation_jira_issue_repo
                    .get_by_conversation_id(&conversation.id)
                    .await
                    .map_err(|error| error.to_string())?;
                if link
                    .as_ref()
                    .is_some_and(|link| jira_link_matches_ticket(link, project_id, &reference))
                {
                    associations.push(agent_conversation_association_item(&conversation));
                }
            }
        }
        PROVIDER_LINEAR => {
            let reference =
                agent_conversation_linear_issue::linear_reference_from_composer_reference(
                    reference,
                )
                .ok_or_else(|| "Invalid Linear ticket reference".to_string())?;
            for conversation in conversations {
                let link = state
                    .agent_conversation_linear_issue_repo
                    .get_by_conversation_id(&conversation.id)
                    .await
                    .map_err(|error| error.to_string())?;
                if link
                    .as_ref()
                    .is_some_and(|link| linear_link_matches_ticket(link, project_id, &reference))
                {
                    associations.push(agent_conversation_association_item(&conversation));
                }
            }
        }
        _ => return Err(format!("Unknown ticketing provider: {provider}")),
    }

    Ok(associations)
}

fn jira_link_matches_ticket(
    link: &AgentConversationJiraIssueLink,
    project_id: &ProjectId,
    reference: &ComposerJiraReferenceMetadata,
) -> bool {
    link.project_id == *project_id
        && (link.issue_key.eq_ignore_ascii_case(&reference.issue_key)
            || reference
                .issue_id
                .as_ref()
                .is_some_and(|issue_id| link.issue_id.as_ref() == Some(issue_id)))
}

fn linear_link_matches_ticket(
    link: &AgentConversationLinearIssueLink,
    project_id: &ProjectId,
    reference: &agent_conversation_linear_issue::ComposerLinearReferenceMetadata,
) -> bool {
    link.project_id == *project_id
        && (link.issue_id == reference.issue_id
            || reference.issue_key.as_ref().is_some_and(|issue_key| {
                link.issue_key
                    .as_deref()
                    .is_some_and(|link_key| link_key.eq_ignore_ascii_case(issue_key))
            }))
}

fn agent_conversation_association_item(
    conversation: &ChatConversation,
) -> TicketAssociationItemResponse {
    let id = conversation.id.as_str();
    TicketAssociationItemResponse {
        id: id.clone(),
        title: conversation
            .title
            .clone()
            .unwrap_or_else(|| "Agent conversation".to_string()),
        subtitle: Some("Agent conversation".to_string()),
        status: conversation.agent_mode.map(|mode| mode.to_string()),
        active: conversation.archived_at.is_none(),
        deep_link: TicketDeepLinkResponse {
            view: "agents".to_string(),
            id,
        },
    }
}

fn empty_associations() -> TicketAssociationsResponse {
    TicketAssociationsResponse {
        tasks: Vec::new(),
        proposals: Vec::new(),
        sessions: Vec::new(),
        conversations: Vec::new(),
        pull_requests: Vec::new(),
        checks: Vec::new(),
        qa: Vec::new(),
        specs: Vec::new(),
        fetched_at: Some(now_string()),
    }
}

fn validate_provider(provider: &str) -> Result<(), String> {
    match provider {
        PROVIDER_JIRA | PROVIDER_LINEAR => Ok(()),
        other => Err(format!("Unknown ticketing provider: {other}")),
    }
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
#[path = "ticketing_commands_tests.rs"]
mod tests;
