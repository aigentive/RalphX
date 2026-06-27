use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{
    granola_integration_service::is_valid_granola_note_id, AppState, GranolaNoteDetail,
    GranolaNoteSummary,
};
use crate::domain::entities::{
    AgentConversationGranolaNoteLink, ChatContextType, ChatConversationId, ProjectId,
};
use crate::domain::integrations::GranolaIntegrationSettings;

/// Connection/settings view for the Granola integration.
///
/// The raw Granola API token is never returned to the frontend — only
/// `has_api_token` signals presence. The keychain reference is likewise withheld.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaIntegrationSettingsResponse {
    pub enabled: bool,
    pub has_api_token: bool,
    pub validation_status: String,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<GranolaIntegrationSettings> for GranolaIntegrationSettingsResponse {
    fn from(settings: GranolaIntegrationSettings) -> Self {
        Self {
            enabled: settings.enabled,
            has_api_token: settings.token_secret_ref.is_some(),
            validation_status: settings.validation_status.as_str().to_string(),
            last_validated_at: settings.last_validated_at,
            last_error: settings.last_error,
            updated_at: settings.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveGranolaIntegrationSettingsInput {
    /// Tri-state: `None` leaves the stored token untouched, `Some("")` clears
    /// it, `Some(value)` replaces it. The raw token goes straight to the
    /// keychain via the service; it is never persisted in the DB row.
    pub api_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListGranolaNotesInput {
    pub page_size: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetGranolaNoteDetailInput {
    pub note_id: String,
    #[serde(default)]
    pub include_transcript: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaNoteSummaryResponse {
    pub id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl From<GranolaNoteSummary> for GranolaNoteSummaryResponse {
    fn from(note: GranolaNoteSummary) -> Self {
        Self {
            id: note.id,
            title: note.title,
            url: note.url,
            summary: note.summary,
            created_at: note.created_at,
            updated_at: note.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaTranscriptEntryResponse {
    pub speaker: Option<String>,
    pub text: String,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaNoteDetailResponse {
    pub id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub transcript: Vec<GranolaTranscriptEntryResponse>,
}

impl From<GranolaNoteDetail> for GranolaNoteDetailResponse {
    fn from(note: GranolaNoteDetail) -> Self {
        Self {
            id: note.id,
            title: note.title,
            url: note.url,
            summary: note.summary,
            transcript: note
                .transcript
                .unwrap_or_default()
                .into_iter()
                .map(|entry| GranolaTranscriptEntryResponse {
                    speaker: entry.speaker,
                    text: entry.text,
                    start_ms: entry.start_ms,
                    end_ms: entry.end_ms,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListGranolaNotesResponse {
    pub notes: Vec<GranolaNoteSummaryResponse>,
    pub has_more: bool,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentConversationGranolaNoteInput {
    pub conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignAgentConversationGranolaNoteInput {
    pub conversation_id: String,
    pub project_id: Option<String>,
    pub note_id: String,
    pub title: Option<String>,
    pub note_url: Option<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub include_transcript: Option<bool>,
    #[serde(default)]
    pub refresh: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAgentConversationGranolaNoteInput {
    pub conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearAgentConversationGranolaNoteInput {
    pub conversation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationGranolaNoteResponse {
    pub note: Option<AgentConversationGranolaNoteLinkResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationGranolaNoteLinkResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub provider: String,
    pub note_id: String,
    pub note_url: Option<String>,
    pub title: Option<String>,
    pub summary_markdown: Option<String>,
    pub transcript: Vec<serde_json::Value>,
    pub include_transcript: bool,
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub refresh_status: String,
    pub refresh_error: Option<String>,
    pub assigned_at: DateTime<Utc>,
    pub assigned_from_message_id: Option<String>,
    pub manually_assigned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AgentConversationGranolaNoteLink> for AgentConversationGranolaNoteLinkResponse {
    fn from(link: AgentConversationGranolaNoteLink) -> Self {
        Self {
            conversation_id: link.conversation_id.as_str().to_string(),
            project_id: link.project_id.as_str().to_string(),
            provider: link.provider,
            note_id: link.note_id,
            note_url: link.note_url,
            title: link.title,
            summary_markdown: link.summary_markdown,
            transcript: serde_json::from_str(&link.transcript_json).unwrap_or_default(),
            include_transcript: link.include_transcript,
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
    Err("Unable to resolve project for Granola assignment".to_string())
}

fn note_response(
    link: Option<AgentConversationGranolaNoteLink>,
) -> AgentConversationGranolaNoteResponse {
    AgentConversationGranolaNoteResponse {
        note: link.map(AgentConversationGranolaNoteLinkResponse::from),
    }
}

#[tauri::command]
pub async fn get_granola_integration_settings(
    state: State<'_, AppState>,
) -> Result<GranolaIntegrationSettingsResponse, String> {
    state
        .granola_integration_service
        .get_settings()
        .await
        .map(GranolaIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn save_granola_integration_settings(
    input: SaveGranolaIntegrationSettingsInput,
    state: State<'_, AppState>,
) -> Result<GranolaIntegrationSettingsResponse, String> {
    state
        .granola_integration_service
        .save_settings(input.api_token)
        .await
        .map(GranolaIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn validate_granola_integration_settings(
    state: State<'_, AppState>,
) -> Result<GranolaIntegrationSettingsResponse, String> {
    state
        .granola_integration_service
        .validate_and_enable()
        .await
        .map(GranolaIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn list_granola_notes(
    input: ListGranolaNotesInput,
    state: State<'_, AppState>,
) -> Result<ListGranolaNotesResponse, String> {
    let page = state
        .granola_integration_service
        .list_notes(input.page_size.unwrap_or(20), input.cursor.as_deref())
        .await?;
    Ok(ListGranolaNotesResponse {
        notes: page
            .notes
            .into_iter()
            .map(GranolaNoteSummaryResponse::from)
            .collect(),
        has_more: page.has_more,
        cursor: page.cursor,
    })
}

#[tauri::command]
pub async fn get_granola_note_detail(
    input: GetGranolaNoteDetailInput,
    state: State<'_, AppState>,
) -> Result<GranolaNoteDetailResponse, String> {
    let note_id = input.note_id.trim();
    let note = state
        .granola_integration_service
        .fetch_note_detail_for_user(note_id, input.include_transcript.unwrap_or(true))
        .await?;
    Ok(GranolaNoteDetailResponse::from(note))
}

#[tauri::command]
pub async fn get_agent_conversation_granola_note(
    input: GetAgentConversationGranolaNoteInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationGranolaNoteResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let link = state
        .agent_conversation_granola_note_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(note_response(link))
}

#[tauri::command]
pub async fn assign_agent_conversation_granola_note(
    input: AssignAgentConversationGranolaNoteInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationGranolaNoteResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let note_id = input.note_id.trim();
    if !is_valid_granola_note_id(note_id) {
        return Err("Granola note id is invalid".to_string());
    }
    let project_id =
        resolve_assignment_project_id(state.inner(), &conversation_id, input.project_id).await?;
    let reference =
        crate::application::agent_conversation_granola_note::ComposerGranolaReferenceMetadata {
            note_id: note_id.to_string(),
            title: non_empty(input.title),
            url: non_empty(input.note_url),
            summary: non_empty(input.summary),
            include_transcript: input.include_transcript.unwrap_or(true),
        };
    let link = crate::application::agent_conversation_granola_note::manual_link_from_reference(
        &conversation_id,
        &project_id,
        reference,
        Utc::now(),
    );
    let link = state
        .agent_conversation_granola_note_repo
        .upsert(link)
        .await
        .map_err(|error| error.to_string())?;
    let link = if input.refresh.unwrap_or(true) {
        crate::application::agent_conversation_granola_note::refresh_granola_note_link(
            &state.agent_conversation_granola_note_repo,
            &state.granola_integration_service,
            link,
        )
        .await
        .map_err(|error| error.to_string())?
    } else {
        link
    };
    Ok(note_response(Some(link)))
}

#[tauri::command]
pub async fn refresh_agent_conversation_granola_note(
    input: RefreshAgentConversationGranolaNoteInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationGranolaNoteResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let link = state
        .agent_conversation_granola_note_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "No Granola note is assigned to this conversation".to_string())?;
    let link = crate::application::agent_conversation_granola_note::refresh_granola_note_link(
        &state.agent_conversation_granola_note_repo,
        &state.granola_integration_service,
        link,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(note_response(Some(link)))
}

#[tauri::command]
pub async fn clear_agent_conversation_granola_note(
    input: ClearAgentConversationGranolaNoteInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationGranolaNoteResponse, String> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    state
        .agent_conversation_granola_note_repo
        .clear(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(note_response(None))
}
