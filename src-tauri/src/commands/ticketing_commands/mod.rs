use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use chrono::Utc;
use serde_json::json;
use tauri::{AppHandle, Runtime, State};

use crate::application::clickup_integration_service::{
    ClickUpFolder, ClickUpList, ClickUpStatus, ClickUpTaskListOptions,
};
use crate::application::external_issue_link_service::TicketConversationLinkInput;
use crate::application::ticketing_pr_summary::{ticket_pr_branch_summary, TicketPrBranchSummary};
use crate::application::{
    agent_conversation_jira_issue, agent_conversation_linear_issue,
    agent_conversation_start_service::{AgentConversationStartDeps, AgentConversationStartService},
    AppState, AtlassianResourceContent, AtlassianResourceKind, AtlassianResourceSummary,
    ClickUpComment, ClickUpSpace, ClickUpTaskContent, ClickUpTaskSummary, ClickUpUser,
    JiraIssueDetail, JiraProjectSummary, LinearComment, LinearIntegrationSettings,
    LinearIssueContent, LinearIssueSummary, LinearLabel, TauriTicketingEventSink,
    TicketAssignRequest, TicketCommentRequest, TicketSetLabelsRequest, TicketTransitionRequest,
    TicketingCommentResult, TicketingLabelResult, TicketingMutationResult, TicketingPersonResult,
    TicketingService, TicketingTicketIdentity, TicketingTransitionOption,
};
use crate::commands::unified_chat_commands::{
    agent_conversation_response_for_state, agent_workspace_response_with_pr_supervision_for_state,
    SendAgentMessageResponse, StartAgentConversationResponse,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    is_open_pr, AgentConversationJiraIssueLink, AgentConversationLinearIssueLink, ChatContextType,
    ChatConversation, ChatConversationId, ProjectId,
};
use crate::domain::integrations::{
    AtlassianIntegrationSettings, ClickUpIntegrationSettings, ExternalIssueLink,
    IntegrationValidationStatus, ObservedTicketingStatus, ProviderTicketOperation,
    TicketingStatusCatalogEntry, TicketingStatusPresentationPatch,
};
use crate::domain::services::{
    jira_reference_from_composer_reference, ComposerIntegrationReference,
    ComposerJiraReferenceMetadata,
};

mod types;
pub use types::*;

const PROVIDER_JIRA: &str = "jira";
const PROVIDER_LINEAR: &str = "linear";
const PROVIDER_CLICKUP: &str = "clickup";
const TICKETING_CONTAINER_LIMIT: usize = 1000;
const TICKET_PAGE_MAX_LIMIT: usize = 40;
const TICKET_SELECTOR_OPTION_LIMIT: usize = 500;
const CLICKUP_FILTERED_TASK_SCAN_LIMIT: usize = 5000;
const TICKET_OFFSET_CURSOR_PREFIX: &str = "offset:";
const UNASSIGNED_ASSIGNEE_FILTER: &str = "__unassigned__";

#[derive(Debug, Clone)]
enum ProjectTicketLink {
    Jira(AgentConversationJiraIssueLink),
    Linear(AgentConversationLinearIssueLink),
    ClickUp(ExternalIssueLink),
}

#[derive(Debug, Clone)]
struct ProjectTicketConversationAssociation {
    link: ProjectTicketLink,
    item: TicketAssociationItemResponse,
}

#[derive(Debug, Clone)]
struct TicketingStatusCatalogScope {
    provider: String,
    scope_kind: String,
    scope_id: String,
}

