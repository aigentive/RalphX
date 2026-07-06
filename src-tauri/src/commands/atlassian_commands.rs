use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::harness_runtime_registry::default_ui_feature_flags;
use crate::application::{
    AppState, AtlassianJiraAttachment, AtlassianJiraComment, AtlassianOAuthAuthorization,
    AtlassianResourceKind, AtlassianResourceSummary, AtlassianResourceUrlResolution,
};
use crate::domain::entities::{
    AgentConversationJiraIssueLink, ChatContextType, ChatConversationId, ProjectId,
};
use crate::domain::integrations::{AtlassianAuthMethod, AtlassianIntegrationSettings};
use crate::domain::services::ComposerJiraReferenceMetadata;

const ATLASSIAN_OAUTH_DISABLED_MESSAGE: &str =
    "Atlassian OAuth setup is disabled. Use API token setup for now.";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianIntegrationSettingsResponse {
    pub enabled: bool,
    pub auth_method: String,
    pub site_url: Option<String>,
    pub email: Option<String>,
    pub has_api_token: bool,
    pub oauth_client_id: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub has_oauth_client_secret: bool,
    pub has_oauth_token: bool,
    pub oauth_cloud_id: Option<String>,
    pub oauth_scopes: Option<String>,
    pub validation_status: String,
    pub jira_available: bool,
    pub confluence_available: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<AtlassianIntegrationSettings> for AtlassianIntegrationSettingsResponse {
    fn from(settings: AtlassianIntegrationSettings) -> Self {
        Self {
            enabled: settings.enabled,
            auth_method: settings.auth_method.as_str().to_string(),
            site_url: settings.site_url,
            email: settings.email,
            has_api_token: settings.token_secret_ref.is_some(),
            oauth_client_id: settings.oauth_client_id,
            oauth_redirect_uri: settings.oauth_redirect_uri,
            has_oauth_client_secret: settings.oauth_client_secret_ref.is_some(),
            has_oauth_token: settings.oauth_access_token_ref.is_some(),
            oauth_cloud_id: settings.oauth_cloud_id,
            oauth_scopes: settings.oauth_scopes,
            validation_status: settings.validation_status.as_str().to_string(),
            jira_available: settings.jira_available,
            confluence_available: settings.confluence_available,
            last_validated_at: settings.last_validated_at,
            last_error: settings.last_error,
            updated_at: settings.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAtlassianIntegrationSettingsInput {
    pub auth_method: Option<String>,
    pub site_url: Option<String>,
    pub email: Option<String>,
    pub api_token: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_client_secret: Option<String>,
    pub oauth_redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeAtlassianOAuthCodeInput {
    pub authorization_code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteAtlassianOAuthLocalCallbackInput {
    pub state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAtlassianResourcesInput {
    pub kind: String,
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAtlassianResourcesResponse {
    pub resources: Vec<AtlassianResourceSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveAtlassianResourceUrlsInput {
    pub urls: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveAtlassianResourceUrlsResponse {
    pub results: Vec<AtlassianResourceUrlResolution>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentConversationJiraIssueInput {
    pub conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignAgentConversationJiraIssueInput {
    pub conversation_id: String,
    pub project_id: Option<String>,
    pub issue_key: String,
    pub issue_id: Option<String>,
    pub title: Option<String>,
    pub issue_url: Option<String>,
    #[serde(default)]
    pub refresh: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAgentConversationJiraIssueInput {
    pub conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignAgentConversationJiraIssueToMeInput {
    pub conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearAgentConversationJiraIssueInput {
    pub conversation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationJiraIssueResponse {
    pub issue: Option<AgentConversationJiraIssueLinkResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationJiraIssueLinkResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub provider: String,
    pub issue_key: String,
    pub issue_id: Option<String>,
    pub issue_url: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub reporter: Option<String>,
    pub updated_at_remote: Option<String>,
    pub description_markdown: Option<String>,
    pub description_text: Option<String>,
    pub acceptance_criteria_markdown: Option<String>,
    pub acceptance_criteria_text: Option<String>,
    pub comments: Vec<AtlassianJiraComment>,
    pub attachments: Vec<AtlassianJiraAttachment>,
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub refresh_status: String,
    pub refresh_error: Option<String>,
    pub assigned_at: DateTime<Utc>,
    pub assigned_from_message_id: Option<String>,
    pub manually_assigned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn atlassian_oauth_enabled() -> bool {
    default_ui_feature_flags().atlassian_oauth
}

fn ensure_atlassian_oauth_enabled() -> Result<(), String> {
    if atlassian_oauth_enabled() {
        Ok(())
    } else {
        Err(ATLASSIAN_OAUTH_DISABLED_MESSAGE.to_string())
    }
}

fn save_input_requests_oauth(input: &SaveAtlassianIntegrationSettingsInput) -> bool {
    matches!(input.auth_method.as_deref(), Some("oauth"))
        || input
            .oauth_client_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        || input
            .oauth_client_secret
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        || input
            .oauth_redirect_uri
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

impl From<AgentConversationJiraIssueLink> for AgentConversationJiraIssueLinkResponse {
    fn from(link: AgentConversationJiraIssueLink) -> Self {
        let comments = serde_json::from_str::<Vec<AtlassianJiraComment>>(&link.comments_json)
            .unwrap_or_default();
        let attachments =
            serde_json::from_str::<Vec<AtlassianJiraAttachment>>(&link.attachments_json)
                .unwrap_or_default();
        Self {
            conversation_id: link.conversation_id.as_str(),
            project_id: link.project_id.as_str().to_string(),
            provider: link.provider,
            issue_key: link.issue_key,
            issue_id: link.issue_id,
            issue_url: link.issue_url,
            title: link.title,
            status: link.status,
            assignee: link.assignee,
            reporter: link.reporter,
            updated_at_remote: link.updated_at_remote,
            description_markdown: link.description_markdown,
            description_text: link.description_text,
            acceptance_criteria_markdown: link.acceptance_criteria_markdown,
            acceptance_criteria_text: link.acceptance_criteria_text,
            comments,
            attachments,
            last_refreshed_at: link.last_refreshed_at,
            refresh_status: link.refresh_status.to_string(),
            refresh_error: link.refresh_error,
            assigned_at: link.assigned_at,
            assigned_from_message_id: link
                .assigned_from_message_id
                .map(|message_id| message_id.as_str().to_string()),
            manually_assigned: link.manually_assigned,
            created_at: link.created_at,
            updated_at: link.updated_at,
        }
    }
}

fn parse_conversation_id(raw: &str) -> Result<ChatConversationId, String> {
    raw.parse::<ChatConversationId>()
        .map_err(|_| "Invalid conversationId".to_string())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

async fn resolve_assignment_project_id(
    state: &AppState,
    conversation_id: &ChatConversationId,
    explicit_project_id: Option<String>,
) -> Result<ProjectId, String> {
    if let Some(project_id) = non_empty(explicit_project_id) {
        return Ok(ProjectId::from_string(project_id));
    }
    if let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(workspace.project_id);
    }
    let conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    if conversation.context_type == ChatContextType::Project {
        return Ok(ProjectId::from_string(conversation.context_id));
    }
    Err("Unable to resolve project for Jira assignment".to_string())
}

fn link_response(
    link: Option<AgentConversationJiraIssueLink>,
) -> AgentConversationJiraIssueResponse {
    AgentConversationJiraIssueResponse {
        issue: link.map(AgentConversationJiraIssueLinkResponse::from),
    }
}

#[tauri::command]
pub async fn get_atlassian_integration_settings(
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    state
        .atlassian_integration_service
        .get_settings()
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn save_atlassian_integration_settings(
    input: SaveAtlassianIntegrationSettingsInput,
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    if save_input_requests_oauth(&input) {
        ensure_atlassian_oauth_enabled()?;
    }
    let auth_method = input
        .auth_method
        .as_deref()
        .map(str::parse::<AtlassianAuthMethod>)
        .transpose()?;
    state
        .atlassian_integration_service
        .save_settings(
            auth_method,
            input.site_url,
            input.email,
            input.api_token,
            input.oauth_client_id,
            input.oauth_client_secret,
            input.oauth_redirect_uri,
        )
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn build_atlassian_oauth_authorization_url(
    state: State<'_, AppState>,
) -> Result<AtlassianOAuthAuthorization, String> {
    ensure_atlassian_oauth_enabled()?;
    state
        .atlassian_integration_service
        .build_oauth_authorization()
        .await
}

#[tauri::command]
pub async fn start_atlassian_oauth_local_callback(
    state: State<'_, AppState>,
) -> Result<AtlassianOAuthAuthorization, String> {
    ensure_atlassian_oauth_enabled()?;
    state
        .atlassian_integration_service
        .start_oauth_local_callback()
        .await
}

#[tauri::command]
pub async fn complete_atlassian_oauth_local_callback(
    input: CompleteAtlassianOAuthLocalCallbackInput,
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    ensure_atlassian_oauth_enabled()?;
    state
        .atlassian_integration_service
        .complete_oauth_local_callback(input.state)
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn exchange_atlassian_oauth_code(
    input: ExchangeAtlassianOAuthCodeInput,
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    ensure_atlassian_oauth_enabled()?;
    state
        .atlassian_integration_service
        .exchange_oauth_code(input.authorization_code)
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn validate_atlassian_integration(
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    let settings = state.atlassian_integration_service.get_settings().await?;
    if settings.auth_method == AtlassianAuthMethod::OAuth {
        ensure_atlassian_oauth_enabled()?;
    }
    state
        .atlassian_integration_service
        .validate_and_enable()
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn disconnect_atlassian_integration(
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    state
        .atlassian_integration_service
        .disconnect()
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn search_atlassian_resources(
    input: SearchAtlassianResourcesInput,
    state: State<'_, AppState>,
) -> Result<SearchAtlassianResourcesResponse, String> {
    let settings = state.atlassian_integration_service.get_settings().await?;
    if settings.auth_method == AtlassianAuthMethod::OAuth {
        ensure_atlassian_oauth_enabled()?;
    }
    let kind = input.kind.parse::<AtlassianResourceKind>()?;
    let query = input.query.trim();
    if query.is_empty() {
        return Ok(SearchAtlassianResourcesResponse {
            resources: Vec::new(),
        });
    }
    let resources = state
        .atlassian_integration_service
        .search_resources(kind, query, input.limit.unwrap_or(10))
        .await?;
    Ok(SearchAtlassianResourcesResponse { resources })
}

#[tauri::command]
pub async fn resolve_atlassian_resource_urls(
    input: ResolveAtlassianResourceUrlsInput,
    state: State<'_, AppState>,
) -> Result<ResolveAtlassianResourceUrlsResponse, String> {
    let settings = state.atlassian_integration_service.get_settings().await?;
    if settings.auth_method == AtlassianAuthMethod::OAuth {
        ensure_atlassian_oauth_enabled()?;
    }
    let results = state
        .atlassian_integration_service
        .resolve_resource_urls(&input.urls)
        .await?;
    Ok(ResolveAtlassianResourceUrlsResponse { results })
}

#[tauri::command]
pub async fn get_agent_conversation_jira_issue(
    input: GetAgentConversationJiraIssueInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationJiraIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let link = state
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(link_response(link))
}

#[tauri::command]
pub async fn assign_agent_conversation_jira_issue(
    input: AssignAgentConversationJiraIssueInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationJiraIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let issue_key = input.issue_key.trim();
    if issue_key.is_empty() {
        return Err("Jira issue key is required".to_string());
    }
    let project_id =
        resolve_assignment_project_id(state.inner(), &conversation_id, input.project_id).await?;
    let reference = ComposerJiraReferenceMetadata {
        issue_key: issue_key.to_ascii_uppercase(),
        issue_id: non_empty(input.issue_id),
        title: non_empty(input.title),
        url: non_empty(input.issue_url),
    };
    let link = crate::application::agent_conversation_jira_issue::manual_link_from_reference(
        &conversation_id,
        &project_id,
        reference,
        Utc::now(),
    );
    let link = state
        .agent_conversation_jira_issue_repo
        .upsert(link)
        .await
        .map_err(|error| error.to_string())?;
    let link = if input.refresh.unwrap_or(true) {
        crate::application::agent_conversation_jira_issue::refresh_jira_issue_link(
            &state.agent_conversation_jira_issue_repo,
            state.atlassian_integration_service.as_ref(),
            link,
        )
        .await
        .map_err(|error| error.to_string())?
    } else {
        link
    };
    Ok(link_response(Some(link)))
}

#[tauri::command]
pub async fn refresh_agent_conversation_jira_issue(
    input: RefreshAgentConversationJiraIssueInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationJiraIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let link = state
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "No Jira issue is assigned to this conversation".to_string())?;
    let link = crate::application::agent_conversation_jira_issue::refresh_jira_issue_link(
        &state.agent_conversation_jira_issue_repo,
        state.atlassian_integration_service.as_ref(),
        link,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(link_response(Some(link)))
}

#[tauri::command]
pub async fn assign_agent_conversation_jira_issue_to_me(
    input: AssignAgentConversationJiraIssueToMeInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationJiraIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let link = state
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "No Jira issue is assigned to this conversation".to_string())?;
    state
        .atlassian_integration_service
        .assign_jira_issue_to_current_user(&link.issue_key)
        .await?;
    let link = crate::application::agent_conversation_jira_issue::refresh_jira_issue_link(
        &state.agent_conversation_jira_issue_repo,
        state.atlassian_integration_service.as_ref(),
        link,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(link_response(Some(link)))
}

#[tauri::command]
pub async fn clear_agent_conversation_jira_issue(
    input: ClearAgentConversationJiraIssueInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationJiraIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    state
        .agent_conversation_jira_issue_repo
        .clear(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(link_response(None))
}

#[cfg(test)]
#[path = "atlassian_commands_tests.rs"]
mod tests;
