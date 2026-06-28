use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{
    granola_integration_service::is_valid_granola_note_id, AppState, GranolaNoteDetail,
    GranolaNoteSummary,
};
use crate::domain::entities::{
    AgentConversationGranolaNoteLink, AgentConversationWorkspace, ChatContextType,
    ChatConversationId, ProjectId,
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
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetGranolaNoteDetailInput {
    pub note_id: String,
    #[serde(default)]
    pub include_transcript: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaNoteRxConversationResponse {
    pub conversation_id: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaNoteTicketLinkResponse {
    pub provider: String,
    pub label: String,
    pub title: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaNotePullRequestLinkResponse {
    pub number: i64,
    pub url: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GranolaNoteAssociations {
    rx_conversations: Vec<GranolaNoteRxConversationResponse>,
    ticket_links: Vec<GranolaNoteTicketLinkResponse>,
    pull_requests: Vec<GranolaNotePullRequestLinkResponse>,
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
    pub rx_conversation_count: usize,
    pub rx_conversations: Vec<GranolaNoteRxConversationResponse>,
    pub ticket_count: usize,
    pub ticket_links: Vec<GranolaNoteTicketLinkResponse>,
    pub pr_count: usize,
    pub pull_requests: Vec<GranolaNotePullRequestLinkResponse>,
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
            rx_conversation_count: 0,
            rx_conversations: Vec::new(),
            ticket_count: 0,
            ticket_links: Vec::new(),
            pr_count: 0,
            pull_requests: Vec::new(),
        }
    }
}

impl GranolaNoteSummaryResponse {
    fn with_associations(mut self, associations: GranolaNoteAssociations) -> Self {
        self.rx_conversation_count = associations.rx_conversations.len();
        self.ticket_count = associations.ticket_links.len();
        self.pr_count = associations.pull_requests.len();
        self.rx_conversations = associations.rx_conversations;
        self.ticket_links = associations.ticket_links;
        self.pull_requests = associations.pull_requests;
        self
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

fn push_unique<T: PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}

async fn granola_project_conversation_titles(
    state: &AppState,
    project_id: &ProjectId,
) -> Result<BTreeMap<String, Option<String>>, String> {
    let conversations = state
        .chat_conversation_repo
        .get_by_context_filtered(ChatContextType::Project, project_id.as_str(), true)
        .await
        .map_err(|error| error.to_string())?;
    Ok(conversations
        .into_iter()
        .map(|conversation| {
            (
                conversation.id.as_str(),
                conversation.title.filter(|title| !title.trim().is_empty()),
            )
        })
        .collect())
}

async fn granola_note_ticket_links(
    state: &AppState,
    project_id: &ProjectId,
    conversation_id: &ChatConversationId,
) -> Result<Vec<GranolaNoteTicketLinkResponse>, String> {
    let mut ticket_links = Vec::new();

    if let Some(link) = state
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .filter(|link| link.project_id == *project_id)
    {
        ticket_links.push(GranolaNoteTicketLinkResponse {
            provider: "jira".to_string(),
            label: link.issue_key,
            title: link.title,
            url: link.issue_url,
        });
    }

    if let Some(link) = state
        .agent_conversation_linear_issue_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .filter(|link| link.project_id == *project_id)
    {
        ticket_links.push(GranolaNoteTicketLinkResponse {
            provider: "linear".to_string(),
            label: link.issue_key.unwrap_or(link.issue_id),
            title: link.title,
            url: link.issue_url,
        });
    }

    Ok(ticket_links)
}

fn granola_note_pull_request(
    workspace: Option<&AgentConversationWorkspace>,
) -> Option<GranolaNotePullRequestLinkResponse> {
    let workspace = workspace?;
    Some(GranolaNotePullRequestLinkResponse {
        number: workspace.publication_pr_number?,
        url: workspace.publication_pr_url.clone(),
        status: workspace.publication_pr_status.clone(),
    })
}

fn sort_granola_note_associations(associations: &mut GranolaNoteAssociations) {
    associations.rx_conversations.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.conversation_id.cmp(&right.conversation_id))
    });
    associations.ticket_links.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.label.cmp(&right.label))
    });
    associations
        .pull_requests
        .sort_by(|left, right| left.number.cmp(&right.number));
}

async fn build_granola_note_associations(
    state: &AppState,
    project_id: &ProjectId,
    note_ids: &[String],
) -> Result<BTreeMap<String, GranolaNoteAssociations>, String> {
    let requested_note_ids: BTreeSet<&str> = note_ids.iter().map(String::as_str).collect();
    if requested_note_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let links = state
        .agent_conversation_granola_note_repo
        .list_by_project_id(project_id)
        .await
        .map_err(|error| error.to_string())?;
    let conversation_titles = granola_project_conversation_titles(state, project_id).await?;
    let workspaces_by_conversation: BTreeMap<String, AgentConversationWorkspace> = state
        .agent_conversation_workspace_repo
        .get_by_project_id(project_id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|workspace| (workspace.conversation_id.as_str(), workspace))
        .collect();

    let mut associations: BTreeMap<String, GranolaNoteAssociations> = BTreeMap::new();
    for link in links {
        if !requested_note_ids.contains(link.note_id.as_str()) {
            continue;
        }
        let conversation_id = link.conversation_id.as_str();
        let note_associations = associations.entry(link.note_id.clone()).or_default();
        push_unique(
            &mut note_associations.rx_conversations,
            GranolaNoteRxConversationResponse {
                conversation_id: conversation_id.clone(),
                title: conversation_titles.get(&conversation_id).cloned().flatten(),
            },
        );

        for ticket_link in
            granola_note_ticket_links(state, project_id, &link.conversation_id).await?
        {
            push_unique(&mut note_associations.ticket_links, ticket_link);
        }

        if let Some(pr_link) =
            granola_note_pull_request(workspaces_by_conversation.get(&conversation_id))
        {
            push_unique(&mut note_associations.pull_requests, pr_link);
        }
    }

    for associations in associations.values_mut() {
        sort_granola_note_associations(associations);
    }

    Ok(associations)
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
    let note_ids: Vec<String> = page.notes.iter().map(|note| note.id.clone()).collect();
    let associations_by_note = if let Some(project_id) = non_empty(input.project_id) {
        build_granola_note_associations(
            state.inner(),
            &ProjectId::from_string(project_id),
            &note_ids,
        )
        .await?
    } else {
        BTreeMap::new()
    };
    Ok(ListGranolaNotesResponse {
        notes: page
            .notes
            .into_iter()
            .map(|note| {
                let note_id = note.id.clone();
                GranolaNoteSummaryResponse::from(note).with_associations(
                    associations_by_note
                        .get(&note_id)
                        .cloned()
                        .unwrap_or_default(),
                )
            })
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