#[tauri::command]
pub async fn list_ticketing_providers(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TicketingProviderSummaryResponse>, String> {
    let _ = project_id;
    let jira = state.atlassian_integration_service.get_settings().await?;
    let linear = state.linear_integration_service.get_settings().await?;
    let clickup = state.clickup_integration_service.get_settings().await?;
    Ok(vec![
        jira_provider_summary(&jira),
        linear_provider_summary(&linear),
        clickup_provider_summary(&clickup),
    ])
}

#[tauri::command]
pub async fn list_ticketing_containers(
    provider: String,
    project_id: Option<String>,
    parent_container_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TicketingContainerResponse>, String> {
    validate_provider(&provider)?;
    let _ = project_id;
    match provider.as_str() {
        PROVIDER_JIRA => state
            .atlassian_integration_service
            .list_jira_projects(TICKETING_CONTAINER_LIMIT)
            .await
            .map(|projects| {
                projects
                    .into_iter()
                    .map(jira_project_to_container)
                    .collect()
            }),
        PROVIDER_LINEAR => state
            .linear_integration_service
            .list_projects(TICKETING_CONTAINER_LIMIT)
            .await
            .map(|projects| {
                projects
                    .into_iter()
                    .map(|project| TicketingContainerResponse {
                        provider: PROVIDER_LINEAR.to_string(),
                        id: project.id,
                        key: None,
                        name: project.name,
                        kind: "project".to_string(),
                        parent_id: None,
                        ticket_count: None,
                    })
                    .collect()
            }),
        // ClickUp loads Spaces first, then folders/lists lazily for a selected
        // Space so the dashboard shell does not block on the full workspace tree.
        PROVIDER_CLICKUP => match clickup_selected_space_id(parent_container_id.as_deref()) {
            Some(space_id) => clickup_location_containers_for_space(state.inner(), space_id).await,
            None => state
                .clickup_integration_service
                .list_spaces()
                .await
                .map(|spaces| spaces.into_iter().map(clickup_space_to_container).collect()),
        },
        _ => unreachable!("provider validated above"),
    }
}

#[tauri::command]
pub async fn list_ticketing_columns(
    provider: String,
    container_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TicketingColumnResponse>, String> {
    validate_provider(&provider)?;
    let Some(scope) = column_status_catalog_scope(&provider, container_id.as_deref())? else {
        return Ok(Vec::new());
    };
    sync_status_catalog_for_scope(state.inner(), &scope)
        .await
        .map(catalog_entries_to_columns)
}

#[tauri::command]
pub async fn list_ticketing_status_catalog(
    provider: String,
    scope_kind: String,
    scope_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<TicketingStatusCatalogEntryResponse>, String> {
    let scope = normalize_status_catalog_scope(provider, scope_kind, scope_id)?;
    state
        .ticketing_status_catalog_service
        .list_status_catalog(&scope.provider, &scope.scope_kind, &scope.scope_id)
        .await
        .map(|entries| {
            entries
                .into_iter()
                .map(status_catalog_entry_response)
                .collect()
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn refresh_ticketing_status_catalog(
    provider: String,
    scope_kind: String,
    scope_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<TicketingStatusCatalogEntryResponse>, String> {
    let scope = normalize_status_catalog_scope(provider, scope_kind, scope_id)?;
    sync_status_catalog_for_scope(state.inner(), &scope)
        .await
        .map(|entries| {
            entries
                .into_iter()
                .map(status_catalog_entry_response)
                .collect()
        })
}

#[tauri::command]
pub async fn update_ticketing_status_presentation(
    input: UpdateTicketingStatusPresentationInput,
    state: State<'_, AppState>,
) -> Result<Vec<TicketingStatusCatalogEntryResponse>, String> {
    let scope = normalize_status_catalog_scope(input.provider, input.scope_kind, input.scope_id)?;
    let patches = input
        .patches
        .into_iter()
        .map(status_presentation_patch)
        .collect::<Result<Vec<_>, _>>()?;
    state
        .ticketing_status_catalog_service
        .update_status_presentation(&scope.provider, &scope.scope_kind, &scope.scope_id, patches)
        .await
        .map(|entries| {
            entries
                .into_iter()
                .map(status_catalog_entry_response)
                .collect()
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_tickets(
    query: ListTicketsQuery,
    state: State<'_, AppState>,
) -> Result<TicketPageResponse, String> {
    validate_provider(&query.provider)?;
    let _ = (&query.project_id, &query.sort);
    let offset = decode_ticket_offset_cursor(query.cursor.as_deref())?;
    let limit = query.limit.unwrap_or(25).clamp(1, TICKET_PAGE_MAX_LIMIT);
    let text = query
        .filters
        .as_ref()
        .and_then(|filters| filters.text.as_deref())
        .unwrap_or("")
        .trim()
        .to_string();
    let fetched_at = now_string();
    let requested = ticket_provider_fetch_limit(
        &query.provider,
        query.container_id.as_deref(),
        query.filters.as_ref(),
        offset.saturating_add(limit).saturating_add(1),
    );
    let container_id = ticket_provider_container_scope(
        &query.provider,
        query.container_id.as_deref(),
        query.filters.as_ref(),
    );
    let items = load_ticket_summaries(
        state.inner(),
        &query.provider,
        container_id,
        &text,
        requested,
        query.filters.as_ref(),
    )
    .await?;
    let (page_items, next_cursor, total_loaded) =
        ticket_page_from_loaded_summaries(items, query.filters.as_ref(), offset, limit);
    let items = hydrate_ticket_association_counts(
        state.inner(),
        &query.provider,
        query.project_id.as_deref(),
        page_items,
    )
    .await?;
    Ok(TicketPageResponse {
        total: Some(total_loaded),
        items,
        next_cursor,
        fetched_at: Some(fetched_at),
    })
}

#[tauri::command]
pub async fn list_ticket_filter_options(
    query: ListTicketFilterOptionsQuery,
    state: State<'_, AppState>,
) -> Result<TicketFilterOptionsResponse, String> {
    validate_provider(&query.provider)?;
    let _ = &query.project_id;
    let limit = query
        .limit
        .unwrap_or(TICKET_SELECTOR_OPTION_LIMIT)
        .clamp(1, TICKET_SELECTOR_OPTION_LIMIT);
    let text = query
        .filters
        .as_ref()
        .and_then(|filters| filters.text.as_deref())
        .unwrap_or("")
        .trim()
        .to_string();
    let requested = ticket_provider_fetch_limit(
        &query.provider,
        query.container_id.as_deref(),
        query.filters.as_ref(),
        limit.saturating_add(1),
    );
    let container_id = ticket_provider_container_scope(
        &query.provider,
        query.container_id.as_deref(),
        query.filters.as_ref(),
    );
    let items = load_ticket_summaries(
        state.inner(),
        &query.provider,
        container_id,
        &text,
        requested,
        query.filters.as_ref(),
    )
    .await?;
    let provider_truncated = items.len() > limit;
    Ok(ticket_filter_options_from_loaded_summaries(
        &query.provider,
        items,
        query.filters.as_ref(),
        limit,
        provider_truncated,
    ))
}

fn ticket_page_from_loaded_summaries(
    items: Vec<TicketSummaryResponse>,
    filters: Option<&TicketFiltersInput>,
    offset: usize,
    limit: usize,
) -> (Vec<TicketSummaryResponse>, Option<String>, usize) {
    let items = filter_ticket_summaries(items, filters);
    let total_loaded = items.len();
    let page_items: Vec<TicketSummaryResponse> =
        items.into_iter().skip(offset).take(limit).collect();
    let next_cursor = if total_loaded > offset.saturating_add(page_items.len()) {
        Some(encode_ticket_offset_cursor(
            offset.saturating_add(page_items.len()),
        ))
    } else {
        None
    };
    (page_items, next_cursor, total_loaded)
}

fn ticket_filter_options_from_loaded_summaries(
    provider: &str,
    items: Vec<TicketSummaryResponse>,
    filters: Option<&TicketFiltersInput>,
    limit: usize,
    provider_truncated: bool,
) -> TicketFilterOptionsResponse {
    let items = filter_ticket_summaries(items, filters);
    let truncated = provider_truncated || items.len() > limit;
    let mut assignees = BTreeSet::new();
    let mut sprints = BTreeSet::new();

    for ticket in items.into_iter().take(limit) {
        for person in ticket.assignees.iter().chain(ticket.assignee.iter()) {
            let name = person.name.trim();
            if !name.is_empty() {
                assignees.insert(name.to_string());
            }
        }
        if provider == PROVIDER_CLICKUP && ticket.current_user_assigned {
            for sprint in ticket_sprint_names(&ticket) {
                sprints.insert(sprint);
            }
        }
    }

    TicketFilterOptionsResponse {
        assignees: assignees.into_iter().collect(),
        sprints: sprints.into_iter().collect(),
        complete: !truncated,
        truncated,
    }
}

fn ticket_provider_fetch_limit(
    provider: &str,
    container_id: Option<&str>,
    filters: Option<&TicketFiltersInput>,
    requested: usize,
) -> usize {
    if provider == PROVIDER_CLICKUP
        && (ticket_filters_need_wide_clickup_scan(filters)
            || container_selected_key(container_id).is_some())
    {
        return requested.max(CLICKUP_FILTERED_TASK_SCAN_LIMIT);
    }
    requested
}

fn ticket_filters_need_wide_clickup_scan(filters: Option<&TicketFiltersInput>) -> bool {
    let Some(filters) = filters else {
        return false;
    };
    !ticket_assignee_filters(filters).is_empty()
        || filters
            .sprint
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
}

fn clickup_provider_assignee_ids(
    filters: Option<&TicketFiltersInput>,
    current_user: Option<&ClickUpUser>,
) -> Vec<i64> {
    let Some(current_user) = current_user else {
        return Vec::new();
    };
    let haystacks = [
        Some(current_user.id.to_string()),
        current_user.username.clone(),
        current_user.email.clone(),
    ];
    let current_user_values: Vec<String> = haystacks
        .into_iter()
        .flatten()
        .map(|value| value.to_ascii_lowercase())
        .collect();
    let named_assignees: Vec<String> = filters
        .map(ticket_assignee_filters)
        .unwrap_or_default()
        .into_iter()
        .filter(|assignee| assignee != UNASSIGNED_ASSIGNEE_FILTER)
        .map(|assignee| assignee.to_ascii_lowercase())
        .collect();
    if named_assignees.len() == 1
        && named_assignees.iter().any(|assignee| {
            current_user_values
                .iter()
                .any(|value| value.contains(assignee.as_str()) || assignee.contains(value.as_str()))
        })
    {
        return vec![current_user.id];
    }
    Vec::new()
}

fn ticket_provider_container_scope<'a>(
    provider: &str,
    container_id: Option<&'a str>,
    filters: Option<&TicketFiltersInput>,
) -> Option<&'a str> {
    if provider == PROVIDER_CLICKUP
        && filters
            .and_then(|filters| filters.sprint.as_deref())
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    {
        if container_selected_key(container_id)
            .is_some_and(|container_id| container_id.strip_prefix("list:").is_some())
        {
            return container_id;
        }
        return None;
    }
    container_id
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClickUpContainerScope {
    Workspace,
    Space(String),
    Folder(String),
    List(String),
}

fn clickup_container_scope(container_id: Option<&str>) -> ClickUpContainerScope {
    let Some(container_id) = container_selected_key(container_id) else {
        return ClickUpContainerScope::Workspace;
    };
    if let Some(id) = container_id.strip_prefix("space:") {
        return ClickUpContainerScope::Space(id.to_string());
    }
    if let Some(id) = container_id.strip_prefix("folder:") {
        return ClickUpContainerScope::Folder(id.to_string());
    }
    if let Some(id) = container_id.strip_prefix("list:") {
        return ClickUpContainerScope::List(id.to_string());
    }
    ClickUpContainerScope::Space(container_id.to_string())
}

fn clickup_status_catalog_scope(
    scope: ClickUpContainerScope,
) -> Option<TicketingStatusCatalogScope> {
    match scope {
        ClickUpContainerScope::Workspace => None,
        ClickUpContainerScope::Space(scope_id) => Some(TicketingStatusCatalogScope {
            provider: PROVIDER_CLICKUP.to_string(),
            scope_kind: "clickup_space".to_string(),
            scope_id,
        }),
        ClickUpContainerScope::Folder(scope_id) => Some(TicketingStatusCatalogScope {
            provider: PROVIDER_CLICKUP.to_string(),
            scope_kind: "clickup_folder".to_string(),
            scope_id,
        }),
        ClickUpContainerScope::List(scope_id) => Some(TicketingStatusCatalogScope {
            provider: PROVIDER_CLICKUP.to_string(),
            scope_kind: "clickup_list".to_string(),
            scope_id,
        }),
    }
}

fn clickup_status_scope_id(scope_kind: &str, scope_id: &str) -> Result<String, String> {
    let prefix = match scope_kind {
        "clickup_space" => "space:",
        "clickup_folder" => "folder:",
        "clickup_list" => "list:",
        _ => return Err(format!("Unsupported ClickUp status scope: {scope_kind}")),
    };
    let scope_id = scope_id.strip_prefix(prefix).unwrap_or(scope_id).trim();
    if scope_id.is_empty() || scope_id.contains(':') {
        return Err(format!(
            "ClickUp status scope must be a {} id",
            scope_kind.trim_start_matches("clickup_")
        ));
    }
    Ok(scope_id.to_string())
}

fn clickup_selected_space_id(container_id: Option<&str>) -> Option<&str> {
    let container_id = container_selected_key(container_id)?;
    if let Some(space_id) = container_id.strip_prefix("space:") {
        return Some(space_id);
    }
    if container_id.contains(':') {
        return None;
    }
    Some(container_id)
}

fn clickup_summary_matches_container(
    summary: &ClickUpTaskSummary,
    scope: &ClickUpContainerScope,
) -> bool {
    match scope {
        ClickUpContainerScope::Workspace => true,
        ClickUpContainerScope::Space(space_id) => {
            summary.space_id.as_deref() == Some(space_id.as_str())
                || summary
                    .location_space_ids
                    .iter()
                    .any(|location_space_id| location_space_id == space_id)
        }
        ClickUpContainerScope::Folder(folder_id) => {
            summary.folder_id.as_deref() == Some(folder_id.as_str())
                || summary
                    .location_folder_ids
                    .iter()
                    .any(|location_folder_id| location_folder_id == folder_id)
        }
        ClickUpContainerScope::List(list_id) => {
            summary.list_id.as_deref() == Some(list_id.as_str())
                || summary
                    .location_ids
                    .iter()
                    .any(|location_id| location_id == list_id)
        }
    }
}

async fn load_ticket_summaries(
    state: &AppState,
    provider: &str,
    container_id: Option<&str>,
    text: &str,
    limit: usize,
    filters: Option<&TicketFiltersInput>,
) -> Result<Vec<TicketSummaryResponse>, String> {
    let limit = limit.max(1);
    match provider {
        // With a selected project, fetch its issues (richer status/assignee/labels
        // needed for kanban columns). Without one, fall back to global text search
        // (the frontend force-select gate means this path is rarely hit, but keep
        // it functional).
        PROVIDER_JIRA => match container_selected_key(container_id) {
            Some(key) => Ok(state
                .atlassian_integration_service
                .list_jira_project_issues(key, limit)
                .await?
                .into_iter()
                .map(jira_issue_detail_to_ticket)
                .collect()),
            None => Ok(state
                .atlassian_integration_service
                .search_resources(AtlassianResourceKind::Jira, text, limit)
                .await?
                .into_iter()
                .map(jira_summary_to_ticket)
                .collect()),
        },
        PROVIDER_LINEAR => Ok(state
            .linear_integration_service
            .search_issues(text, limit)
            .await?
            .into_iter()
            .map(linear_summary_to_ticket)
            .collect()),
        // ClickUp tasks load via the workspace-scoped filtered-tasks endpoint
        // (Jira-like server-side scoping). A selected Space narrows the query; with
        // no Space selected the workspace returns all of its tasks. Text filtering
        // is applied provider-neutrally by `filter_ticket_summaries` below.
        PROVIDER_CLICKUP => {
            let clickup_scope = clickup_container_scope(container_id);
            let current_user = state.clickup_integration_service.current_user().await.ok();
            let assignee_ids = clickup_provider_assignee_ids(filters, current_user.as_ref());
            let options = ClickUpTaskListOptions {
                query: Some(text.to_string()),
                limit: Some(limit),
                assignee_ids,
            };
            let provider_scoped = matches!(
                clickup_scope,
                ClickUpContainerScope::Space(_) | ClickUpContainerScope::List(_)
            );
            let summaries = match &clickup_scope {
                ClickUpContainerScope::Space(space_id) => {
                    state
                        .clickup_integration_service
                        .list_tasks(vec![space_id.clone()], options)
                        .await?
                }
                ClickUpContainerScope::List(list_id) => {
                    state
                        .clickup_integration_service
                        .list_tasks_for_list(list_id, options)
                        .await?
                }
                _ => {
                    state
                        .clickup_integration_service
                        .list_tasks(Vec::new(), options)
                        .await?
                }
            };
            Ok(summaries
                .into_iter()
                .filter(|summary| {
                    provider_scoped || clickup_summary_matches_container(summary, &clickup_scope)
                })
                .map(|summary| {
                    let current_user_assigned = current_user
                        .as_ref()
                        .is_some_and(|user| clickup_summary_assigned_to_user(&summary, user));
                    let current_user_watching = current_user
                        .as_ref()
                        .is_some_and(|user| clickup_summary_watched_by_user(&summary, user));
                    let mut ticket = clickup_summary_to_ticket(summary);
                    ticket.current_user_assigned = current_user_assigned;
                    ticket.current_user_watching = current_user_watching;
                    ticket
                })
                .collect())
        }
        _ => unreachable!("provider validated above"),
    }
}

fn decode_ticket_offset_cursor(cursor: Option<&str>) -> Result<usize, String> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(0);
    };
    let offset = cursor
        .strip_prefix(TICKET_OFFSET_CURSOR_PREFIX)
        .ok_or_else(|| "Unsupported ticket cursor".to_string())?;
    offset
        .parse::<usize>()
        .map_err(|_| "Invalid ticket cursor".to_string())
}

fn encode_ticket_offset_cursor(offset: usize) -> String {
    format!("{TICKET_OFFSET_CURSOR_PREFIX}{offset}")
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
        // ClickUp addresses tasks by their opaque id, so fetch by id directly
        // rather than via the composer reference used by Jira/Linear.
        PROVIDER_CLICKUP => state
            .clickup_integration_service
            .fetch_task(&ticket_ref.id)
            .await
            .map(clickup_content_to_detail),
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

    // Join the linked conversations to their workspace branch/PR state so the
    // detail "Pull Requests" tab can show the RalphX git work for this ticket.
    let conversation_ids: Vec<ChatConversationId> = response
        .conversations
        .iter()
        .map(|item| ChatConversationId::from_string(item.id.clone()))
        .collect();
    let summaries = ticket_pr_branch_summary(
        state.agent_conversation_workspace_repo.as_ref(),
        &project_id,
        &conversation_ids,
    )
    .await
    .map_err(|error| error.to_string())?;
    response.pull_requests = pull_request_association_items(&summaries, project_id.as_str());

    Ok(response)
}

/// Maps workspace branch/PR summaries to detail "Pull Requests" association items,
/// deep-linking each back to its conversation. PR status is poll/reconcile-driven,
/// so `active`/`status` reflect last-known state, not real-time GitHub.
fn pull_request_association_items(
    summaries: &[TicketPrBranchSummary],
    project_id: &str,
) -> Vec<TicketAssociationItemResponse> {
    summaries
        .iter()
        .filter(|summary| summary.has_pr() || !summary.branch_name.trim().is_empty())
        .map(|summary| {
            let title = match summary.pr_number {
                Some(number) => format!("PR #{number}"),
                None => summary.branch_name.clone(),
            };
            let status = summary
                .pr_status
                .clone()
                .or_else(|| (!summary.branch_name.trim().is_empty()).then(|| "branch".to_string()));
            TicketAssociationItemResponse {
                id: summary
                    .pr_url
                    .clone()
                    .unwrap_or_else(|| summary.conversation_id.clone()),
                title,
                subtitle: Some(summary.branch_name.clone()),
                status,
                active: summary.is_open,
                deep_link: TicketDeepLinkResponse {
                    view: "agents".to_string(),
                    id: summary.conversation_id.clone(),
                    project_id: Some(project_id.to_string()),
                },
                branch_name: Some(summary.branch_name.clone()),
                base_ref: (!summary.base_ref.trim().is_empty()).then(|| summary.base_ref.clone()),
                pr_number: summary.pr_number,
                pr_url: summary.pr_url.clone(),
            }
        })
        .collect()
}

#[tauri::command]
pub async fn get_conversation_ticket(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ConversationTicketResponse>, String> {
    let conversation_id = conversation_id
        .parse::<ChatConversationId>()
        .map_err(|_| "Invalid conversationId".to_string())?;
    let conversation_id_value = conversation_id.as_str();

    if let Some(link) = state
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(Some(ConversationTicketResponse {
            ticket_ref: TicketRefInput {
                provider: PROVIDER_JIRA.to_string(),
                id: link
                    .issue_id
                    .clone()
                    .unwrap_or_else(|| link.issue_key.clone()),
                key: Some(link.issue_key),
            },
            project_id: link.project_id.as_str().to_string(),
            title: link.title,
            url: link.issue_url,
        }));
    }

    let clickup_link = state
        .external_issue_link_service
        .list_ticket_links_for_conversation(&conversation_id_value)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|link| {
            link.provider.eq_ignore_ascii_case(PROVIDER_CLICKUP)
                && link.external_kind.eq_ignore_ascii_case(PROVIDER_CLICKUP)
        });
    if let Some(link) = clickup_link {
        let title = link.metadata_json.as_deref().and_then(|metadata| {
            serde_json::from_str::<serde_json::Value>(metadata)
                .ok()
                .and_then(|metadata| {
                    metadata
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
        });
        return Ok(Some(ConversationTicketResponse {
            ticket_ref: TicketRefInput {
                provider: PROVIDER_CLICKUP.to_string(),
                id: link.external_id,
                key: link.external_key,
            },
            project_id: link.local_project_id.unwrap_or_default(),
            title,
            url: link.external_url,
        }));
    }

    if let Some(link) = state
        .agent_conversation_linear_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(Some(ConversationTicketResponse {
            ticket_ref: TicketRefInput {
                provider: PROVIDER_LINEAR.to_string(),
                id: link.issue_id,
                key: link.issue_key,
            },
            project_id: link.project_id.as_str().to_string(),
            title: link.title,
            url: link.issue_url,
        }));
    }

    Ok(None)
}

#[tauri::command]
pub async fn start_ralphx_work_from_ticket<R: Runtime + 'static>(
    mut input: StartRalphxWorkFromTicketInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    _app: tauri::AppHandle<R>,
) -> Result<StartAgentConversationResponse, String> {
    let provider = input.ticket_ref.provider.clone();
    validate_provider(&provider)?;
    let project_id = ProjectId::from_string(
        input
            .start
            .project_id
            .clone()
            .ok_or_else(|| "Starting work from a ticket requires a project".to_string())?,
    );
    let ticket_reference = ticket_ref_to_composer_reference(&provider, &input.ticket_ref);
    let issue_reference = ensure_ticket_composer_reference(
        &mut input.start.composer_integration_references,
        ticket_reference,
    );

    let mut result = AgentConversationStartService::new(AgentConversationStartDeps {
        state: state.inner(),
        execution_state: execution_state.inner(),
        events: Arc::clone(&state.events),
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
        Some(workspace) => Some(
            agent_workspace_response_with_pr_supervision_for_state(
                state.inner(),
                execution_state.inner(),
                workspace,
            )
            .await?,
        ),
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

#[tauri::command]
pub async fn set_ticket_labels(
    input: SetTicketLabelsInput,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<TicketMutationResponse, String> {
    validate_provider(&input.provider)?;
    let result = ticketing_service_from_state_with_events(&state, app_handle)
        .set_ticket_labels(TicketSetLabelsRequest {
            ticket: ticket_identity(&input.provider, &input.ticket_ref, input.project_id.clone()),
            labels: input.labels,
            client_operation_id: input.client_operation_id,
        })
        .await?;
    Ok(ticket_mutation_response(result))
}

#[tauri::command]
pub async fn list_ticket_labels(
    provider: String,
    ticket_ref: TicketRefInput,
    state: State<'_, AppState>,
) -> Result<Vec<TicketLabelOptionResponse>, String> {
    validate_provider(&provider)?;
    match provider.as_str() {
        PROVIDER_LINEAR => state
            .linear_integration_service
            .list_issue_team_labels(&ticket_ref.id)
            .await
            .map(|labels| {
                labels
                    .into_iter()
                    .map(ticket_label_option_response)
                    .collect()
            }),
        // Jira labels and ClickUp tags are free-text with no fixed selectable list,
        // so neither exposes label options (the pick-list gating stays Linear-only).
        _ => Ok(Vec::new()),
    }
}

fn ticket_label_option_response(label: LinearLabel) -> TicketLabelOptionResponse {
    TicketLabelOptionResponse {
        id: Some(label.id),
        name: label.name,
    }
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

fn clickup_provider_summary(
    settings: &ClickUpIntegrationSettings,
) -> TicketingProviderSummaryResponse {
    let is_valid = settings.validation_status == IntegrationValidationStatus::Valid;
    let connection_status = if !settings.enabled {
        "disconnected"
    } else if !is_valid {
        "error"
    } else if !settings.task_search_available {
        "permission_limited"
    } else {
        "connected"
    };
    TicketingProviderSummaryResponse {
        provider: PROVIDER_CLICKUP.to_string(),
        label: "ClickUp".to_string(),
        enabled: settings.enabled && is_valid && settings.task_search_available,
        connection_status: connection_status.to_string(),
        // ClickUp has full write-back parity (transition/assign/comment/tags) but no
        // webhook reconciliation, so freshness is "manual" like Jira. The deferred
        // start-work/conversation-link affordance has no backend capability flag; it
        // is gated client-side, preserving the deferral without a half-wired button.
        capabilities: if settings.enabled && is_valid && settings.task_search_available {
            writable_capabilities("manual")
        } else {
            read_only_capabilities("manual")
        },
        fetched_at: Some(now_string()),
        stale_at: None,
        permission_message: (!settings.task_search_available && is_valid)
            .then(|| "ClickUp task search is not available for this connection.".to_string()),
        error_message: (!is_valid && settings.enabled).then(|| {
            settings
                .last_error
                .clone()
                .unwrap_or_else(|| "ClickUp integration is not valid.".to_string())
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
        label_write: false,
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
        label_write: true,
        freshness: freshness.to_string(),
    }
}

fn column_status_catalog_scope(
    provider: &str,
    container_id: Option<&str>,
) -> Result<Option<TicketingStatusCatalogScope>, String> {
    match provider {
        PROVIDER_JIRA => Ok(container_selected_key(container_id).map(|project_key| {
            TicketingStatusCatalogScope {
                provider: PROVIDER_JIRA.to_string(),
                scope_kind: "jira_project".to_string(),
                scope_id: project_key.to_string(),
            }
        })),
        PROVIDER_LINEAR => {
            if let Some(team_id) = container_selected_key(container_id)
                .and_then(|value| value.strip_prefix("team:"))
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Ok(Some(TicketingStatusCatalogScope {
                    provider: PROVIDER_LINEAR.to_string(),
                    scope_kind: "linear_team".to_string(),
                    scope_id: team_id.to_string(),
                }));
            }
            Ok(Some(TicketingStatusCatalogScope {
                provider: PROVIDER_LINEAR.to_string(),
                scope_kind: "linear_global".to_string(),
                scope_id: "all".to_string(),
            }))
        }
        PROVIDER_CLICKUP => Ok(clickup_status_catalog_scope(clickup_container_scope(
            container_id,
        ))),
        _ => Err(format!("Unsupported ticketing provider: {provider}")),
    }
}

fn normalize_status_catalog_scope(
    provider: String,
    scope_kind: String,
    scope_id: String,
) -> Result<TicketingStatusCatalogScope, String> {
    validate_provider(&provider)?;
    let scope_kind = scope_kind.trim();
    let scope_id = scope_id.trim();
    if scope_id.is_empty() {
        return Err("Status scope id is required".to_string());
    }
    match provider.as_str() {
        PROVIDER_JIRA if scope_kind == "jira_project" => Ok(TicketingStatusCatalogScope {
            provider,
            scope_kind: scope_kind.to_string(),
            scope_id: scope_id.to_string(),
        }),
        PROVIDER_LINEAR if scope_kind == "linear_team" => Ok(TicketingStatusCatalogScope {
            provider,
            scope_kind: scope_kind.to_string(),
            scope_id: scope_id.to_string(),
        }),
        PROVIDER_LINEAR if scope_kind == "linear_global" => Ok(TicketingStatusCatalogScope {
            provider,
            scope_kind: scope_kind.to_string(),
            scope_id: "all".to_string(),
        }),
        PROVIDER_CLICKUP
            if matches!(
                scope_kind,
                "clickup_space" | "clickup_folder" | "clickup_list"
            ) =>
        {
            let scope_id = clickup_status_scope_id(scope_kind, scope_id)?;
            Ok(TicketingStatusCatalogScope {
                provider,
                scope_kind: scope_kind.to_string(),
                scope_id,
            })
        }
        _ => Err(format!(
            "Unsupported status scope for provider {provider}: {scope_kind}"
        )),
    }
}

async fn sync_status_catalog_for_scope(
    state: &AppState,
    scope: &TicketingStatusCatalogScope,
) -> Result<Vec<TicketingStatusCatalogEntry>, String> {
    let observed = observed_statuses_for_scope(state, scope).await?;
    state
        .ticketing_status_catalog_service
        .sync_observed_statuses(
            &scope.provider,
            &scope.scope_kind,
            &scope.scope_id,
            observed,
        )
        .await
        .map_err(|error| error.to_string())
}

async fn observed_statuses_for_scope(
    state: &AppState,
    scope: &TicketingStatusCatalogScope,
) -> Result<Vec<ObservedTicketingStatus>, String> {
    match scope.provider.as_str() {
        PROVIDER_JIRA => {
            let mut statuses = state
                .atlassian_integration_service
                .list_jira_project_statuses(&scope.scope_id)
                .await?;
            statuses.sort_by_key(|status| jira_status_category_rank(&status.category));
            Ok(statuses
                .into_iter()
                .enumerate()
                .map(|(index, status)| ObservedTicketingStatus {
                    provider_status_id: status.id,
                    provider_status_name: status.name,
                    provider_category: status.category.clone(),
                    provider_color: None,
                    provider_order: Some(index as i64),
                    is_terminal: status.category == "done",
                    metadata_json: None,
                })
                .collect())
        }
        PROVIDER_LINEAR => {
            let team_id = (scope.scope_kind == "linear_team").then_some(scope.scope_id.as_str());
            state
                .linear_integration_service
                .list_workflow_states(team_id)
                .await
                .map(|states| {
                    states
                        .into_iter()
                        .enumerate()
                        .map(|(index, state)| ObservedTicketingStatus {
                            provider_status_id: state.id,
                            provider_status_name: state.name,
                            provider_category: state.category.clone(),
                            provider_color: state.color,
                            provider_order: Some(index as i64),
                            is_terminal: state.category == "done",
                            metadata_json: None,
                        })
                        .collect()
                })
        }
        PROVIDER_CLICKUP => {
            let mut statuses = clickup_statuses_for_catalog_scope(state, scope).await?;
            statuses.sort_by_key(|status| status.orderindex.unwrap_or(i64::MAX));
            Ok(statuses
                .into_iter()
                .enumerate()
                .map(|(index, status)| {
                    let raw_status_id = status.id.clone();
                    ObservedTicketingStatus {
                        provider_status_id: state_id(&status.status),
                        provider_status_name: status.status,
                        provider_category: status.category.clone(),
                        provider_color: status.color,
                        provider_order: Some(status.orderindex.unwrap_or(index as i64)),
                        is_terminal: status.category == "done",
                        metadata_json: raw_status_id
                            .map(|raw_id| json!({ "clickupStatusId": raw_id }).to_string()),
                    }
                })
                .collect())
        }
        _ => Err(format!(
            "Unsupported ticketing provider: {}",
            scope.provider
        )),
    }
}

async fn clickup_statuses_for_catalog_scope(
    state: &AppState,
    scope: &TicketingStatusCatalogScope,
) -> Result<Vec<ClickUpStatus>, String> {
    match scope.scope_kind.as_str() {
        "clickup_space" => clickup_aggregate_space_statuses(state, &scope.scope_id).await,
        "clickup_folder" => {
            state
                .clickup_integration_service
                .list_folder_statuses(&scope.scope_id)
                .await
        }
        "clickup_list" => {
            state
                .clickup_integration_service
                .list_list_statuses(&scope.scope_id)
                .await
        }
        _ => Err(format!(
            "Unsupported ClickUp status scope: {}",
            scope.scope_kind
        )),
    }
}

async fn clickup_aggregate_space_statuses(
    state: &AppState,
    space_id: &str,
) -> Result<Vec<ClickUpStatus>, String> {
    let mut statuses = Vec::new();
    let mut seen = BTreeSet::new();
    append_clickup_statuses(
        &mut statuses,
        &mut seen,
        state
            .clickup_integration_service
            .list_statuses(space_id)
            .await?,
    );

    let folders = state
        .clickup_integration_service
        .list_folders(space_id)
        .await?;
    for folder in folders {
        append_clickup_statuses(
            &mut statuses,
            &mut seen,
            state
                .clickup_integration_service
                .list_folder_statuses(&folder.id)
                .await?,
        );
        for list in state
            .clickup_integration_service
            .list_folder_lists(&folder.id)
            .await?
        {
            append_clickup_statuses(
                &mut statuses,
                &mut seen,
                state
                    .clickup_integration_service
                    .list_list_statuses(&list.id)
                    .await?,
            );
        }
    }

    for list in state
        .clickup_integration_service
        .list_folderless_lists(space_id)
        .await?
    {
        append_clickup_statuses(
            &mut statuses,
            &mut seen,
            state
                .clickup_integration_service
                .list_list_statuses(&list.id)
                .await?,
        );
    }

    statuses.sort_by_key(|status| {
        (
            jira_status_category_rank(&status.category),
            status.orderindex.unwrap_or(i64::MAX),
            status.status.to_lowercase(),
        )
    });
    for (index, status) in statuses.iter_mut().enumerate() {
        status.orderindex = Some(index as i64);
    }
    Ok(statuses)
}

fn append_clickup_statuses(
    target: &mut Vec<ClickUpStatus>,
    seen: &mut BTreeSet<String>,
    mut source: Vec<ClickUpStatus>,
) {
    source.sort_by_key(|status| {
        (
            status.orderindex.unwrap_or(i64::MAX),
            status.status.to_lowercase(),
        )
    });
    for mut status in source {
        let key = state_id(&status.status);
        if !seen.insert(key) {
            continue;
        }
        status.orderindex = Some(target.len() as i64);
        target.push(status);
    }
}

fn status_presentation_patch(
    input: TicketingStatusPresentationPatchInput,
) -> Result<TicketingStatusPresentationPatch, String> {
    let provider_status_id = input.provider_status_id.trim();
    if provider_status_id.is_empty() {
        return Err("Provider status id is required".to_string());
    }
    let color_override = match input.color_override {
        Some(Some(value)) => {
            let value = value.trim();
            Some((!value.is_empty()).then(|| value.to_string()))
        }
        Some(None) => Some(None),
        None => None,
    };
    Ok(TicketingStatusPresentationPatch {
        provider_status_id: provider_status_id.to_string(),
        display_order: input.display_order,
        color_override,
        is_visible: input.is_visible,
    })
}

fn catalog_entries_to_columns(
    entries: Vec<TicketingStatusCatalogEntry>,
) -> Vec<TicketingColumnResponse> {
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| status_catalog_entry_column(entry, index))
        .collect()
}

fn status_catalog_entry_column(
    entry: TicketingStatusCatalogEntry,
    order: usize,
) -> TicketingColumnResponse {
    let color = entry
        .color_override
        .clone()
        .or_else(|| entry.provider_color.clone());
    TicketingColumnResponse {
        id: entry.provider_status_id,
        name: entry.provider_status_name,
        category: entry.provider_category,
        order,
        color,
        provider_color: entry.provider_color,
        color_override: entry.color_override,
        provider_order: entry.provider_order,
        display_order: Some(entry.display_order),
        scope_kind: Some(entry.scope_kind),
        scope_id: Some(entry.scope_id),
        is_visible: Some(entry.is_visible),
        is_terminal: Some(entry.is_terminal),
        stale: Some(entry.stale_since.is_some()),
        last_seen_at: entry.last_seen_at.map(|value| value.to_rfc3339()),
        stale_since: entry.stale_since.map(|value| value.to_rfc3339()),
    }
}

fn status_catalog_entry_response(
    entry: TicketingStatusCatalogEntry,
) -> TicketingStatusCatalogEntryResponse {
    let color = entry
        .color_override
        .clone()
        .or_else(|| entry.provider_color.clone());
    TicketingStatusCatalogEntryResponse {
        id: entry.id,
        provider: entry.provider,
        scope_kind: entry.scope_kind,
        scope_id: entry.scope_id,
        provider_status_id: entry.provider_status_id,
        provider_status_name: entry.provider_status_name,
        provider_category: entry.provider_category,
        provider_color: entry.provider_color,
        provider_order: entry.provider_order,
        display_order: entry.display_order,
        color_override: entry.color_override,
        color,
        is_visible: entry.is_visible,
        is_terminal: entry.is_terminal,
        stale: entry.stale_since.is_some(),
        last_seen_at: entry.last_seen_at.map(|value| value.to_rfc3339()),
        stale_since: entry.stale_since.map(|value| value.to_rfc3339()),
        updated_at: entry.updated_at.to_rfc3339(),
    }
}

// Test-only helper: a provider-neutral default column set. No production path uses
// it now that Jira columns load per project (None → empty) and Linear always
// fetches workflow states.
#[cfg(test)]
fn default_ticketing_columns() -> Vec<TicketingColumnResponse> {
    vec![
        ticketing_column("todo", "To Do", "todo", 0),
        ticketing_column("in_progress", "In Progress", "in_progress", 1),
        ticketing_column("done", "Done", "done", 2),
        ticketing_column("other", "Other", "other", 3),
    ]
}

#[cfg(test)]
fn ticketing_column(id: &str, name: &str, category: &str, order: usize) -> TicketingColumnResponse {
    TicketingColumnResponse {
        id: id.to_string(),
        name: name.to_string(),
        category: category.to_string(),
        order,
        color: None,
        provider_color: None,
        color_override: None,
        provider_order: None,
        display_order: None,
        scope_kind: None,
        scope_id: None,
        is_visible: None,
        is_terminal: None,
        stale: None,
        last_seen_at: None,
        stale_since: None,
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
        assignees: Vec::new(),
        watchers: Vec::new(),
        reporter: None,
        labels: Vec::new(),
        sprints: Vec::new(),
        project: None,
        priority: None,
        updated_at: now_string(),
        url: summary.url,
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
        current_user_watching: false,
    }
}

/// Normalize the optional container id to a non-empty selected project key, or
/// `None` when no project is selected (the "All projects" / force-select case).
fn container_selected_key(container_id: Option<&str>) -> Option<&str> {
    container_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Map a Jira project to a ticketing container. The container `id` is the project
/// **key** because both the statuses endpoint and the issue JQL key on the
/// project key (and the frontend stores `containerId` → passes it back).
fn jira_project_to_container(project: JiraProjectSummary) -> TicketingContainerResponse {
    TicketingContainerResponse {
        provider: PROVIDER_JIRA.to_string(),
        id: project.key.clone(),
        key: Some(project.key),
        name: project.name,
        kind: "project".to_string(),
        parent_id: None,
        ticket_count: None,
    }
}

/// Display rank for a normalized status category so Jira columns read
/// left-to-right as To Do → In Progress → Done. Jira's status endpoint exposes
/// no explicit order; category is the only stable ordering signal.
fn jira_status_category_rank(category: &str) -> u8 {
    match category {
        "todo" => 0,
        "in_progress" => 1,
        "done" => 2,
        _ => 3,
    }
}

/// Map a project-scoped Jira issue into a ticket summary, populating the real
/// status/assignee/labels/updated/priority/url (replaces the lossy
/// `jira_summary_to_ticket` for project-scoped results).
fn jira_issue_detail_to_ticket(issue: JiraIssueDetail) -> TicketSummaryResponse {
    let state_name = issue
        .status_name
        .clone()
        .unwrap_or_else(|| "Provider result".to_string());
    let state = TicketStateResponse {
        id: issue.status_id.unwrap_or_else(|| state_id(&state_name)),
        category: issue
            .status_category
            .unwrap_or_else(|| state_category(&state_name)),
        name: state_name,
        color: None,
    };
    let assignee = issue.assignee_name.as_deref().map(|name| {
        let mut person = named_person(name);
        person.avatar_url = issue.assignee_avatar.clone();
        person
    });
    let assignees = assignee.iter().cloned().collect();
    TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_JIRA.to_string(),
            id: issue.key.clone(),
            key: Some(issue.key),
        },
        title: issue.title,
        state,
        assignee,
        assignees,
        watchers: Vec::new(),
        reporter: None,
        labels: issue.labels,
        sprints: Vec::new(),
        project: None,
        priority: issue.priority,
        updated_at: issue.updated.unwrap_or_else(now_string),
        url: issue.url,
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
        current_user_watching: false,
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
    let assignee = summary.assignee.as_deref().map(named_person);
    let assignees = assignee.iter().cloned().collect();
    TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_LINEAR.to_string(),
            id: summary.id,
            key: summary.key,
        },
        title: summary.title,
        state,
        assignee,
        assignees,
        watchers: Vec::new(),
        reporter: None,
        labels: summary.labels,
        sprints: Vec::new(),
        project: summary.project,
        priority: None,
        updated_at: summary.updated_at.unwrap_or_else(now_string),
        url: summary.url,
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
        current_user_watching: false,
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

    let conversation_associations =
        project_ticket_conversation_associations(state, provider, &project_id).await?;

    // Load the project's workspaces once and roll up PR state by conversation
    // (open flag + number/url/status + the workspace `updated_at` used to rank
    // fallback PRs), so the per-ticket loop below does not add a second N+1 query.
    let pr_by_conversation: HashMap<String, ConversationPrRollup> = state
        .agent_conversation_workspace_repo
        .get_by_project_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|workspace| {
            let open = is_open_pr(
                workspace.publication_pr_number,
                workspace.publication_pr_status.as_deref(),
            );
            (
                workspace.conversation_id.to_string(),
                ConversationPrRollup {
                    open,
                    pr_number: workspace.publication_pr_number,
                    pr_url: workspace.publication_pr_url,
                    pr_status: workspace.publication_pr_status,
                    updated_at: workspace.updated_at,
                },
            )
        })
        .collect();

    let mut hydrated = Vec::with_capacity(items.len());
    for mut item in items {
        let reference = ticket_ref_to_composer_reference(provider, &item.ref_);
        let associations = linked_agent_conversation_associations_from_batch(
            provider,
            &project_id,
            &reference,
            &conversation_associations,
        )?;
        item.association_count = associations.len();

        // All workspaces (carrying a PR number) linked to this ticket.
        let prs: Vec<&ConversationPrRollup> = associations
            .iter()
            .filter_map(|association| pr_by_conversation.get(&association.id))
            .filter(|rollup| rollup.pr_number.is_some())
            .collect();
        // `open_pr_count` keeps its meaning: number of OPEN PRs (unchanged).
        item.open_pr_count = prs.iter().filter(|rollup| rollup.open).count();

        if let Some(representative) = representative_pr(&prs) {
            item.open_pr_number = representative.pr_number;
            item.open_pr_url = representative.pr_url.clone();
            item.open_pr_status = representative.pr_status.clone();
        }
        hydrated.push(item);
    }
    Ok(hydrated)
}

/// Per-conversation PR rollup used to pick the representative PR for a ticket's
/// list PR column without an N+1 query (workspaces are batch-loaded once).
#[derive(Debug, Clone)]
struct ConversationPrRollup {
    open: bool,
    pr_number: Option<i64>,
    pr_url: Option<String>,
    pr_status: Option<String>,
    updated_at: chrono::DateTime<Utc>,
}

/// Pick the representative PR for a ticket's list column from its linked
/// workspaces (all of which already carry a PR number): prefer an open PR, then
/// fall back to the most-recent PR by workspace `updated_at` regardless of
/// status (merged/closed/draft). Ties break on the most-recent `updated_at`.
fn representative_pr<'a>(prs: &[&'a ConversationPrRollup]) -> Option<&'a ConversationPrRollup> {
    let open = prs
        .iter()
        .filter(|rollup| rollup.open)
        .max_by_key(|rollup| rollup.updated_at)
        .copied();
    open.or_else(|| prs.iter().max_by_key(|rollup| rollup.updated_at).copied())
}

fn filter_ticket_summaries(
    items: Vec<TicketSummaryResponse>,
    filters: Option<&TicketFiltersInput>,
) -> Vec<TicketSummaryResponse> {
    let Some(filters) = filters else {
        return items
            .into_iter()
            .filter(|ticket| !ticket_has_terminal_state(ticket))
            .collect();
    };
    items
        .into_iter()
        .filter(|ticket| ticket_matches_filters(ticket, filters))
        .collect()
}

fn ticket_matches_filters(ticket: &TicketSummaryResponse, filters: &TicketFiltersInput) -> bool {
    if let Some(text) = filters
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
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

    if let Some(state_ids) = filters
        .state_ids
        .as_ref()
        .filter(|values| !values.is_empty())
    {
        let ticket_state_id = ticket.state.id.as_str();
        let ticket_state_category = ticket.state.category.as_str();
        if !state_ids
            .iter()
            .any(|state_id| state_id == ticket_state_id || state_id == ticket_state_category)
        {
            return false;
        }
    } else if ticket_has_terminal_state(ticket) {
        return false;
    }

    let assignees = ticket_assignee_filters(filters);
    if !assignees.is_empty() {
        let ticket_assignees: Vec<&TicketingPersonResponse> = ticket
            .assignees
            .iter()
            .chain(ticket.assignee.iter())
            .collect();
        let matches_named_assignee = assignees
            .iter()
            .filter(|assignee| assignee.as_str() != UNASSIGNED_ASSIGNEE_FILTER)
            .map(|assignee| assignee.to_ascii_lowercase())
            .any(|assignee| {
                ticket_assignees.iter().any(|ticket_assignee| {
                    ticket_assignee_matches_filter(ticket_assignee, &assignee)
                })
            });
        let matches_unassigned = assignees
            .iter()
            .any(|assignee| assignee == UNASSIGNED_ASSIGNEE_FILTER)
            && ticket_assignees.is_empty();
        if !matches_named_assignee && !matches_unassigned {
            return false;
        }
    }

    if let Some(sprint) = filters
        .sprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !ticket_sprint_names(ticket)
            .iter()
            .any(|ticket_sprint| ticket_sprint.eq_ignore_ascii_case(sprint))
        {
            return false;
        }
    }

    if filters.watcher_me.unwrap_or(false) && !ticket.current_user_watching {
        return false;
    }

    if let Some(labels) = filters.labels.as_ref().filter(|values| !values.is_empty()) {
        if !labels.iter().all(|required_label| {
            ticket
                .labels
                .iter()
                .any(|label| label.eq_ignore_ascii_case(required_label))
        }) {
            return false;
        }
    }

    true
}

fn ticket_assignee_filters(filters: &TicketFiltersInput) -> Vec<String> {
    let mut assignees = BTreeSet::new();
    if let Some(values) = filters.assignees.as_ref() {
        for value in values {
            let value = value.trim();
            if !value.is_empty() {
                assignees.insert(value.to_string());
            }
        }
    }
    if let Some(value) = filters.assignee.as_deref().map(str::trim) {
        if !value.is_empty() {
            assignees.insert(value.to_string());
        }
    }
    assignees.into_iter().collect()
}

fn ticket_has_terminal_state(ticket: &TicketSummaryResponse) -> bool {
    matches!(ticket.state.category.as_str(), "done" | "closed")
}

fn ticket_assignee_matches_filter(
    ticket_assignee: &TicketingPersonResponse,
    assignee: &str,
) -> bool {
    let haystacks = [
        Some(ticket_assignee.name.as_str()),
        ticket_assignee.id.as_deref(),
        ticket_assignee.email.as_deref(),
    ];
    haystacks.into_iter().flatten().any(|value| {
        let value = value.to_ascii_lowercase();
        value.contains(assignee) || assignee.contains(&value)
    })
}

fn ticket_sprint_names(ticket: &TicketSummaryResponse) -> Vec<String> {
    let mut names = Vec::new();
    for sprint in &ticket.sprints {
        let sprint = sprint.trim();
        if !sprint.is_empty() && !names.iter().any(|name: &String| name == sprint) {
            names.push(sprint.to_string());
        }
    }
    if names.is_empty() {
        if let Some(project) = ticket
            .project
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.to_ascii_lowercase().contains("sprint"))
        {
            names.push(project.to_string());
        }
    }
    names
}

fn jira_content_to_detail(content: AtlassianResourceContent) -> TicketDetailResponse {
    let assignee = content.assignee.as_deref().map(named_person);
    let assignees = assignee.iter().cloned().collect();
    let summary = TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_JIRA.to_string(),
            id: content.id.clone(),
            key: content.key.clone(),
        },
        title: content.title.clone(),
        state: ticket_state(content.status.as_deref().unwrap_or("Provider result")),
        assignee,
        assignees,
        watchers: Vec::new(),
        reporter: content.reporter.as_deref().map(named_person),
        labels: Vec::new(),
        sprints: Vec::new(),
        project: None,
        priority: None,
        updated_at: content.updated_at_remote.clone().unwrap_or_else(now_string),
        url: content.url.clone(),
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
        current_user_watching: false,
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
                attachments: Vec::new(),
                replies: Vec::new(),
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
    let assignee = content.assignee.as_deref().map(named_person);
    let assignees = assignee.iter().cloned().collect();
    let summary = TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_LINEAR.to_string(),
            id: content.id.clone(),
            key: content.key.clone(),
        },
        title: content.title.clone(),
        state: ticket_state(content.state_name.as_deref().unwrap_or("Provider result")),
        assignee,
        assignees,
        watchers: Vec::new(),
        reporter: content.creator.as_deref().map(named_person),
        labels: content.labels.clone(),
        sprints: Vec::new(),
        project: content.project.clone(),
        priority: None,
        updated_at: content.updated_at.clone().unwrap_or_else(now_string),
        url: content.url.clone(),
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
        current_user_watching: false,
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
        attachments: content
            .attachments
            .into_iter()
            .map(|attachment| TicketAttachmentResponse {
                id: Some(attachment.id),
                filename: attachment.title,
                mime_type: None,
                size: None,
                url: Some(attachment.url),
            })
            .collect(),
        transitions: Vec::new(),
        fetched_at: Some(now_string()),
    }
}

/// Map ClickUp hierarchy nodes to ticketing containers.
fn clickup_space_to_container(space: ClickUpSpace) -> TicketingContainerResponse {
    TicketingContainerResponse {
        provider: PROVIDER_CLICKUP.to_string(),
        id: clickup_space_container_id(&space.id),
        key: Some("Space".to_string()),
        name: space.name,
        kind: "space".to_string(),
        parent_id: None,
        ticket_count: None,
    }
}

async fn clickup_location_containers_for_space(
    state: &AppState,
    space_id: &str,
) -> Result<Vec<TicketingContainerResponse>, String> {
    let mut containers = Vec::new();
    let folders = state
        .clickup_integration_service
        .list_folders(space_id)
        .await?;
    for folder in folders {
        let folder_id = folder.id.clone();
        containers.push(clickup_folder_to_container(folder, space_id));
        for list in state
            .clickup_integration_service
            .list_folder_lists(&folder_id)
            .await?
        {
            containers.push(clickup_list_to_container(
                list,
                &clickup_folder_container_id(&folder_id),
            ));
        }
    }

    for list in state
        .clickup_integration_service
        .list_folderless_lists(space_id)
        .await?
    {
        containers.push(clickup_list_to_container(
            list,
            &clickup_space_container_id(space_id),
        ));
    }
    Ok(containers)
}

fn clickup_folder_to_container(
    folder: ClickUpFolder,
    fallback_space_id: &str,
) -> TicketingContainerResponse {
    let parent_space_id = folder.space_id.as_deref().unwrap_or(fallback_space_id);
    TicketingContainerResponse {
        provider: PROVIDER_CLICKUP.to_string(),
        id: clickup_folder_container_id(&folder.id),
        key: Some("Folder".to_string()),
        name: folder.name,
        kind: "folder".to_string(),
        parent_id: Some(clickup_space_container_id(parent_space_id)),
        ticket_count: None,
    }
}

fn clickup_list_to_container(
    list: ClickUpList,
    fallback_parent_id: &str,
) -> TicketingContainerResponse {
    let parent_id = list
        .folder_id
        .as_deref()
        .map(clickup_folder_container_id)
        .or_else(|| list.space_id.as_deref().map(clickup_space_container_id))
        .unwrap_or_else(|| fallback_parent_id.to_string());
    TicketingContainerResponse {
        provider: PROVIDER_CLICKUP.to_string(),
        id: clickup_list_container_id(&list.id),
        key: Some("List".to_string()),
        name: list.name,
        kind: "list".to_string(),
        parent_id: Some(parent_id),
        ticket_count: None,
    }
}

fn clickup_space_container_id(space_id: &str) -> String {
    format!("space:{space_id}")
}

fn clickup_folder_container_id(folder_id: &str) -> String {
    format!("folder:{folder_id}")
}

fn clickup_list_container_id(list_id: &str) -> String {
    format!("list:{list_id}")
}

/// Map a ClickUp task summary into a ticket summary. The `state.id` is derived from
/// the status name (ClickUp carries no task-level status id) so it aligns with the
/// column id for kanban grouping; the category comes from the already-derived
/// `status.type` mapping, falling back to a name-based heuristic. ClickUp tags map
/// to labels; the full ClickUp assignee list is preserved while the legacy
/// single-assignee slot keeps the first assignee for compatibility.
fn clickup_summary_to_ticket(summary: ClickUpTaskSummary) -> TicketSummaryResponse {
    let sprints = clickup_sprint_names(&summary);
    let state_name = summary
        .status_name
        .clone()
        .unwrap_or_else(|| "Provider result".to_string());
    let state = TicketStateResponse {
        id: state_id(&state_name),
        category: summary
            .status_category
            .clone()
            .unwrap_or_else(|| state_category(&state_name)),
        name: state_name,
        color: summary.status_color,
    };
    let assignees: Vec<TicketingPersonResponse> = summary
        .assignees
        .iter()
        .map(|name| named_person(name))
        .collect();
    let watchers: Vec<TicketingPersonResponse> = summary
        .watchers
        .iter()
        .map(clickup_user_to_person_response)
        .collect();
    let assignee = assignees.first().cloned();
    TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_CLICKUP.to_string(),
            id: summary.id,
            key: summary.custom_id,
        },
        title: summary.name,
        state,
        assignee,
        assignees,
        watchers,
        reporter: None,
        labels: summary.tags,
        sprints,
        project: summary.list_name,
        priority: None,
        updated_at: summary.updated_at.unwrap_or_else(now_string),
        url: summary.url,
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
        current_user_watching: false,
    }
}

