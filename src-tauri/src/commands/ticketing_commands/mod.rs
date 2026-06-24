use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tauri::{AppHandle, Runtime, State};

use crate::application::clickup_integration_service::ClickUpTaskListOptions;
use crate::application::ticket_canonical_branch::ensure_ticket_canonical_branch;
use crate::application::ticketing_pr_summary::{ticket_pr_branch_summary, TicketPrBranchSummary};
use crate::application::{
    agent_conversation_jira_issue, agent_conversation_linear_issue,
    agent_conversation_start_service::{
        AgentConversationStartDeps, AgentConversationStartService, StartAgentConversationInput,
    },
    AppState, AtlassianResourceContent, AtlassianResourceKind, AtlassianResourceSummary,
    ClickUpComment, ClickUpSpace, ClickUpStatus, ClickUpTaskContent, ClickUpTaskSummary,
    ClickUpUser, JiraIssueDetail, JiraProjectSummary, JiraStatusSummary, LinearComment,
    LinearIntegrationSettings, LinearIssueContent, LinearIssueSummary, LinearLabel,
    LinearWorkflowState, TauriTicketingEventSink, TeamService, TicketAssignRequest,
    TicketCommentRequest, TicketSetLabelsRequest, TicketTransitionRequest, TicketingCommentResult,
    TicketingLabelResult, TicketingMutationResult, TicketingPersonResult, TicketingService,
    TicketingTicketIdentity, TicketingTransitionOption,
};
use crate::commands::unified_chat_commands::{
    agent_conversation_response_for_state, agent_workspace_response_for_state,
    SendAgentMessageResponse, StartAgentConversationResponse,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    is_open_pr, AgentConversationJiraIssueLink, AgentConversationLinearIssueLink,
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::integrations::{
    AtlassianIntegrationSettings, ClickUpIntegrationSettings, IntegrationValidationStatus,
    ProviderTicketOperation,
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

#[derive(Debug, Clone)]
enum ProjectTicketLink {
    Jira(AgentConversationJiraIssueLink),
    Linear(AgentConversationLinearIssueLink),
}

#[derive(Debug, Clone)]
struct ProjectTicketConversationAssociation {
    link: ProjectTicketLink,
    item: TicketAssociationItemResponse,
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
    state: State<'_, AppState>,
) -> Result<Vec<TicketingContainerResponse>, String> {
    validate_provider(&provider)?;
    let _ = project_id;
    match provider.as_str() {
        PROVIDER_JIRA => state
            .atlassian_integration_service
            .list_jira_projects(100)
            .await
            .map(|projects| {
                projects
                    .into_iter()
                    .map(jira_project_to_container)
                    .collect()
            }),
        PROVIDER_LINEAR => state
            .linear_integration_service
            .list_projects(100)
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
        // ClickUp containers are Spaces within the stored workspace; the workspace
        // id is resolved server-side from the saved settings.
        PROVIDER_CLICKUP => state
            .clickup_integration_service
            .list_spaces()
            .await
            .map(|spaces| spaces.into_iter().map(clickup_space_to_container).collect()),
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
    match provider.as_str() {
        PROVIDER_JIRA => match container_id.as_deref() {
            // Jira statuses are project-scoped; columns only load meaningfully once
            // a project is selected (matches the force-select gate). No project →
            // provider-neutral defaults.
            Some(key) => state
                .atlassian_integration_service
                .list_jira_project_statuses(key)
                .await
                .map(|mut statuses| {
                    // Jira's status endpoint exposes no order field; category is the
                    // only stable signal, so sort To Do → In Progress → Done (stable
                    // within a category, preserving the API order) for left-to-right
                    // columns.
                    statuses.sort_by_key(|status| jira_status_category_rank(&status.category));
                    statuses
                        .into_iter()
                        .enumerate()
                        .map(|(index, status)| jira_status_to_column(status, index))
                        .collect()
                }),
            // No project selected → no statuses; the dashboard forces a project pick
            // before loading per-project statuses.
            None => Ok(Vec::new()),
        },
        PROVIDER_LINEAR => state
            .linear_integration_service
            // Linear containers are projects, but workflow states are team-scoped;
            // passing a project id as a team id errors, so fetch all states.
            .list_workflow_states(None)
            .await
            .map(|states| {
                states
                    .into_iter()
                    .enumerate()
                    .map(|(index, state)| linear_workflow_state_to_column(state, index))
                    .collect()
            }),
        // ClickUp statuses are Space-scoped, so columns only load meaningfully once
        // a Space (container) is selected, mirroring the Jira project gate. ClickUp
        // exposes an explicit `orderindex`, so sort by it for left-to-right columns.
        PROVIDER_CLICKUP => match container_selected_key(container_id.as_deref()) {
            Some(space_id) => state
                .clickup_integration_service
                .list_statuses(space_id)
                .await
                .map(|mut statuses| {
                    statuses.sort_by_key(|status| status.orderindex.unwrap_or(i64::MAX));
                    statuses
                        .into_iter()
                        .enumerate()
                        .map(|(index, status)| clickup_status_to_column(status, index))
                        .collect()
                }),
            None => Ok(Vec::new()),
        },
        _ => unreachable!("provider validated above"),
    }
}

#[tauri::command]
pub async fn list_tickets(
    query: ListTicketsQuery,
    state: State<'_, AppState>,
) -> Result<TicketPageResponse, String> {
    validate_provider(&query.provider)?;
    let _ = (&query.project_id, &query.cursor, &query.sort);
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
        // With a selected project, fetch its issues (richer status/assignee/labels
        // needed for kanban columns). Without one, fall back to global text search
        // (the frontend force-select gate means this path is rarely hit, but keep
        // it functional).
        PROVIDER_JIRA => match container_selected_key(query.container_id.as_deref()) {
            Some(key) => state
                .atlassian_integration_service
                .list_jira_project_issues(key, limit)
                .await?
                .into_iter()
                .map(jira_issue_detail_to_ticket)
                .collect(),
            None => state
                .atlassian_integration_service
                .search_resources(AtlassianResourceKind::Jira, &text, limit)
                .await?
                .into_iter()
                .map(jira_summary_to_ticket)
                .collect(),
        },
        PROVIDER_LINEAR => state
            .linear_integration_service
            .search_issues(&text, limit)
            .await?
            .into_iter()
            .map(linear_summary_to_ticket)
            .collect(),
        // ClickUp tasks load via the workspace-scoped filtered-tasks endpoint
        // (Jira-like server-side scoping). A selected Space narrows the query; with
        // no Space selected the workspace returns all of its tasks. Text filtering
        // is applied provider-neutrally by `filter_ticket_summaries` below.
        PROVIDER_CLICKUP => {
            let space_ids = container_selected_key(query.container_id.as_deref())
                .map(|space_id| vec![space_id.to_string()])
                .unwrap_or_default();
            let current_user = state.clickup_integration_service.current_user().await.ok();
            state
                .clickup_integration_service
                .list_tasks(
                    space_ids,
                    ClickUpTaskListOptions {
                        query: Some(text.clone()),
                        limit: None,
                    },
                )
                .await?
                .into_iter()
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
                .collect()
        }
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

    // For workspace-creating ticket starts with no explicit branch/PR base,
    // base the new conversation off the ticket's single canonical branch so all
    // work for the ticket converges on one branch. Explicit user-selected PRs
    // and branches are preserved.
    if ticket_start_should_apply_canonical_branch(&input.start) {
        let issue_key = ticket_ref_issue_key(&input.ticket_ref);
        let canonical =
            ensure_ticket_canonical_branch(state.inner(), &project_id, &provider, &issue_key)
                .await
                .map_err(|error| error.to_string())?;
        apply_ticket_canonical_branch_base(&mut input.start, &issue_key, &canonical.branch_name);
    }

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

/// Resolve the issue key used for the ticket canonical branch: prefer the
/// provider `.key` (matches the link tables), falling back to `.id`.
fn ticket_ref_issue_key(ticket_ref: &TicketRefInput) -> String {
    ticket_ref
        .key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(ticket_ref.id.as_str())
        .to_string()
}

/// Whether a ticket start in this mode will create a base-selecting workspace
/// that should inherit the ticket's canonical branch as its base.
///
/// Only Edit and Plan modes create a local-branch-base workspace from a ticket
/// start. Chat-only starts create no workspace (effectively read-only), and the
/// other workspace modes are not reachable from a ticket start, so they skip the
/// canonical-branch base injection.
fn ticket_start_inherits_canonical_branch(mode: Option<&str>) -> bool {
    let parsed = mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("edit")
        .parse::<AgentConversationWorkspaceMode>();
    matches!(
        parsed,
        Ok(AgentConversationWorkspaceMode::Edit | AgentConversationWorkspaceMode::Plan)
    )
}

fn ticket_start_should_apply_canonical_branch(start: &StartAgentConversationInput) -> bool {
    if !ticket_start_inherits_canonical_branch(start.mode.as_deref()) {
        return false;
    }
    if start.base_source_pull_request.is_some() {
        return false;
    }
    let base_ref_kind = match start
        .base_ref_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<IdeationAnalysisBaseRefKind>)
        .transpose()
    {
        Ok(kind) => kind,
        Err(_) => return false,
    };
    matches!(
        base_ref_kind,
        None | Some(IdeationAnalysisBaseRefKind::ProjectDefault)
    )
}

/// Overwrite the conversation start base so it inherits the ticket's canonical
/// branch as a local-branch base. Uses `local_branch` (NOT `pull_request`, which
/// hard-errors in the workspace path); the existing LocalBranch path materializes
/// the branch locally if missing and forges the per-conversation branch off it.
fn apply_ticket_canonical_branch_base(
    start: &mut StartAgentConversationInput,
    issue_key: &str,
    canonical_branch_name: &str,
) {
    start.base_ref_kind = Some("local_branch".to_string());
    start.base_ref = Some(canonical_branch_name.to_string());
    start.base_display_name = Some(format!("Ticket {issue_key} ({canonical_branch_name})"));
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
        assignees: Vec::new(),
        watchers: Vec::new(),
        reporter: None,
        labels: Vec::new(),
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

/// Map a deduped Jira project status into a kanban column, preserving the real
/// provider status id/name and the normalized category.
fn jira_status_to_column(status: JiraStatusSummary, order: usize) -> TicketingColumnResponse {
    TicketingColumnResponse {
        id: status.id,
        name: status.name,
        category: status.category,
        order,
        color: None,
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
        return items;
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
    }

    if let Some(assignee) = filters
        .assignee
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let assignee = assignee.to_ascii_lowercase();
        let matches_assignee =
            ticket
                .assignees
                .iter()
                .chain(ticket.assignee.iter())
                .any(|ticket_assignee| {
                    ticket_assignee
                        .name
                        .to_ascii_lowercase()
                        .contains(&assignee)
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
                            .contains(&assignee)
                });
        if !matches_assignee {
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

/// Map a ClickUp Space to a ticketing container. Spaces reuse the existing
/// `project` container kind (no shared enum widening); the frontend supplies the
/// "Space" label for the ClickUp provider.
fn clickup_space_to_container(space: ClickUpSpace) -> TicketingContainerResponse {
    TicketingContainerResponse {
        provider: PROVIDER_CLICKUP.to_string(),
        id: space.id,
        key: None,
        name: space.name,
        kind: "project".to_string(),
        parent_id: None,
        ticket_count: None,
    }
}

/// Map a ClickUp Space status into a kanban column. ClickUp tasks expose their
/// status by NAME (no stable per-task status id), so the column id is derived from
/// the status name to match the ticket `state.id` produced by the ticket mappers
/// (kanban groups by `state.id == column.id`). `category` is the already-derived
/// `status.type` category.
fn clickup_status_to_column(status: ClickUpStatus, order: usize) -> TicketingColumnResponse {
    TicketingColumnResponse {
        id: state_id(&status.status),
        name: status.status,
        category: status.category,
        order,
        color: status.color,
    }
}

/// Map a ClickUp task summary into a ticket summary. The `state.id` is derived from
/// the status name (ClickUp carries no task-level status id) so it aligns with the
/// column id for kanban grouping; the category comes from the already-derived
/// `status.type` mapping, falling back to a name-based heuristic. ClickUp tags map
/// to labels; the full ClickUp assignee list is preserved while the legacy
/// single-assignee slot keeps the first assignee for compatibility.
fn clickup_summary_to_ticket(summary: ClickUpTaskSummary) -> TicketSummaryResponse {
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
    summary.watchers.iter().any(|watcher| clickup_users_match(watcher, user))
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
        // ClickUp start-work is supported through the provider-neutral composer
        // reference. A first-class ClickUp conversation link table is deferred,
        // so there is nothing to persist here yet.
        PROVIDER_CLICKUP => Ok(()),
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
        // ClickUp conversation-linking is deferred (no link table yet), so ClickUp
        // tickets have no conversation associations; the list still renders with a
        // zero association count rather than erroring.
        PROVIDER_CLICKUP => {}
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
        // ClickUp conversation-linking is deferred, so there are never any batched
        // ClickUp associations to match against.
        PROVIDER_CLICKUP => Ok(Vec::new()),
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
