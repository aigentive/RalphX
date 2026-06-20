use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{
    AppState, AtlassianResourceContent, AtlassianResourceKind, AtlassianResourceSummary,
    LinearIntegrationSettings, LinearIssueContent, LinearIssueSummary,
};
use crate::domain::integrations::{AtlassianIntegrationSettings, IntegrationValidationStatus};
use crate::domain::services::ComposerIntegrationReference;

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
pub fn list_ticketing_columns(
    provider: String,
    container_id: Option<String>,
) -> Result<Vec<TicketingColumnResponse>, String> {
    validate_provider(&provider)?;
    let _ = container_id;
    Ok(default_ticketing_columns())
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
    if let Some(filters) = query.filters.as_ref() {
        let _ = (&filters.assignee, &filters.state_ids, &filters.labels);
    }
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
pub fn list_ticket_transitions(
    provider: String,
    ticket_ref: TicketRefInput,
) -> Result<Vec<TicketTransitionOptionResponse>, String> {
    validate_provider(&provider)?;
    let _ = ticket_ref;
    Ok(Vec::new())
}

#[tauri::command]
pub fn get_ticket_associations(
    provider: String,
    ticket_ref: TicketRefInput,
    project_id: String,
) -> Result<TicketAssociationsResponse, String> {
    validate_provider(&provider)?;
    let _ = (ticket_ref, project_id);
    Ok(empty_associations())
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
        capabilities: read_only_capabilities("manual"),
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
        capabilities: read_only_capabilities("webhook"),
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
    let state = ticket_state(&state_name);
    TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_LINEAR.to_string(),
            id: summary.id,
            key: summary.key,
        },
        title: summary.title,
        state,
        assignee: None,
        reporter: None,
        labels: Vec::new(),
        priority: None,
        updated_at: now_string(),
        url: summary.url,
        association_count: 0,
    }
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
        labels: Vec::new(),
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
        comments: Vec::new(),
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