fn clickup_sprint_names(summary: &ClickUpTaskSummary) -> Vec<String> {
    let mut names = Vec::new();
    for sprint in &summary.sprint_names {
        let sprint = sprint.trim();
        if !sprint.is_empty() && !names.iter().any(|name: &String| name == sprint) {
            names.push(sprint.to_string());
        }
    }
    if names.is_empty() {
        if let Some(list_name) = summary
            .list_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.to_ascii_lowercase().contains("sprint"))
        {
            names.push(list_name.to_string());
        }
    }
    names
}

fn clickup_summary_assigned_to_user(summary: &ClickUpTaskSummary, user: &ClickUpUser) -> bool {
    summary.assignee_ids.contains(&user.id)
        || summary.assignees.iter().any(|assignee| {
            user.username
                .as_deref()
                .is_some_and(|username| assignee.eq_ignore_ascii_case(username))
                || user
                    .email
                    .as_deref()
                    .is_some_and(|email| assignee.eq_ignore_ascii_case(email))
        })
}

fn clickup_summary_watched_by_user(summary: &ClickUpTaskSummary, user: &ClickUpUser) -> bool {
    summary
        .watchers
        .iter()
        .any(|watcher| clickup_users_match(watcher, user))
}

fn clickup_users_match(left: &ClickUpUser, right: &ClickUpUser) -> bool {
    left.id == right.id
        || left
            .username
            .as_deref()
            .zip(right.username.as_deref())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        || left
            .email
            .as_deref()
            .zip(right.email.as_deref())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
}

