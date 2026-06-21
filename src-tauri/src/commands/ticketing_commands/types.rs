use serde::{Deserialize, Serialize};

use crate::application::agent_conversation_start_service::StartAgentConversationInput;

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
pub struct ConversationTicketResponse {
    pub ticket_ref: TicketRefInput,
    pub project_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
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
    /// Linked conversations whose workspace currently has an open (non-terminal) PR.
    pub open_pr_count: usize,
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
    /// Project the deep-link target lives in (set for conversation associations so
    /// the agents view can select the exact conversation, not just switch views).
    pub project_id: Option<String>,
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
