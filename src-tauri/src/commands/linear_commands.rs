use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{AppState, LinearIntegrationSettings, LinearIssueSummary};
use crate::domain::entities::{
    AgentConversationLinearIssueLink, ChatContextType, ChatConversationId, ProjectId,
};
use crate::domain::services::SecretStore;
use crate::infrastructure::secret_store::MacosKeychainSecretStore;
use crate::infrastructure::sqlite::SqliteLinearWebhookStore;

const LINEAR_WEBHOOK_SIGNING_SECRET_REF: &str =
    "integrations/linear/default/webhook-signing-secret";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearWebhookConfigResponse {
    pub enabled: bool,
    pub has_signing_secret: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLinearWebhookSigningSecretInput {
    pub signing_secret: String,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearIntegrationSettingsResponse {
    pub enabled: bool,
    pub has_api_token: bool,
    pub validation_status: String,
    pub issue_search_available: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<LinearIntegrationSettings> for LinearIntegrationSettingsResponse {
    fn from(settings: LinearIntegrationSettings) -> Self {
        Self {
            enabled: settings.enabled,
            has_api_token: settings.token_secret_ref.is_some(),
            validation_status: settings.validation_status.as_str().to_string(),
            issue_search_available: settings.issue_search_available,
            last_validated_at: settings.last_validated_at,
            last_error: settings.last_error,
            updated_at: settings.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLinearIntegrationSettingsInput {
    pub api_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchLinearIssuesInput {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchLinearIssuesResponse {
    pub issues: Vec<LinearIssueSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentConversationLinearIssueInput {
    pub conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignAgentConversationLinearIssueInput {
    pub conversation_id: String,
    pub project_id: Option<String>,
    pub issue_id: String,
    pub issue_key: Option<String>,
    pub title: Option<String>,
    pub issue_url: Option<String>,
    #[serde(default)]
    pub refresh: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAgentConversationLinearIssueInput {
    pub conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearAgentConversationLinearIssueInput {
    pub conversation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationLinearIssueResponse {
    pub issue: Option<AgentConversationLinearIssueLinkResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationLinearIssueLinkResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub provider: String,
    pub issue_id: String,
    pub issue_key: Option<String>,
    pub issue_url: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub reporter: Option<String>,
    pub updated_at_remote: Option<String>,
    pub description_markdown: Option<String>,
    pub description_text: Option<String>,
    pub comments: Vec<serde_json::Value>,
    pub attachments: Vec<serde_json::Value>,
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub refresh_status: String,
    pub refresh_error: Option<String>,
    pub assigned_at: DateTime<Utc>,
    pub assigned_from_message_id: Option<String>,
    pub manually_assigned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AgentConversationLinearIssueLink> for AgentConversationLinearIssueLinkResponse {
    fn from(link: AgentConversationLinearIssueLink) -> Self {
        Self {
            conversation_id: link.conversation_id.as_str().to_string(),
            project_id: link.project_id.as_str().to_string(),
            provider: link.provider,
            issue_id: link.issue_id,
            issue_key: link.issue_key,
            issue_url: link.issue_url,
            title: link.title,
            status: link.status,
            assignee: link.assignee,
            reporter: link.reporter,
            updated_at_remote: link.updated_at_remote,
            description_markdown: link.description_markdown,
            description_text: link.description_text,
            comments: serde_json::from_str(&link.comments_json).unwrap_or_default(),
            attachments: serde_json::from_str(&link.attachments_json).unwrap_or_default(),
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
    Err("Unable to resolve project for Linear assignment".to_string())
}

fn link_response(
    link: Option<AgentConversationLinearIssueLink>,
) -> AgentConversationLinearIssueResponse {
    AgentConversationLinearIssueResponse {
        issue: link.map(AgentConversationLinearIssueLinkResponse::from),
    }
}

#[tauri::command]
pub async fn get_linear_webhook_config(
    state: State<'_, AppState>,
) -> Result<LinearWebhookConfigResponse, String> {
    let store = SqliteLinearWebhookStore::new(state.db.clone());
    let (enabled, signing_secret_ref) = store
        .get_config()
        .await
        .map_err(|error| error.to_string())?;

    Ok(LinearWebhookConfigResponse {
        enabled,
        has_signing_secret: signing_secret_ref.is_some(),
    })
}

#[tauri::command]
pub async fn get_linear_integration_settings(
    state: State<'_, AppState>,
) -> Result<LinearIntegrationSettingsResponse, String> {
    state
        .linear_integration_service
        .get_settings()
        .await
        .map(LinearIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn save_linear_integration_settings(
    input: SaveLinearIntegrationSettingsInput,
    state: State<'_, AppState>,
) -> Result<LinearIntegrationSettingsResponse, String> {
    state
        .linear_integration_service
        .save_settings(input.api_token)
        .await
        .map(LinearIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn validate_linear_integration(
    state: State<'_, AppState>,
) -> Result<LinearIntegrationSettingsResponse, String> {
    state
        .linear_integration_service
        .validate_and_enable()
        .await
        .map(LinearIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn disconnect_linear_integration(
    state: State<'_, AppState>,
) -> Result<LinearIntegrationSettingsResponse, String> {
    state
        .linear_integration_service
        .disconnect()
        .await
        .map(LinearIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn search_linear_issues(
    input: SearchLinearIssuesInput,
    state: State<'_, AppState>,
) -> Result<SearchLinearIssuesResponse, String> {
    let query = input.query.trim();
    if query.is_empty() {
        return Ok(SearchLinearIssuesResponse { issues: Vec::new() });
    }
    let issues = state
        .linear_integration_service
        .search_issues(query, input.limit.unwrap_or(10))
        .await?;
    Ok(SearchLinearIssuesResponse { issues })
}

#[tauri::command]
pub async fn get_agent_conversation_linear_issue(
    input: GetAgentConversationLinearIssueInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationLinearIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let link = state
        .agent_conversation_linear_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(link_response(link))
}

#[tauri::command]
pub async fn assign_agent_conversation_linear_issue(
    input: AssignAgentConversationLinearIssueInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationLinearIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let issue_id = input.issue_id.trim();
    if issue_id.is_empty()
        || issue_id.contains('\0')
        || issue_id.contains('\n')
        || issue_id.contains('\r')
    {
        return Err("Linear issue id is required".to_string());
    }
    let project_id =
        resolve_assignment_project_id(state.inner(), &conversation_id, input.project_id).await?;
    let reference =
        crate::application::agent_conversation_linear_issue::ComposerLinearReferenceMetadata {
            issue_id: issue_id.to_string(),
            issue_key: non_empty(input.issue_key),
            title: non_empty(input.title),
            url: non_empty(input.issue_url),
        };
    let link = crate::application::agent_conversation_linear_issue::manual_link_from_reference(
        &conversation_id,
        &project_id,
        reference,
        Utc::now(),
    );
    let link = state
        .agent_conversation_linear_issue_repo
        .upsert(link)
        .await
        .map_err(|error| error.to_string())?;
    let link = if input.refresh.unwrap_or(true) {
        crate::application::agent_conversation_linear_issue::refresh_linear_issue_link(
            &state.agent_conversation_linear_issue_repo,
            state.linear_integration_service.as_ref(),
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
pub async fn refresh_agent_conversation_linear_issue(
    input: RefreshAgentConversationLinearIssueInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationLinearIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let link = state
        .agent_conversation_linear_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "No Linear issue is assigned to this conversation".to_string())?;
    let link = crate::application::agent_conversation_linear_issue::refresh_linear_issue_link(
        &state.agent_conversation_linear_issue_repo,
        state.linear_integration_service.as_ref(),
        link,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(link_response(Some(link)))
}

#[tauri::command]
pub async fn clear_agent_conversation_linear_issue(
    input: ClearAgentConversationLinearIssueInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationLinearIssueResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    state
        .agent_conversation_linear_issue_repo
        .clear(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(link_response(None))
}

#[tauri::command]
pub async fn save_linear_webhook_signing_secret(
    input: SaveLinearWebhookSigningSecretInput,
    state: State<'_, AppState>,
) -> Result<LinearWebhookConfigResponse, String> {
    let signing_secret = input.signing_secret.trim();
    if signing_secret.is_empty() {
        return Err("Linear webhook signing secret cannot be empty".to_string());
    }

    MacosKeychainSecretStore::new()
        .put_secret(LINEAR_WEBHOOK_SIGNING_SECRET_REF, signing_secret)
        .await
        .map_err(|error| error.to_string())?;

    let enabled = input.enabled.unwrap_or(true);
    let store = SqliteLinearWebhookStore::new(state.db.clone());
    store
        .set_signing_secret_ref(Some(LINEAR_WEBHOOK_SIGNING_SECRET_REF.to_string()), enabled)
        .await
        .map_err(|error| error.to_string())?;

    Ok(LinearWebhookConfigResponse {
        enabled,
        has_signing_secret: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    use crate::domain::entities::{ChatConversation, ProjectId};
    use crate::domain::integrations::IntegrationValidationStatus;

    fn test_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .manage(AppState::new_test())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app should build")
    }

    #[test]
    fn integration_settings_response_reports_secret_presence_without_secret_value() {
        let settings = LinearIntegrationSettings {
            enabled: true,
            token_secret_ref: Some("linear-secret-ref".to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            issue_search_available: true,
            last_error: Some("previous error".to_string()),
            ..Default::default()
        };

        let response = LinearIntegrationSettingsResponse::from(settings);

        assert!(response.enabled);
        assert!(response.has_api_token);
        assert_eq!(response.validation_status, "valid");
        assert!(response.issue_search_available);
        assert_eq!(response.last_error.as_deref(), Some("previous error"));
    }

    #[test]
    fn integration_settings_response_handles_unconfigured_settings() {
        let response =
            LinearIntegrationSettingsResponse::from(LinearIntegrationSettings::default());

        assert!(!response.enabled);
        assert!(!response.has_api_token);
        assert_eq!(response.validation_status, "not_configured");
        assert!(!response.issue_search_available);
        assert!(response.last_error.is_none());
    }

    #[tokio::test]
    async fn get_linear_integration_settings_returns_default_state() {
        let app = test_app();

        let settings = get_linear_integration_settings(app.state::<AppState>())
            .await
            .expect("default settings should load");
        assert!(!settings.enabled);
        assert!(!settings.has_api_token);
        assert_eq!(settings.validation_status, "not_configured");
    }

    #[tokio::test]
    async fn search_linear_issues_short_circuits_blank_query() {
        let app = test_app();

        let response = search_linear_issues(
            SearchLinearIssuesInput {
                query: "   \n\t ".to_string(),
                limit: Some(50),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("blank searches should not call Linear");

        assert!(response.issues.is_empty());
    }

    #[tokio::test]
    async fn validate_linear_integration_reports_missing_token() {
        let app = test_app();

        let error = validate_linear_integration(app.state::<AppState>())
            .await
            .expect_err("validation without a token should fail");

        assert!(error.contains("Linear API token is required"));
    }

    #[tokio::test]
    async fn save_linear_webhook_signing_secret_rejects_blank_secret_before_keychain_write() {
        let app = test_app();

        let error = save_linear_webhook_signing_secret(
            SaveLinearWebhookSigningSecretInput {
                signing_secret: "  ".to_string(),
                enabled: Some(true),
            },
            app.state::<AppState>(),
        )
        .await
        .expect_err("blank signing secrets should be rejected");

        assert_eq!(error, "Linear webhook signing secret cannot be empty");
    }

    #[tokio::test]
    async fn linear_assignment_commands_validate_empty_or_missing_assignments() {
        let app = test_app();
        let conversation_id = ChatConversationId::new();

        let empty_issue_error = assign_agent_conversation_linear_issue(
            AssignAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
                project_id: Some("project-1".to_string()),
                issue_id: "   ".to_string(),
                issue_key: None,
                title: None,
                issue_url: None,
                refresh: Some(false),
            },
            app.state::<AppState>(),
        )
        .await
        .unwrap_err();
        assert_eq!(empty_issue_error, "Linear issue id is required");

        let missing_assignment_error = refresh_agent_conversation_linear_issue(
            RefreshAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
            },
            app.state::<AppState>(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            missing_assignment_error,
            "No Linear issue is assigned to this conversation"
        );
    }

    #[tokio::test]
    async fn linear_assignment_commands_round_trip_and_refresh_cached_issue() {
        let app_state = AppState::new_test();
        app_state
            .linear_integration_service
            .save_settings(Some("lin-api-token".to_string()))
            .await
            .expect("save Linear settings");
        app_state
            .linear_integration_service
            .validate_and_enable()
            .await
            .expect("enable Linear");

        let project_id = ProjectId::from_string("project-1".to_string());
        let conversation = ChatConversation::new_project(project_id.clone());
        let conversation_id = conversation.id.clone();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("create conversation");
        let app = tauri::test::mock_builder()
            .manage(app_state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let assigned = assign_agent_conversation_linear_issue(
            AssignAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
                project_id: None,
                issue_id: " issue-1 ".to_string(),
                issue_key: Some(" LIN-123 ".to_string()),
                title: Some(" Fix Linear tab ".to_string()),
                issue_url: Some(" https://linear.app/acme/issue/LIN-123/fix ".to_string()),
                refresh: Some(false),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("assign Linear issue")
        .issue
        .expect("assigned response");

        assert_eq!(assigned.project_id, project_id.as_str());
        assert_eq!(assigned.issue_id, "issue-1");
        assert_eq!(assigned.issue_key.as_deref(), Some("LIN-123"));
        assert_eq!(assigned.title.as_deref(), Some("Fix Linear tab"));
        assert_eq!(assigned.refresh_status, "not_loaded");
        assert!(assigned.manually_assigned);

        let loaded = get_agent_conversation_linear_issue(
            GetAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("get Linear issue")
        .issue
        .expect("loaded response");
        assert_eq!(loaded.issue_id, "issue-1");

        let refreshed = refresh_agent_conversation_linear_issue(
            RefreshAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("refresh Linear issue")
        .issue
        .expect("refreshed response");
        assert_eq!(refreshed.refresh_status, "loaded");
        assert_eq!(refreshed.title.as_deref(), Some("Fix Linear tab"));
        assert!(refreshed.last_refreshed_at.is_some());

        let cleared = clear_agent_conversation_linear_issue(
            ClearAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("clear Linear issue");
        assert!(cleared.issue.is_none());
    }

    #[tokio::test]
    async fn linear_refresh_records_error_when_integration_is_disabled_but_clear_still_works() {
        let app_state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        let conversation = ChatConversation::new_project(project_id);
        let conversation_id = conversation.id.clone();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("create conversation");
        let app = tauri::test::mock_builder()
            .manage(app_state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        assign_agent_conversation_linear_issue(
            AssignAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
                project_id: Some("project-1".to_string()),
                issue_id: "issue-1".to_string(),
                issue_key: Some("LIN-123".to_string()),
                title: Some("Fix Linear tab".to_string()),
                issue_url: None,
                refresh: Some(false),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("assign Linear issue without API");

        let refreshed = refresh_agent_conversation_linear_issue(
            RefreshAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("refresh records error")
        .issue
        .expect("refreshed response");

        assert_eq!(refreshed.refresh_status, "error");
        assert_eq!(
            refreshed.refresh_error.as_deref(),
            Some("Linear integration is not enabled")
        );

        let cleared = clear_agent_conversation_linear_issue(
            ClearAgentConversationLinearIssueInput {
                conversation_id: conversation_id.as_str().to_string(),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("clear still works without Linear API");
        assert!(cleared.issue.is_none());
    }

    #[tokio::test]
    async fn disconnect_linear_integration_resets_saved_connection() {
        let app_state = AppState::new_test();
        app_state
            .linear_integration_service
            .save_settings(Some("lin-api-token".to_string()))
            .await
            .expect("save Linear settings");
        app_state
            .linear_integration_service
            .validate_and_enable()
            .await
            .expect("enable Linear");
        let app = tauri::test::mock_builder()
            .manage(app_state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let before = get_linear_integration_settings(app.state::<AppState>())
            .await
            .expect("settings load");
        assert!(before.enabled);
        assert!(before.has_api_token);

        let cleared = disconnect_linear_integration(app.state::<AppState>())
            .await
            .expect("disconnect Linear");

        assert!(!cleared.enabled);
        assert!(!cleared.has_api_token);
        assert_eq!(cleared.validation_status, "not_configured");
        assert!(!cleared.issue_search_available);
    }
}