/// Map full ClickUp task content into a ticket detail. Mirrors
/// `clickup_summary_to_ticket` for the summary block (task content carries no
/// status color), uses the task description for the markdown/text body, maps the
/// creator to the reporter, and maps ClickUp comments. ClickUp task content has no
/// attachments here, and transitions are loaded separately.
fn clickup_content_to_detail(content: ClickUpTaskContent) -> TicketDetailResponse {
    let state_name = content
        .status_name
        .clone()
        .unwrap_or_else(|| "Provider result".to_string());
    let state = TicketStateResponse {
        id: state_id(&state_name),
        category: content
            .status_category
            .clone()
            .unwrap_or_else(|| state_category(&state_name)),
        name: state_name,
        color: None,
    };
    let assignees: Vec<TicketingPersonResponse> = content
        .assignees
        .iter()
        .map(|name| named_person(name))
        .collect();
    let watchers: Vec<TicketingPersonResponse> = content
        .watchers
        .iter()
        .map(clickup_user_to_person_response)
        .collect();
    let assignee = assignees.first().cloned();
    let summary = TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: PROVIDER_CLICKUP.to_string(),
            id: content.id.clone(),
            key: content.custom_id.clone(),
        },
        title: content.name.clone(),
        state,
        assignee,
        assignees,
        watchers,
        reporter: content.creator.as_deref().map(named_person),
        labels: content.tags.clone(),
        sprints: Vec::new(),
        project: content.list_name.clone(),
        priority: None,
        updated_at: content.updated_at.clone().unwrap_or_else(now_string),
        url: content.url.clone(),
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
        current_user_watching: false,
    };
    TicketDetailResponse {
        summary,
        description_markdown: Some(content.description.clone()),
        description_text: Some(content.description),
        acceptance_criteria_markdown: None,
        comments: content
            .comments
            .into_iter()
            .map(ticket_comment_from_clickup_comment)
            .collect(),
        attachments: content
            .attachments
            .into_iter()
            .map(|attachment| TicketAttachmentResponse {
                id: attachment.id,
                filename: attachment.filename,
                mime_type: attachment.mime_type,
                size: attachment.size,
                url: attachment.url,
            })
            .collect(),
        transitions: Vec::new(),
        fetched_at: Some(now_string()),
    }
}

fn ticket_comment_from_clickup_comment(comment: ClickUpComment) -> TicketCommentResponse {
    TicketCommentResponse {
        id: Some(comment.id),
        author: comment.author_name.as_deref().map(named_person),
        body_markdown: comment.body.clone(),
        body_text: comment.body,
        created_at: comment.created_at,
        // ClickUp comment payloads do not carry an updated timestamp.
        updated_at: None,
        attachments: comment
            .attachments
            .into_iter()
            .map(|attachment| TicketAttachmentResponse {
                id: attachment.id,
                filename: attachment.filename,
                mime_type: attachment.mime_type,
                size: attachment.size,
                url: attachment.url,
            })
            .collect(),
        replies: comment
            .replies
            .into_iter()
            .map(ticket_comment_from_clickup_comment)
            .collect(),
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

fn clickup_user_to_person_response(user: &ClickUpUser) -> TicketingPersonResponse {
    TicketingPersonResponse {
        id: Some(user.id.to_string()),
        name: user
            .username
            .clone()
            .or_else(|| user.email.clone())
            .unwrap_or_else(|| user.id.to_string()),
        email: user.email.clone(),
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
    // Jira composer references live under the `atlassian` provider with a `jira`
    // kind; Linear and ClickUp use their own name for both provider and kind.
    let (ref_provider, ref_kind) = match provider {
        PROVIDER_JIRA => ("atlassian", "jira"),
        PROVIDER_CLICKUP => (PROVIDER_CLICKUP, PROVIDER_CLICKUP),
        _ => (PROVIDER_LINEAR, PROVIDER_LINEAR),
    };
    ComposerIntegrationReference {
        provider: ref_provider.to_string(),
        kind: ref_kind.to_string(),
        id: ticket_ref.id.clone(),
        key: ticket_ref.key.clone(),
        title: None,
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    }
}

fn ticketing_service_from_state(state: &AppState) -> TicketingService {
    TicketingService::new(
        Arc::clone(&state.atlassian_integration_service),
        Arc::clone(&state.linear_integration_service),
        Arc::clone(&state.clickup_integration_service),
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
        labels: result.labels.map(ticket_labels_response),
        refreshed_at: now_string(),
    }
}

fn ticket_labels_response(labels: TicketingLabelResult) -> TicketLabelsResponse {
    TicketLabelsResponse {
        labels: labels.labels,
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
        attachments: Vec::new(),
        replies: Vec::new(),
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
        attachments: Vec::new(),
        replies: Vec::new(),
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
        PROVIDER_CLICKUP => {
            let conversation_id_value = conversation_id.as_str();
            let already_linked = state
                .external_issue_link_service
                .list_ticket_links_for_conversation(&conversation_id_value)
                .await
                .map_err(|error| error.to_string())?
                .into_iter()
                .any(|link| {
                    link.provider.eq_ignore_ascii_case(PROVIDER_CLICKUP)
                        && (link.external_id.eq_ignore_ascii_case(reference.id.trim())
                            || match (link.external_key.as_deref(), reference.key.as_deref()) {
                                (Some(left), Some(right)) => {
                                    left.eq_ignore_ascii_case(right.trim())
                                }
                                _ => false,
                            })
                });
            if already_linked {
                return Ok(());
            }
            state
                .external_issue_link_service
                .upsert_ticket_conversation_link(TicketConversationLinkInput {
                    provider: PROVIDER_CLICKUP.to_string(),
                    external_kind: PROVIDER_CLICKUP.to_string(),
                    external_id: reference.id.clone(),
                    external_key: reference.key.clone(),
                    external_url: reference.url.clone(),
                    conversation_id: conversation_id.as_str(),
                    project_id: project_id.to_string(),
                    local_sha: None,
                    local_state: Some("active".to_string()),
                    metadata_json: Some(
                        serde_json::json!({
                            "source": "ticket_start",
                            "title": reference.title,
                            "validated_at": Utc::now().to_rfc3339(),
                        })
                        .to_string(),
                    ),
                })
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
    let associations =
        project_ticket_conversation_associations(state, provider, project_id).await?;
    linked_agent_conversation_associations_from_batch(
        provider,
        project_id,
        reference,
        &associations,
    )
}

async fn project_ticket_conversation_associations(
    state: &AppState,
    provider: &str,
    project_id: &ProjectId,
) -> Result<Vec<ProjectTicketConversationAssociation>, String> {
    let conversations = state
        .chat_conversation_repo
        .get_by_context_filtered(ChatContextType::Project, project_id.as_str(), true)
        .await
        .map_err(|error| error.to_string())?;
    let mut associations = Vec::new();

    match provider {
        PROVIDER_JIRA => {
            for conversation in conversations {
                let link = state
                    .agent_conversation_jira_issue_repo
                    .get_by_conversation_id(&conversation.id)
                    .await
                    .map_err(|error| error.to_string())?;
                if let Some(link) = link.filter(|link| link.project_id == *project_id) {
                    associations.push(ProjectTicketConversationAssociation {
                        link: ProjectTicketLink::Jira(link),
                        item: agent_conversation_association_item(
                            &conversation,
                            project_id.as_str(),
                        ),
                    });
                }
            }
        }
        PROVIDER_LINEAR => {
            for conversation in conversations {
                let link = state
                    .agent_conversation_linear_issue_repo
                    .get_by_conversation_id(&conversation.id)
                    .await
                    .map_err(|error| error.to_string())?;
                if let Some(link) = link.filter(|link| link.project_id == *project_id) {
                    associations.push(ProjectTicketConversationAssociation {
                        link: ProjectTicketLink::Linear(link),
                        item: agent_conversation_association_item(
                            &conversation,
                            project_id.as_str(),
                        ),
                    });
                }
            }
        }
        PROVIDER_CLICKUP => {
            for conversation in conversations {
                let conversation_id = conversation.id.as_str();
                let links = state
                    .external_issue_link_service
                    .list_ticket_links_for_conversation(&conversation_id)
                    .await
                    .map_err(|error| error.to_string())?;
                for link in links.into_iter().filter(|link| {
                    link.provider.eq_ignore_ascii_case(PROVIDER_CLICKUP)
                        && link.external_kind.eq_ignore_ascii_case(PROVIDER_CLICKUP)
                        && link.local_project_id.as_deref() == Some(project_id.as_str())
                }) {
                    associations.push(ProjectTicketConversationAssociation {
                        link: ProjectTicketLink::ClickUp(link),
                        item: agent_conversation_association_item(
                            &conversation,
                            project_id.as_str(),
                        ),
                    });
                }
            }
        }
        _ => return Err(format!("Unknown ticketing provider: {provider}")),
    }

    Ok(associations)
}

fn linked_agent_conversation_associations_from_batch(
    provider: &str,
    project_id: &ProjectId,
    reference: &ComposerIntegrationReference,
    associations: &[ProjectTicketConversationAssociation],
) -> Result<Vec<TicketAssociationItemResponse>, String> {
    match provider {
        PROVIDER_JIRA => {
            let reference = jira_reference_from_composer_reference(reference)
                .ok_or_else(|| "Invalid Jira ticket reference".to_string())?;
            Ok(associations
                .iter()
                .filter_map(|association| {
                    let ProjectTicketLink::Jira(link) = &association.link else {
                        return None;
                    };
                    jira_link_matches_ticket(link, project_id, &reference)
                        .then(|| association.item.clone())
                })
                .collect())
        }
        PROVIDER_LINEAR => {
            let reference =
                agent_conversation_linear_issue::linear_reference_from_composer_reference(
                    reference,
                )
                .ok_or_else(|| "Invalid Linear ticket reference".to_string())?;
            Ok(associations
                .iter()
                .filter_map(|association| {
                    let ProjectTicketLink::Linear(link) = &association.link else {
                        return None;
                    };
                    linear_link_matches_ticket(link, project_id, &reference)
                        .then(|| association.item.clone())
                })
                .collect())
        }
        PROVIDER_CLICKUP => Ok(associations
            .iter()
            .filter_map(|association| {
                let ProjectTicketLink::ClickUp(link) = &association.link else {
                    return None;
                };
                clickup_link_matches_ticket(link, project_id, reference)
                    .then(|| association.item.clone())
            })
            .collect()),
        _ => Err(format!("Unknown ticketing provider: {provider}")),
    }
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

fn clickup_link_matches_ticket(
    link: &ExternalIssueLink,
    project_id: &ProjectId,
    reference: &ComposerIntegrationReference,
) -> bool {
    link.local_project_id.as_deref() == Some(project_id.as_str())
        && (link.external_id.eq_ignore_ascii_case(reference.id.trim())
            || match (link.external_key.as_deref(), reference.key.as_deref()) {
                (Some(left), Some(right)) => left.eq_ignore_ascii_case(right.trim()),
                _ => false,
            })
}

fn agent_conversation_association_item(
    conversation: &ChatConversation,
    project_id: &str,
) -> TicketAssociationItemResponse {
    let id = conversation.id.as_str();
    TicketAssociationItemResponse {
        id: id.clone(),
        // Mirror the agents UI fallback (`conversation.title || "Untitled agent"`)
        // so the ticket panel shows the same label as the actual conversation.
        title: conversation
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Untitled agent".to_string()),
        subtitle: Some("Agent conversation".to_string()),
        status: conversation.agent_mode.map(|mode| mode.to_string()),
        active: conversation.archived_at.is_none(),
        deep_link: TicketDeepLinkResponse {
            view: "agents".to_string(),
            id,
            project_id: Some(project_id.to_string()),
        },
        branch_name: None,
        base_ref: None,
        pr_number: None,
        pr_url: None,
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
        PROVIDER_JIRA | PROVIDER_LINEAR | PROVIDER_CLICKUP => Ok(()),
        other => Err(format!("Unknown ticketing provider: {other}")),
    }
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
