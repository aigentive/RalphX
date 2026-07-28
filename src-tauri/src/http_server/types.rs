// Request/Response types for HTTP server endpoints

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    AgentConversationResponse, AgentConversationWorkspaceResponse, SendAgentMessageResponse,
};
use crate::commands::ExecutionState;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    AgentTaskState, Artifact, ArtifactContent, AuditLogEntry, MemoryEntry, StepProgressSummary,
    TaskProposal, TaskStep,
};
use crate::http_server::delegation::DelegationService;
use crate::http_server::handlers::artifacts::EditError;

// ============================================================================
// HTTP Server State
// ============================================================================

/// Combined state for HTTP server handlers
/// Includes both AppState and ExecutionState for task transitions
#[derive(Clone)]
pub struct HttpServerState {
    pub app_state: Arc<AppState>,
    pub execution_state: Arc<ExecutionState>,
    pub delegation_service: Arc<DelegationService>,
}

#[cfg(test)]
impl HttpServerState {
    pub(crate) fn new_test(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            execution_state: Arc::new(ExecutionState::new()),
            delegation_service: Default::default(),
        }
    }
}

// ============================================================================
// Request/Response Types - Ideation (Sessions)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct UpdateSessionTitleRequest {
    pub session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub title: String,
}

// ============================================================================
// Request/Response Types - Child Session Status + Messaging
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ChildSessionStatusParams {
    pub include_messages: Option<bool>,
    pub message_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeationSessionSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub session_purpose: Option<String>,
    pub parent_session_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_effective_model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStateInfo {
    pub is_running: bool,
    pub started_at: Option<String>,
    pub last_active_at: Option<String>,
    pub pid: Option<u32>,
    pub estimated_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationInfo {
    pub status: String,
    pub generation: i32,
    pub current_round: Option<u32>,
    pub gap_score: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessageSummary {
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChildSessionStatusResponse {
    pub session: IdeationSessionSummary,
    pub agent_state: AgentStateInfo,
    pub verification: Option<VerificationInfo>,
    pub recent_messages: Option<Vec<ChatMessageSummary>>,
    pub pending_initial_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelegatedSessionSummary {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub parent_context_type: String,
    pub parent_context_id: String,
    pub agent_name: String,
    pub harness: String,
    pub provider_session_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelegatedRunSummary {
    pub agent_run_id: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<String>,
    pub effective_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub processed_tokens: Option<u64>,
    pub estimated_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelegatedSessionStatusResponse {
    pub session: DelegatedSessionSummary,
    pub agent_state: AgentStateInfo,
    pub conversation_id: Option<String>,
    pub latest_run: Option<DelegatedRunSummary>,
    pub recent_messages: Option<Vec<ChatMessageSummary>>,
}

// ============================================================================
// Request/Response Types - Agent Conversation Follow-Ups
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateFollowupAgentConversationRequest {
    /// Optional explicit origin Agent conversation. When omitted, source_task_id
    /// resolves through the task's attached ideation session/workspace.
    pub origin_conversation_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub initial_prompt: Option<String>,
    pub spawn_reason: Option<String>,
    pub blocker_fingerprint: Option<String>,
    pub provider_harness: Option<String>,
    pub model_override: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
}

#[derive(Debug, Serialize)]
pub struct CreateFollowupAgentConversationResponse {
    pub reused_existing: bool,
    pub origin_conversation_id: String,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub spawn_reason: Option<String>,
    pub blocker_fingerprint: Option<String>,
    pub conversation: AgentConversationResponse,
    pub workspace: Option<AgentConversationWorkspaceResponse>,
    pub send_result: Option<SendAgentMessageResponse>,
}

// ============================================================================
// Request/Response Types - Agent Conversation Issues
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterAgentConversationIssueRequest {
    pub origin_conversation_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub issue_kind: String,
    pub severity: Option<String>,
    pub blocking_scope: Option<String>,
    pub title: String,
    pub summary: String,
    pub evidence: Option<String>,
    pub recommendation: Option<String>,
    pub blocker_fingerprint: Option<String>,
    pub attach_to_issue_id: Option<String>,
    #[serde(default)]
    pub confirm_new: bool,
    pub new_issue_reason: Option<String>,
    pub issue_check_token: Option<String>,
    pub followup_title: Option<String>,
    pub followup_prompt: Option<String>,
    #[serde(default)]
    pub auto_followup_eligible: bool,
    pub provider_harness: Option<String>,
    pub model_override: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
}

#[derive(Debug, Deserialize)]
pub struct ListAgentConversationIssuesRequest {
    pub conversation_id: String,
    #[serde(default)]
    pub include_resolved: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAgentConversationIssueStatusRequest {
    pub issue_id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ConvertAgentConversationIssueFollowupRequest {
    pub issue_id: String,
    pub title: Option<String>,
    pub initial_prompt: Option<String>,
    pub provider_harness: Option<String>,
    pub model_override: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
}

#[derive(Debug, Serialize)]
pub struct AgentConversationIssueOccurrenceResponse {
    pub id: String,
    pub issue_id: String,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub issue_kind: String,
    pub severity: String,
    pub blocking_scope: String,
    pub title: String,
    pub summary: String,
    pub evidence: Option<String>,
    pub recommendation: Option<String>,
    pub raw_blocker_fingerprint: Option<String>,
    pub canonical_fingerprint: Option<String>,
    pub dedupe_decision: Option<String>,
    pub created_at: String,
}

impl From<crate::domain::entities::AgentConversationIssueOccurrence>
    for AgentConversationIssueOccurrenceResponse
{
    fn from(occurrence: crate::domain::entities::AgentConversationIssueOccurrence) -> Self {
        Self {
            id: occurrence.id,
            issue_id: occurrence.issue_id,
            source_task_id: occurrence.source_task_id,
            source_context_type: occurrence.source_context_type,
            source_context_id: occurrence.source_context_id,
            source_agent_name: occurrence.source_agent_name,
            issue_kind: occurrence.issue_kind,
            severity: occurrence.severity,
            blocking_scope: occurrence.blocking_scope,
            title: occurrence.title,
            summary: occurrence.summary,
            evidence: occurrence.evidence,
            recommendation: occurrence.recommendation,
            raw_blocker_fingerprint: occurrence.raw_blocker_fingerprint,
            canonical_fingerprint: occurrence.canonical_fingerprint,
            dedupe_decision: occurrence.dedupe_decision,
            created_at: occurrence.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AgentConversationIssueResponse {
    pub id: String,
    pub project_id: String,
    pub conversation_id: String,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub issue_kind: String,
    pub severity: String,
    pub status: String,
    pub blocking_scope: String,
    pub title: String,
    pub summary: String,
    pub evidence: Option<String>,
    pub recommendation: Option<String>,
    pub blocker_fingerprint: Option<String>,
    pub canonical_fingerprint: Option<String>,
    pub canonical_scope_kind: Option<String>,
    pub canonical_scope_subject: Option<String>,
    pub canonical_family: Option<String>,
    pub superseded_by_issue_id: Option<String>,
    pub occurrence_count: Option<usize>,
    pub occurrences: Vec<AgentConversationIssueOccurrenceResponse>,
    pub followup_title: Option<String>,
    pub followup_prompt: Option<String>,
    pub auto_followup_eligible: bool,
    pub linked_followup_conversation_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

impl From<crate::domain::entities::AgentConversationIssue> for AgentConversationIssueResponse {
    fn from(issue: crate::domain::entities::AgentConversationIssue) -> Self {
        Self {
            id: issue.id,
            project_id: issue.project_id.as_str().to_string(),
            conversation_id: issue.conversation_id.as_str(),
            source_task_id: issue.source_task_id,
            source_context_type: issue.source_context_type,
            source_context_id: issue.source_context_id,
            source_agent_name: issue.source_agent_name,
            issue_kind: issue.issue_kind,
            severity: issue.severity,
            status: issue.status,
            blocking_scope: issue.blocking_scope,
            title: issue.title,
            summary: issue.summary,
            evidence: issue.evidence,
            recommendation: issue.recommendation,
            blocker_fingerprint: issue.blocker_fingerprint,
            canonical_fingerprint: issue.canonical_fingerprint,
            canonical_scope_kind: issue.canonical_scope_kind,
            canonical_scope_subject: issue.canonical_scope_subject,
            canonical_family: issue.canonical_family,
            superseded_by_issue_id: issue.superseded_by_issue_id,
            occurrence_count: None,
            occurrences: Vec::new(),
            followup_title: issue.followup_title,
            followup_prompt: issue.followup_prompt,
            auto_followup_eligible: issue.auto_followup_eligible,
            linked_followup_conversation_id: issue
                .linked_followup_conversation_id
                .map(|id| id.as_str()),
            created_at: issue.created_at.to_rfc3339(),
            updated_at: issue.updated_at.to_rfc3339(),
            resolved_at: issue.resolved_at.map(|value| value.to_rfc3339()),
        }
    }
}

impl AgentConversationIssueResponse {
    pub fn with_occurrences(
        mut self,
        occurrences: Vec<crate::domain::entities::AgentConversationIssueOccurrence>,
    ) -> Self {
        self.occurrence_count = Some(occurrences.len());
        self.occurrences = occurrences.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Debug, Serialize)]
pub struct RegisterAgentConversationIssueResponse {
    pub issue: AgentConversationIssueResponse,
    pub auto_followup_created: bool,
    pub followup: Option<CreateFollowupAgentConversationResponse>,
    pub dedupe_result: String,
    pub canonical_fingerprint: Option<String>,
    pub occurrence_id: Option<String>,
    pub occurrence_count: Option<usize>,
    pub candidate_issues: Vec<AgentConversationIssueResponse>,
    pub issue_check_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListAgentConversationIssuesResponse {
    pub issues: Vec<AgentConversationIssueResponse>,
    pub issue_check_token: String,
}

#[derive(Debug, Serialize)]
pub struct UpdateAgentConversationIssueStatusResponse {
    pub issue: AgentConversationIssueResponse,
}

#[derive(Debug, Serialize)]
pub struct ConvertAgentConversationIssueFollowupResponse {
    pub issue: AgentConversationIssueResponse,
    pub followup: CreateFollowupAgentConversationResponse,
}

// ============================================================================
// Request/Response Types - Native Agent Tasks
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct AgentTaskContextFields {
    pub context_type: Option<String>,
    pub context_id: Option<String>,
    pub project_id: Option<String>,
    pub actor_agent: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAgentTaskRequest {
    #[serde(flatten)]
    pub context: AgentTaskContextFields,
    pub title: String,
    pub details: String,
    pub active_label: Option<String>,
    pub owner_agent: Option<String>,
    pub metadata: Option<Value>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetAgentTaskRequest {
    #[serde(flatten)]
    pub context: AgentTaskContextFields,
    #[serde(alias = "task_id")]
    pub task_ref: String,
}

#[derive(Debug, Deserialize)]
pub struct ListAgentTasksRequest {
    #[serde(flatten)]
    pub context: AgentTaskContextFields,
    #[serde(default)]
    pub include_done: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListAgentTaskListsRequest {
    #[serde(flatten)]
    pub context: AgentTaskContextFields,
}

#[derive(Debug, Deserialize)]
pub struct ListAgentTasksForListRequest {
    #[serde(flatten)]
    pub context: AgentTaskContextFields,
    pub list_id: String,
    #[serde(default)]
    pub include_done: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAgentTaskRequest {
    #[serde(flatten)]
    pub context: AgentTaskContextFields,
    #[serde(alias = "task_id")]
    pub task_ref: String,
    pub title: Option<String>,
    pub details: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_patch")]
    pub active_label: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_string_patch")]
    pub owner_agent: Option<Option<String>>,
    pub state: Option<AgentTaskState>,
    pub metadata: Option<Value>,
    #[serde(default)]
    pub add_blocked_by: Vec<String>,
    #[serde(default)]
    pub add_blocks: Vec<String>,
    #[serde(default)]
    pub remove_blocked_by: Vec<String>,
    #[serde(default)]
    pub remove_blocks: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClaimAgentTaskRequest {
    #[serde(flatten)]
    pub context: AgentTaskContextFields,
    #[serde(alias = "task_id")]
    pub task_ref: String,
    pub owner_agent: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteAgentTaskRequest {
    #[serde(flatten)]
    pub context: AgentTaskContextFields,
    #[serde(alias = "task_id")]
    pub task_ref: String,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteDelegateAssignmentRequest {
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseDelegateAssignmentRequest {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelegateAssignmentDto {
    pub task_number: i64,
    pub title: String,
    pub details: String,
    pub task_state: String,
    pub assignment_state: String,
    pub delegate_agent_name: String,
    pub caller_scope_type: String,
}

#[derive(Debug, Serialize)]
pub struct DelegateAssignmentResponse {
    pub success: bool,
    pub assignment: Option<DelegateAssignmentDto>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentTaskStateChangeDto {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentTaskDto {
    pub task_id: String,
    pub task_number: i64,
    pub title: String,
    pub details: String,
    pub active_label: Option<String>,
    pub owner_agent: Option<String>,
    pub state: String,
    pub metadata: Option<Value>,
    pub blocked_by: Vec<String>,
    pub unresolved_blocked_by: Vec<String>,
    pub blocks: Vec<String>,
    pub availability: String,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentTaskSummaryDto {
    pub task_id: String,
    pub task_number: i64,
    pub title: String,
    pub state: String,
    pub owner_agent: Option<String>,
    pub blocked_by: Vec<String>,
    pub blocks: Vec<String>,
    pub availability: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentTaskListSummaryDto {
    pub list_id: String,
    pub list_sequence: i64,
    pub task_count: i64,
    pub open_count: i64,
    pub active_count: i64,
    pub done_count: i64,
    pub dropped_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct AgentTaskMutationResponse {
    pub success: bool,
    pub task: Option<AgentTaskDto>,
    pub changed_fields: Vec<String>,
    pub state_change: Option<AgentTaskStateChangeDto>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentTaskGetResponse {
    pub success: bool,
    pub task: Option<AgentTaskDto>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentTaskListResponse {
    pub success: bool,
    pub tasks: Vec<AgentTaskSummaryDto>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentTaskListsResponse {
    pub success: bool,
    pub lists: Vec<AgentTaskListSummaryDto>,
    pub error: Option<String>,
}

fn deserialize_optional_string_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
pub struct SendSessionMessageRequest {
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendSessionMessageResponse {
    pub delivery_status: String,
    pub conversation_id: Option<String>,
}

// ============================================================================
// Request/Response Types - Ideation (Proposals)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateProposalRequest {
    pub session_id: String,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub priority: Option<String>,
    pub steps: Option<Vec<String>>,
    pub acceptance_criteria: Option<Vec<String>>,
    pub affected_paths: Option<Vec<String>>,
    /// Optional list of proposal IDs this proposal depends on
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Optional target project ID for cross-project proposal execution
    pub target_project: Option<String>,
    /// Expected total number of proposals for this session (set-once gating)
    pub expected_proposal_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProposalRequest {
    pub proposal_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub steps: Option<Vec<String>>,
    pub acceptance_criteria: Option<Vec<String>>,
    pub affected_paths: Option<Vec<String>>,
    pub user_priority: Option<String>,
    /// Additive: proposal IDs this proposal should depend on
    #[serde(default)]
    pub add_depends_on: Vec<String>,
    /// Additive: proposal IDs this proposal should block (reverse direction)
    #[serde(default)]
    pub add_blocks: Vec<String>,
    /// Optional target project ID for cross-project proposal execution
    pub target_project: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FinalizeProposalsRequest {
    pub session_id: String,
}

#[derive(Debug, Serialize)]
pub struct FinalizeProposalsResponse {
    pub created_task_ids: Vec<String>,
    /// Number of proposal-to-proposal dependency edges created (excludes merge task edges).
    pub dependencies_created: u32,
    /// Number of plan tasks created (excludes the auto-generated merge task).
    pub tasks_created: u32,
    /// Human-readable summary of the finalization result.
    pub message: Option<String>,
    pub session_status: String,
    pub execution_plan_id: Option<String>,
    pub warnings: Vec<String>,
    pub project_id: String,
    /// Number of proposals skipped because their target_project points to a different project.
    #[serde(default)]
    pub skipped_foreign_count: u32,
    /// Whether any tasks were created in Ready status — used to guard scheduler trigger.
    #[serde(default)]
    pub any_ready_tasks: bool,
    /// Finalization result status: "success" or "pending_acceptance"
    pub status: String,
    /// Session title for webhook payload enrichment.
    #[serde(default)]
    pub session_title: Option<String>,
    /// Project name for webhook payload enrichment.
    #[serde(default)]
    pub project_name: Option<String>,
}

/// Request to accept a pending finalize confirmation
#[derive(Debug, Deserialize)]
pub struct AcceptFinalizeRequest {
    pub session_id: String,
}

/// Request to reject a pending finalize confirmation
#[derive(Debug, Deserialize)]
pub struct RejectFinalizeRequest {
    pub session_id: String,
}

/// Response from accept/reject finalize
#[derive(Debug, Serialize)]
pub struct AcceptanceActionResponse {
    /// "accepted" or "rejected"
    pub status: String,
    pub session_id: String,
}

/// Response for get_acceptance_status
#[derive(Debug, Serialize)]
pub struct AcceptanceStatusResponse {
    pub session_id: String,
    /// "pending", "accepted", "rejected", or null
    pub acceptance_status: Option<String>,
}

/// Response for get_pending_confirmations
#[derive(Debug, Serialize)]
pub struct PendingConfirmationsResponse {
    pub sessions: Vec<PendingConfirmationItem>,
}

/// One item in the pending confirmations list
#[derive(Debug, Serialize)]
pub struct PendingConfirmationItem {
    pub session_id: String,
    pub session_title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteProposalRequest {
    pub proposal_id: String,
}

#[derive(Debug, Deserialize)]
pub struct AddDependencyRequest {
    pub proposal_id: String,
    pub depends_on_id: String,
}

#[derive(Debug, Serialize)]
pub struct ProposalResponse {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub priority: String,
    pub steps: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub created_at: String,
    /// Partial failure contract: non-fatal dependency errors encountered during create/update
    pub dependency_errors: Vec<String>,
    /// Optional target project ID for cross-project proposal execution
    pub target_project: Option<String>,
    /// Whether the auto-accept pipeline was triggered for this session (always false — kept for backward compat)
    pub auto_accept_triggered: bool,
    /// Whether the session is ready to finalize (expected proposal count reached)
    pub ready_to_finalize: bool,
}

impl From<TaskProposal> for ProposalResponse {
    fn from(proposal: TaskProposal) -> Self {
        Self {
            id: proposal.id.to_string(),
            session_id: proposal.session_id.to_string(),
            title: proposal.title,
            description: proposal.description,
            category: proposal.category.to_string(),
            priority: proposal.suggested_priority.to_string(),
            steps: proposal.steps,
            acceptance_criteria: proposal.acceptance_criteria,
            created_at: proposal.created_at.to_rfc3339(),
            dependency_errors: Vec::new(),
            target_project: proposal.target_project.clone(),
            auto_accept_triggered: false,
            ready_to_finalize: false,
        }
    }
}

/// Lightweight proposal summary for list endpoint
#[derive(Debug, Serialize)]
pub struct ProposalSummary {
    pub id: String,
    pub title: String,
    pub category: String,
    pub priority: String,
    pub depends_on: Vec<String>,
    pub plan_artifact_id: Option<String>,
    /// Optional target project ID for cross-project proposal execution
    pub target_project: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListProposalsResponse {
    pub proposals: Vec<ProposalSummary>,
    pub count: usize,
}

/// Full proposal details for get endpoint
#[derive(Debug, Serialize)]
pub struct ProposalDetailResponse {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub priority: String,
    pub steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub depends_on: Vec<String>,
    pub plan_artifact_id: Option<String>,
    pub created_at: String,
    /// Optional target project ID for cross-project proposal execution
    pub target_project: Option<String>,
}

// ============================================================================
// Request/Response Types - Dependency Analysis
// ============================================================================

/// Node in dependency analysis response
#[derive(Debug, Serialize)]
pub struct DependencyNodeResponse {
    pub id: String,
    pub title: String,
    pub in_degree: usize,
    pub out_degree: usize,
    pub is_root: bool,
    pub is_blocker: bool,
}

/// Edge in dependency analysis response
#[derive(Debug, Serialize)]
pub struct DependencyEdgeResponse {
    pub from: String,
    pub to: String,
    pub reason: Option<String>,
}

/// Summary statistics for dependency analysis
#[derive(Debug, Serialize)]
pub struct DependencyAnalysisSummary {
    pub total_proposals: usize,
    pub root_count: usize,
    pub leaf_count: usize,
    pub max_depth: usize,
}

/// Response for analyze_session_dependencies endpoint
#[derive(Debug, Serialize)]
pub struct AnalyzeDependenciesResponse {
    pub nodes: Vec<DependencyNodeResponse>,
    pub edges: Vec<DependencyEdgeResponse>,
    pub critical_path: Vec<String>,
    pub critical_path_length: usize,
    pub has_cycles: bool,
    pub cycles: Option<Vec<Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub summary: DependencyAnalysisSummary,
}

// ============================================================================
// Request/Response Types - Tasks
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct UpdateTaskRequest {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct AddTaskNoteRequest {
    pub task_id: String,
    pub note: String,
}

#[derive(Debug, Deserialize)]
pub struct GetTaskDetailsRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub category: String,
    pub created_at: String,
    pub updated_at: String,
}

// ============================================================================
// Request/Response Types - Projects
// ============================================================================

/// Request body for POST /api/external/projects
/// Registers a directory as a RalphX project (creates dir + git if needed).
#[derive(Debug, Deserialize)]
pub struct RegisterProjectExternalRequest {
    pub working_directory: String,
    pub name: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub worktree_parent_directory: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListTasksRequest {
    pub project_id: String,
    pub status: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListTasksResponse {
    pub tasks: Vec<TaskResponse>,
}

#[derive(Debug, Deserialize)]
pub struct SuggestTaskRequest {
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub priority: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SuggestTaskResponse {
    pub task: TaskResponse,
}

// ============================================================================
// Request/Response Types - Reviews
// ============================================================================

#[derive(Debug, Deserialize, Clone)]
pub struct ReviewIssueRequest {
    pub severity: String, // "critical" | "major" | "minor" | "suggestion"
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub no_step_reason: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default, alias = "file")]
    pub file_path: Option<String>,
    #[serde(default, alias = "line")]
    pub line_number: Option<u32>,
    #[serde(default)]
    pub code_snippet: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ReviewIssue {
    pub severity: String, // "critical" | "major" | "minor" | "suggestion"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct CompleteReviewRequest {
    pub task_id: String,
    pub decision: String, // "approved" | "needs_changes" | "escalate"
    pub summary: Option<String>,
    pub feedback: Option<String>,
    pub issues: Option<Vec<ReviewIssueRequest>>,
    pub escalation_reason: Option<String>,
    pub scope_drift_classification: Option<String>,
    pub scope_drift_notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReviewNoteResponse {
    pub id: String,
    pub reviewer: String,
    pub outcome: String,
    pub summary: Option<String>,
    pub notes: Option<String>,
    pub issues: Option<Vec<ReviewIssue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followup_session_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ReviewNotesResponse {
    pub task_id: String,
    pub revision_count: u32,
    pub max_revisions: u32,
    pub reviews: Vec<ReviewNoteResponse>,
}

#[derive(Debug, Serialize)]
pub struct CompleteReviewResponse {
    pub success: bool,
    pub message: String,
    pub new_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followup_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followup_conversation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApproveTaskRequest {
    pub task_id: String,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RequestTaskChangesRequest {
    pub task_id: String,
    pub feedback: String,
}

// ============================================================================
// Request/Response Types - Permissions
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PermissionRequestInput {
    #[serde(default)]
    pub request_id: Option<String>,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    pub context: Option<String>,
    // Agent identity fields (optional for backward compat)
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub context_type: Option<String>,
    #[serde(default)]
    pub context_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PermissionRequestResponse {
    pub request_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ResolvePermissionInput {
    pub request_id: String,
    pub decision: String, // "allow" or "deny"
    pub message: Option<String>,
}

// ============================================================================
// Request/Response Types - Plan Artifacts
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreatePlanArtifactRequest {
    pub session_id: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub blueprint_title: Option<String>,
    #[serde(default)]
    pub blueprint_content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePlanArtifactRequest {
    pub artifact_id: String,
    pub content: String,
    #[serde(default)]
    pub caller_session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EditPlanArtifactRequest {
    pub artifact_id: String,
    pub edits: Vec<PlanEdit>,
    #[serde(default)]
    pub caller_session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApprovePlanArtifactRequest {
    pub session_id: String,
    #[serde(default)]
    pub artifact_id: Option<String>,
    /// The Blueprint identity displayed alongside the Overview being approved.
    /// Required for v2 bundles so a stale UI cannot approve a revised Blueprint.
    #[serde(default)]
    pub blueprint_artifact_id: Option<String>,
    #[serde(default)]
    pub blueprint_artifact_version: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct SubmitPlanComplexityAssessmentRequest {
    pub session_id: String,
    pub artifact_id: String,
    pub artifact_version: u32,
    #[serde(default)]
    pub blueprint_artifact_id: Option<String>,
    #[serde(default)]
    pub blueprint_artifact_version: Option<u32>,
    pub level: String,
    pub score: u8,
    pub recommended_action: String,
    pub confidence: f64,
    pub reason_summary: String,
    #[serde(default)]
    pub signals: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct SubmitPlanComplexityAssessmentResponse {
    pub success: bool,
    pub assessment: PlanComplexityAssessmentResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanComplexityAssessmentResponse {
    pub id: String,
    pub session_id: String,
    pub artifact_id: String,
    pub artifact_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blueprint_artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blueprint_artifact_version: Option<u32>,
    pub level: String,
    pub score: u8,
    pub recommended_action: String,
    pub confidence: f64,
    pub reason_summary: String,
    pub signals: Value,
    pub assessed_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct PlanEdit {
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Deserialize)]
pub struct LinkProposalsToPlanRequest {
    pub proposal_ids: Vec<String>,
    pub artifact_id: String,
}

/// Payload for the plan:proposals_may_need_update event
/// Emitted when a plan artifact is updated and has linked proposals
#[derive(Debug, Clone, Serialize)]
pub struct PlanProposalsSyncPayload {
    /// The new artifact ID (new version)
    pub artifact_id: String,
    /// The previous artifact ID (the one that was updated)
    pub previous_artifact_id: String,
    /// IDs of proposals linked to the original plan
    pub proposal_ids: Vec<String>,
    /// The new version number
    pub new_version: u32,
    /// The ideation session this plan belongs to (for scoped notifications)
    pub session_id: Option<String>,
    /// Whether proposals were already re-linked to the new artifact ID server-side.
    /// When true, the UI only needs to refresh — no client-side re-linking is needed.
    pub proposals_relinked: bool,
}

#[derive(Debug, Serialize)]
pub struct ArtifactResponse {
    pub id: String,
    pub artifact_type: String,
    pub name: String,
    pub content_type: String,
    pub content: String,
    pub version: u32,
    pub created_at: String,
    pub created_by: String,
    pub bucket_id: Option<String>,
    pub task_id: Option<String>,
    pub process_id: Option<String>,
    pub derived_from: Vec<String>,
    /// Companion detailed implementation blueprint for plan bundle responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blueprint_artifact: Option<Box<ArtifactResponse>>,
    /// Plan contract version (1 = legacy overview-only, 2 = paired bundle).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_contract_version: Option<i32>,
    /// Backend-derived verification/approval target for the exact current bundle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_target_id: Option<String>,
    /// Role of the returned artifact within a plan bundle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_role: Option<String>,
    /// The artifact ID that was replaced (only set on update responses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_artifact_id: Option<String>,
    /// The ideation session this artifact belongs to (only set on update responses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Whether this plan was inherited from a parent session (only set on get_session_plan responses).
    /// When true, the plan is read-only — use create_plan_artifact to create a session-specific plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_inherited: Option<bool>,
    /// The working directory of the project this session belongs to (only set on get_session_plan responses).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_working_directory: Option<String>,
    /// Plan-mode approval state for the current artifact version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_approval_status: Option<String>,
    /// Approved artifact id when the current artifact version is approved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_approved_artifact_id: Option<String>,
    /// Approved artifact version when the current artifact version is approved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_approved_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_approved_blueprint_artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_approved_blueprint_version: Option<u32>,
    /// Approval timestamp for the current artifact version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_approved_at: Option<String>,
}

impl From<Artifact> for ArtifactResponse {
    fn from(artifact: Artifact) -> Self {
        let (content_type, content) = match &artifact.content {
            ArtifactContent::Inline { text } => ("inline".to_string(), text.clone()),
            ArtifactContent::File { path } => ("file".to_string(), path.clone()),
        };

        Self {
            id: artifact.id.to_string(),
            artifact_type: artifact.artifact_type.to_string(),
            name: artifact.name,
            content_type,
            content,
            version: artifact.metadata.version,
            created_at: artifact.metadata.created_at.to_rfc3339(),
            created_by: artifact.metadata.created_by.clone(),
            bucket_id: artifact.bucket_id.map(|id| id.as_str().to_string()),
            task_id: artifact.metadata.task_id.map(|id| id.as_str().to_string()),
            process_id: artifact
                .metadata
                .process_id
                .map(|id| id.as_str().to_string()),
            derived_from: artifact
                .derived_from
                .iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            blueprint_artifact: None,
            plan_contract_version: None,
            plan_target_id: None,
            artifact_role: None,
            previous_artifact_id: None,
            session_id: None,
            is_inherited: None,
            project_working_directory: None,
            plan_approval_status: None,
            plan_approved_artifact_id: None,
            plan_approved_version: None,
            plan_approved_blueprint_artifact_id: None,
            plan_approved_blueprint_version: None,
            plan_approved_at: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchArtifactsRequest {
    pub project_id: String,
    pub query: String,
    pub artifact_types: Option<Vec<String>>,
}

/// Summary of an artifact version for history display
#[derive(Debug, Serialize)]
pub struct ArtifactVersionSummaryResponse {
    pub id: String,
    pub version: u32,
    pub name: String,
    pub created_at: String,
    pub created_by: String,
    pub metadata: Option<serde_json::Value>,
}

impl From<crate::domain::repositories::ArtifactVersionSummary> for ArtifactVersionSummaryResponse {
    fn from(summary: crate::domain::repositories::ArtifactVersionSummary) -> Self {
        Self {
            id: summary.id.to_string(),
            version: summary.version,
            name: summary.name,
            created_at: summary.created_at.to_rfc3339(),
            created_by: summary.created_by,
            metadata: summary.metadata,
        }
    }
}

// ============================================================================
// Request/Response Types - Task Steps
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct StartStepRequest {
    pub step_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CompleteStepRequest {
    pub step_id: String,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SkipStepRequest {
    pub step_id: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct FailStepRequest {
    pub step_id: String,
    pub error: String,
}

#[derive(Debug, Deserialize)]
pub struct AddStepRequest {
    pub task_id: String,
    pub title: String,
    pub description: Option<String>,
    pub after_step_id: Option<String>,
    pub parent_step_id: Option<String>, // NEW: create as sub-step
    pub scope_context: Option<String>,  // NEW: STRICT SCOPE JSON
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResponse {
    pub id: String,
    pub task_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub sort_order: i32,
    pub completion_note: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub parent_step_id: Option<String>,
    pub scope_context: Option<String>,
}

impl From<TaskStep> for StepResponse {
    fn from(step: TaskStep) -> Self {
        Self {
            id: step.id.as_str().to_string(),
            task_id: step.task_id.as_str().to_string(),
            title: step.title,
            description: step.description,
            status: step.status.to_db_string().to_string(),
            sort_order: step.sort_order,
            completion_note: step.completion_note,
            started_at: step.started_at.map(|dt| dt.to_rfc3339()),
            completed_at: step.completed_at.map(|dt| dt.to_rfc3339()),
            parent_step_id: step.parent_step_id.map(|id| id.as_str().to_string()),
            scope_context: step.scope_context,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TaskSummaryForStep {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub internal_status: String,
}

#[derive(Debug, Serialize)]
pub struct StepContextResponse {
    pub step: StepResponse,
    pub parent_step: Option<StepResponse>,
    pub task_summary: TaskSummaryForStep,
    pub scope_context: Option<String>,
    pub sibling_steps: Vec<StepResponse>,
    pub step_progress: StepProgressSummary,
    pub context_hints: Vec<String>,
}

// ============================================================================
// Request/Response Types - Review Issues
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct MarkIssueInProgressRequest {
    pub issue_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MarkIssueAddressedRequest {
    pub issue_id: String,
    pub resolution_notes: String,
    pub attempt_number: i32,
}

// ============================================================================
// Request/Response Types - Questions (AskUserQuestion)
// ============================================================================

/// Option in a question request
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct QuestionOptionInput {
    pub value: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct QuestionRequestInput {
    #[serde(default)]
    pub request_id: Option<String>,
    pub session_id: String,
    pub question: String,
    pub header: Option<String>,
    #[serde(default)]
    pub options: Vec<QuestionOptionInput>,
    #[serde(default)]
    pub multi_select: bool,
    #[serde(default = "default_question_allow_skip")]
    pub allow_skip: bool,
    pub batch_index: Option<u32>,
    pub batch_total: Option<u32>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct QuestionRequestResponse {
    pub request_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ResolveQuestionInput {
    pub request_id: String,
    pub selected_options: Vec<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub skipped: bool,
}

fn default_question_allow_skip() -> bool {
    true
}

// ============================================================================
// Request/Response Types - Memory (read + write tools)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct MemoryEntryResponse {
    pub id: String,
    pub project_id: String,
    pub bucket: String,
    pub title: String,
    pub summary: String,
    pub details_markdown: String,
    pub scope_paths: Vec<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_conversation_id: Option<String>,
    pub source_rule_file: Option<String>,
    pub quality_score: Option<f64>,
    pub status: String,
    pub content_hash: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<MemoryEntry> for MemoryEntryResponse {
    fn from(entry: MemoryEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            project_id: entry.project_id.to_string(),
            bucket: entry.bucket.to_string(),
            title: entry.title,
            summary: entry.summary,
            details_markdown: entry.details_markdown,
            scope_paths: entry.scope_paths,
            source_context_type: entry.source_context_type,
            source_context_id: entry.source_context_id,
            source_conversation_id: entry.source_conversation_id,
            source_rule_file: entry.source_rule_file,
            quality_score: entry.quality_score,
            status: entry.status.to_string(),
            content_hash: entry.content_hash,
            created_at: entry.created_at.to_rfc3339(),
            updated_at: entry.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchMemoriesRequest {
    pub project_id: String,
    pub query: Option<String>,
    pub bucket: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SearchMemoriesResponse {
    pub memories: Vec<MemoryEntryResponse>,
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct GetMemoryRequest {
    pub memory_id: String,
}

#[derive(Debug, Serialize)]
pub struct GetMemoryResponse {
    pub memory: Option<MemoryEntryResponse>,
}

#[derive(Debug, Deserialize)]
pub struct GetMemoriesForPathsRequest {
    pub project_id: String,
    pub paths: Vec<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct GetMemoriesForPathsResponse {
    pub memories: Vec<MemoryEntryResponse>,
    pub count: usize,
}

/// Single memory entry to upsert
#[derive(Debug, Deserialize)]
pub struct MemoryEntryInput {
    pub bucket: String, // architecture_patterns | implementation_discoveries | operational_playbooks
    pub title: String,
    pub summary: String,
    pub details_markdown: String,
    pub scope_paths: Vec<String>, // glob patterns for path scoping
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_conversation_id: Option<String>,
    pub quality_score: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertMemoriesRequest {
    pub project_id: String,
    pub memories: Vec<MemoryEntryInput>,
}

#[derive(Debug, Serialize)]
pub struct UpsertMemoriesResponse {
    pub inserted: usize,
    pub skipped: usize,
    pub failed: usize,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct MarkMemoryObsoleteRequest {
    pub memory_id: String,
}

#[derive(Debug, Serialize)]
pub struct MarkMemoryObsoleteResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct RefreshMemoryRuleIndexRequest {
    pub project_id: String,
    pub scope_key: Option<String>, // if None, refresh all
}

#[derive(Debug, Serialize)]
pub struct RefreshMemoryRuleIndexResponse {
    pub files_refreshed: usize,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct IngestRuleFileRequest {
    pub project_id: String,
    pub rule_file_path: String, // relative to project root (e.g., ".claude/rules/task-state-machine.md")
}

#[derive(Debug, Serialize)]
pub struct IngestRuleFileResponse {
    pub memories_created: usize,
    pub memories_updated: usize,
    pub file_rewritten: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct RebuildArchiveSnapshotsRequest {
    pub project_id: String,
}

#[derive(Debug, Serialize)]
pub struct RebuildArchiveSnapshotsResponse {
    pub job_id: String,
    pub message: String,
}

// ============================================================================
// Request/Response Types - Session Linking
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateChildSessionRequest {
    pub parent_session_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default = "default_inherit_context")]
    pub inherit_context: bool,
    pub initial_prompt: Option<String>,
    /// Purpose of the child session: "general" (default) or "verification"
    pub purpose: Option<String>,
    /// When true, the child session origin is set to External (triggered via external MCP).
    /// When false or absent (default), origin is set to Internal.
    /// Ignored for verification children — they always inherit parent origin.
    #[serde(default)]
    pub is_external_trigger: bool,
    /// Task that triggered this follow-up session, when spawned from execution/review/merge.
    pub source_task_id: Option<String>,
    /// Originating non-ideation context type (task_execution, review, merge, research, etc.).
    pub source_context_type: Option<String>,
    /// Originating non-ideation context ID.
    pub source_context_id: Option<String>,
    /// Why this follow-up was spawned (out_of_scope_failure, review_followup, etc.).
    pub spawn_reason: Option<String>,
    /// Stable dedupe key for a blocker targeted by this follow-up session.
    pub blocker_fingerprint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DelegateStartRequest {
    pub caller_agent_name: Option<String>,
    pub caller_agent_profile: Option<String>,
    pub caller_context_type: Option<String>,
    pub caller_context_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub parent_turn_id: Option<String>,
    pub parent_message_id: Option<String>,
    pub parent_conversation_id: Option<String>,
    pub parent_tool_use_id: Option<String>,
    pub delegated_session_id: Option<String>,
    pub child_session_id: Option<String>,
    pub task_ref: Option<String>,
    pub agent_name: String,
    #[serde(alias = "message")]
    pub prompt: String,
    pub title: Option<String>,
    #[serde(default = "default_inherit_context")]
    pub inherit_context: bool,
    pub harness: Option<AgentHarnessKind>,
    pub model: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DelegateWaitRequest {
    pub job_id: String,
    pub include_delegated_status: Option<bool>,
    pub include_child_status: Option<bool>,
    pub include_messages: Option<bool>,
    pub message_limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct DelegateCancelRequest {
    pub job_id: String,
}

fn default_inherit_context() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct CreateChildSessionResponse {
    pub session_id: String,
    pub parent_session_id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_context: Option<ParentContextResponse>,
    /// Whether an orchestrator job was enqueued (true when description is provided)
    pub orchestration_triggered: bool,
    /// Verification generation number; only set when purpose == "verification" and initialization succeeded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<i32>,
    /// Prompt persisted for deferred launch when orchestration was queued behind capacity limits.
    /// Present when `orchestration_triggered` is false and a prompt/description was provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_initial_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ParentSessionSummary {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct ParentProposalSummary {
    pub id: String,
    pub title: String,
    pub category: String,
    pub priority: String,
    pub status: String,
    pub acceptance_criteria: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ParentContextResponse {
    pub parent_session: ParentSessionSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_content: Option<String>,
    pub proposals: Vec<ParentProposalSummary>,
}

// ============================================================================
// Request/Response Types - Conversation Transcript
// ============================================================================

/// Single message in a transcript
#[derive(Debug, Serialize, Clone)]
pub struct TranscriptMessage {
    pub role: String, // "user", "assistant", etc.
    pub content: String,
    pub created_at: String, // RFC3339 timestamp
}

#[derive(Debug, Deserialize)]
pub struct GetConversationTranscriptRequest {
    pub conversation_id: String,
}

#[derive(Debug, Serialize)]
pub struct GetConversationTranscriptResponse {
    pub conversation_id: String,
    pub messages: Vec<TranscriptMessage>,
    pub message_count: usize,
}

// ============================================================================
// Request/Response Types - Session Messages (Ideation Agent Context Recovery)
// ============================================================================

/// Default limit for session messages retrieval
fn default_session_messages_limit() -> usize {
    50
}

#[derive(Debug, Deserialize)]
pub struct GetSessionMessagesRequest {
    pub session_id: String,
    #[serde(default = "default_session_messages_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub include_tool_calls: bool,
}

/// Single message in a session messages response
#[derive(Debug, Serialize, Clone)]
pub struct SessionMessageResponse {
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct GetSessionMessagesResponse {
    pub messages: Vec<SessionMessageResponse>,
    pub count: usize,
    pub truncated: bool,
    pub total_available: usize,
}

/// POST /api/team/artifact — create a team artifact
#[derive(Debug, Deserialize)]
pub struct CreateTeamArtifactRequest {
    pub session_id: String,
    pub title: String,
    pub content: String,
    pub artifact_type: String,
    pub related_artifact_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateTeamArtifactResponse {
    pub artifact_id: String,
}
/// GET /api/team/artifacts/:session_id response
#[derive(Debug, Serialize)]
pub struct TeamArtifactSummary {
    pub id: String,
    pub name: String,
    pub artifact_type: String,
    pub version: u32,
    pub content_preview: String,
    pub created_at: String,
    pub author_teammate: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetTeamArtifactsResponse {
    pub artifacts: Vec<TeamArtifactSummary>,
    pub count: usize,
}

// ============================================================================
// Request/Response Types - Active Streaming State
// ============================================================================

/// Response for GET /api/conversations/:id/active-state
///
/// Returns the current streaming state for a conversation, used by frontend
/// to hydrate streaming UI when navigating to an active agent execution.
#[derive(Debug, Serialize)]
pub struct ActiveStateResponse {
    /// Whether an agent is currently running for this conversation
    pub is_active: bool,
    /// Owning run for the transient projection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Tool calls currently in progress or recently completed
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ActiveToolCall>,
    /// Streaming tasks (subagents) currently running or completed
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub streaming_tasks: Vec<ActiveStreamingTask>,
    /// Partial text content accumulated from agent:chunk events
    #[serde(skip_serializing_if = "String::is_empty")]
    pub partial_text: String,
}

/// A tool call in the active state response.
///
/// Mirrors CachedToolCall from streaming_state_cache.rs for HTTP serialization.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveToolCall {
    /// Unique tool call ID (e.g., "toolu_01A...")
    pub id: String,
    /// Tool name (e.g., "bash", "read", "edit")
    pub name: String,
    /// Current arguments (may be partial during streaming)
    pub arguments: serde_json::Value,
    /// Result if completed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Diff context for Edit/Write tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_context: Option<serde_json::Value>,
    /// Parent tool use ID for nested tool calls
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

impl From<crate::application::chat_service::CachedToolCall> for ActiveToolCall {
    fn from(cached: crate::application::chat_service::CachedToolCall) -> Self {
        Self {
            id: cached.id,
            name: cached.name,
            arguments: cached.arguments,
            result: cached.result,
            diff_context: cached.diff_context,
            parent_tool_use_id: cached.parent_tool_use_id,
        }
    }
}

/// A streaming task in the active state response.
///
/// Mirrors CachedStreamingTask from streaming_state_cache.rs for HTTP serialization.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveStreamingTask {
    /// Tool use ID that started this task
    pub tool_use_id: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Subagent type (e.g., "ralphx:coder")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// Model being used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Current status: "running" or "completed"
    pub status: String,
    /// Agent ID if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_agent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_harness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    /// Total tokens used by this task (from TaskCompleted stats)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    /// Total tool uses count (from TaskCompleted stats)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tool_uses: Option<u64>,
    /// Duration in milliseconds (from TaskCompleted stats)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_provenance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

impl From<crate::application::chat_service::CachedStreamingTask> for ActiveStreamingTask {
    fn from(cached: crate::application::chat_service::CachedStreamingTask) -> Self {
        Self {
            tool_use_id: cached.tool_use_id,
            description: cached.description,
            subagent_type: cached.subagent_type,
            model: cached.model,
            status: cached.status,
            agent_id: cached.agent_id,
            delegated_job_id: cached.delegated_job_id,
            delegated_session_id: cached.delegated_session_id,
            delegated_conversation_id: cached.delegated_conversation_id,
            delegated_agent_run_id: cached.delegated_agent_run_id,
            provider_harness: cached.provider_harness,
            provider_session_id: cached.provider_session_id,
            upstream_provider: cached.upstream_provider,
            provider_profile: cached.provider_profile,
            logical_model: cached.logical_model,
            effective_model_id: cached.effective_model_id,
            logical_effort: cached.logical_effort,
            effective_effort: cached.effective_effort,
            approval_policy: cached.approval_policy,
            sandbox_mode: cached.sandbox_mode,
            total_tokens: cached.total_tokens,
            total_tool_uses: cached.total_tool_uses,
            duration_ms: cached.duration_ms,
            input_tokens: cached.input_tokens,
            output_tokens: cached.output_tokens,
            cache_creation_tokens: cached.cache_creation_tokens,
            cache_read_tokens: cached.cache_read_tokens,
            estimated_usd: cached.estimated_usd,
            text_output: cached.text_output,
            started_at: cached.started_at,
            completed_at: cached.completed_at,
            timestamp_provenance: cached.timestamp_provenance,
            seq: cached.seq,
        }
    }
}

// ============================================================================
// Request/Response Types - Execution Complete
// ============================================================================

/// Optional test result reported by the worker agent at execution completion.
/// Used to populate the validation cache in tasks.metadata.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResultInput {
    #[serde(alias = "tests_ran")]
    pub tests_ran: bool,
    #[serde(alias = "tests_passed")]
    pub tests_passed: bool,
    #[serde(alias = "test_summary")]
    pub test_summary: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionCompleteRequest {
    pub summary: Option<String>,
    /// Optional test results for validation cache. When absent, no cache entry is created.
    #[serde(alias = "test_result")]
    pub test_result: Option<TestResultInput>,
}

#[derive(Debug, Serialize)]
pub struct ExecutionCompleteResponse {
    pub success: bool,
    pub message: String,
}

// ============================================================================
// Common Response Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
}

// ============================================================================
// HTTP Error Type
// ============================================================================

/// HTTP handler error that preserves validation messages in the response body.
///
/// `From<StatusCode>` allows existing `?` operators on `Result<T, StatusCode>`
/// to compile unchanged when handler return types use `HttpError` as the error type.
#[derive(Debug)]
pub struct HttpError {
    pub status: StatusCode,
    pub message: Option<String>,
}

impl HttpError {
    /// 422 Unprocessable Entity with an actionable message body.
    pub fn validation(message: String) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: Some(message),
        }
    }
}

impl From<StatusCode> for HttpError {
    fn from(status: StatusCode) -> Self {
        Self {
            status,
            message: None,
        }
    }
}

impl From<EditError> for HttpError {
    fn from(e: EditError) -> Self {
        match e {
            EditError::AnchorNotFound {
                edit_index,
                old_text_preview,
            } => HttpError::validation(format!(
                "Edit #{} failed: old_text not found in plan content. Preview: '{}'",
                edit_index, old_text_preview
            )),
            EditError::AmbiguousAnchor {
                edit_index,
                old_text_preview,
            } => HttpError::validation(format!(
                "Edit #{} failed: old_text matches multiple locations. Use a longer/more unique anchor. Preview: '{}'",
                edit_index, old_text_preview
            )),
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> axum::response::Response {
        match self.message {
            Some(msg) => {
                // If message is a pre-serialized JSON object, use it directly as the body.
                // This allows rich error responses (e.g. queue_full with queued_count + hint)
                // without changing the HttpError struct layout.
                if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str(&msg) {
                    return (self.status, Json(serde_json::Value::Object(obj))).into_response();
                }
                (self.status, Json(serde_json::json!({"error": msg}))).into_response()
            }
            None => self.status.into_response(),
        }
    }
}

// ============================================================================
// Request/Response Types - API Key Management
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub permissions: Option<i32>,
    pub project_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub id: String,
    pub name: String,
    pub key: String,
    pub key_prefix: String,
    pub permissions: i32,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub permissions: i32,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub last_used_at: Option<String>,
    pub project_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ListApiKeysResponse {
    pub keys: Vec<ApiKeyInfo>,
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyProjectsRequest {
    pub project_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RotateApiKeyResponse {
    pub id: String,
    pub new_key: String,
    pub key_prefix: String,
    pub old_key_grace_expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct AuditLogResponse {
    pub entries: Vec<AuditLogEntry>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePermissionsRequest {
    pub permissions: i64,
}

// ============================================================================
// Request/Response Types - Plan Verification
// ============================================================================

/// A gap identified by the critic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationGapRequest {
    pub severity: String,
    pub category: String,
    pub description: String,
    #[serde(default)]
    pub why_it_matters: Option<String>,
    /// Which critic layer identified this gap: "layer1" | "layer2"
    #[serde(default)]
    pub source: Option<String>,
}

/// Request to update verification state (from MCP orchestrator)
#[derive(Debug, Deserialize)]
pub struct UpdateVerificationRequest {
    pub status: String, // "reviewing" | "needs_revision" | "verified" | "skipped"
    #[serde(default)]
    pub in_progress: bool,
    #[serde(default)]
    pub round: Option<u32>,
    #[serde(default)]
    pub gaps: Option<Vec<VerificationGapRequest>>,
    #[serde(default)]
    pub convergence_reason: Option<String>,
    #[serde(default)]
    pub max_rounds: Option<u32>,
    /// True if the critic output could not be parsed this round (parse failure tracking)
    #[serde(default)]
    pub parse_failed: Option<bool>,
    /// Generation counter for zombie protection — must match session's current generation
    /// when setting in_progress=true
    #[serde(default)]
    pub generation: Option<i32>,
}

/// Request to terminate verification as an infrastructure/runtime failure.
///
/// Unlike `UpdateVerificationRequest`, this path does not record a content verdict.
/// It resets the parent session to `unverified`, clears authoritative current gaps,
/// preserves round/debug metadata where available, and ends the active verification run.
#[derive(Debug, Deserialize)]
pub struct VerificationInfraFailureRequest {
    /// Generation counter for zombie protection — must match the session's current generation.
    #[serde(default)]
    pub generation: Option<i32>,
    /// Why verification failed to complete cleanly. Defaults to `agent_error`.
    #[serde(default)]
    pub convergence_reason: Option<String>,
    /// Optional current round for debugging continuity.
    #[serde(default)]
    pub round: Option<u32>,
    /// Optional max-rounds value for debugging continuity.
    #[serde(default)]
    pub max_rounds: Option<u32>,
}

/// A single verification gap in the API response (mirrors domain VerificationGap)
#[derive(Debug, Serialize)]
pub struct VerificationGapResponse {
    pub severity: String,
    pub category: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why_it_matters: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Per-round summary in the API response (round number derived from array index + 1)
#[derive(Debug, Serialize)]
pub struct VerificationRoundSummary {
    /// 1-based round number (derived from array index)
    pub round: u32,
    pub gap_score: u32,
    /// Deduplicated unique gap count (fingerprints.len() for historical rounds)
    pub gap_count: u32,
}

/// Per-round detail in the API response with full gap snapshots when available.
#[derive(Debug, Serialize)]
pub struct VerificationRoundDetailResponse {
    /// 1-based round number (derived from array index)
    pub round: u32,
    pub gap_score: u32,
    pub gap_count: u32,
    #[serde(default)]
    pub gaps: Vec<VerificationGapResponse>,
}

/// Continuity context for the most recent verification child session.
/// Populated only when the parent session has at least one verification child.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationChildInfo {
    /// Non-null only when in_progress=true and the child session is not archived
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_child_session_id: Option<String>,
    /// Always present when this block exists — the most recent child session ID
    pub latest_child_session_id: String,
    /// True when the latest child session is archived
    pub latest_child_archived: bool,
    /// updated_at timestamp of the latest child session (RFC3339)
    pub latest_child_updated_at: String,
    /// Inferred agent state: "likely_generating" | "likely_waiting" | "idle"
    pub agent_state: String,
    /// Deferred launch prompt waiting for capacity, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_initial_prompt: Option<String>,
    /// Last assistant message content truncated to 500 chars, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
    /// Timestamp of the last assistant message (RFC3339), if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_message_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationRunHistoryEntryResponse {
    pub generation: i32,
    pub status: String,
    pub in_progress: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_round: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rounds: Option<u32>,
    pub round_count: u32,
    pub gap_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_score: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct VerificationQueryParams {
    pub generation: Option<i32>,
}

/// Response for GET/POST verification status
#[derive(Debug, Serialize)]
pub struct VerificationResponse {
    pub session_id: String,
    pub status: String,
    pub in_progress: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_round: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rounds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_score: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_round_index: Option<u32>,
    /// Full gap objects for the latest round (empty if no native run snapshot exists)
    #[serde(default)]
    pub current_gaps: Vec<VerificationGapResponse>,
    /// Round history summaries — last 10 rounds in chronological order (empty if no native run snapshot exists)
    #[serde(default)]
    pub rounds: Vec<VerificationRoundSummary>,
    /// Full round history details — last 10 rounds in chronological order (empty if no native run snapshot exists)
    #[serde(default)]
    pub round_details: Vec<VerificationRoundDetailResponse>,
    /// Plan artifact version when verification ran — null if session has no linked plan
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_version: Option<u32>,
    /// Current verification generation counter
    pub verification_generation: i32,
    /// The generation represented by current_gaps / rounds / round_details in this response.
    pub selected_generation: i32,
    /// Cross-generation native verification lineage (newest first).
    #[serde(default)]
    pub run_history: Vec<VerificationRunHistoryEntryResponse>,
    /// Continuity context for the most recent verification child session, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_child: Option<VerificationChildInfo>,
}

/// Request to atomically revert plan + skip verification
#[derive(Debug, Deserialize)]
pub struct RevertAndSkipRequest {
    /// The plan artifact version (artifact_id) to restore content from
    pub plan_version_to_restore: String,
}

// ============================================================================
// Request/Response Types - Verification Confirmation (Wave 2)
// ============================================================================

/// POST /api/verification/confirm — queue a model-native Verify Plan action.
#[derive(Debug, Deserialize)]
pub struct ConfirmVerificationRequest {
    pub session_id: String,
}

/// POST /api/verification/dismiss — remove a pending verification entry.
#[derive(Debug, Deserialize)]
pub struct DismissVerificationRequest {
    pub session_id: String,
}

/// POST /api/verification/auto-accept — toggle per-session auto-accept.
#[derive(Debug, Deserialize)]
pub struct AutoAcceptVerificationRequest {
    pub session_id: String,
    pub enabled: bool,
}

/// Generic OK response for verification mutation endpoints.
#[derive(Debug, Serialize)]
pub struct VerificationActionResponse {
    pub status: String,
}

/// One specialist entry in the GET /api/verification/specialists response.
#[derive(Debug, Clone, Serialize)]
pub struct SpecialistEntryResponse {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub dispatch_mode: String,
    pub enabled_by_default: bool,
}

/// Response for GET /api/verification/specialists.
#[derive(Debug, Serialize)]
pub struct SpecialistsResponse {
    pub specialists: Vec<SpecialistEntryResponse>,
}

/// Response for GET /api/verification/confirmation-status/{session_id}.
/// status: "pending" | "accepted" | "rejected" | "not_applicable"
#[derive(Debug, Serialize)]
pub struct ConfirmationStatusResponse {
    pub session_id: String,
    /// DB-backed status for this session's verification confirmation gate.
    /// "pending"        — dialog should be shown to the user
    /// "accepted"       — user confirmed; verification was triggered
    /// "rejected"       — user dismissed; session stays Unverified
    /// "not_applicable" — NULL in DB (external session, auto-verify, or no plan yet)
    pub status: String,
    /// Only present when status == "pending"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_artifact_id: Option<String>,
    /// Only present when status == "pending"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_specialists: Option<Vec<SpecialistEntryResponse>>,
}

/// Response for GET /api/verification/pending-confirmations?project_id=X
#[derive(Debug, Serialize)]
pub struct PendingVerificationConfirmationsResponse {
    pub sessions: Vec<PendingVerificationConfirmationItem>,
}

/// One item in the pending verification confirmations list
#[derive(Debug, Clone, Serialize)]
pub struct PendingVerificationConfirmationItem {
    pub session_id: String,
    pub session_title: Option<String>,
    pub plan_artifact_id: Option<String>,
    pub available_specialists: Vec<SpecialistEntryResponse>,
}

#[cfg(test)]
mod http_error_tests {
    use super::*;

    #[test]
    fn test_validation_error_has_422_status_and_message() {
        let err =
            HttpError::validation("Cannot modify accepted session. Reopen it first.".to_string());
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            err.message.as_deref(),
            Some("Cannot modify accepted session. Reopen it first.")
        );
    }

    #[test]
    fn test_validation_error_message_is_not_sensitive() {
        // Verify the message is a user-actionable string, not a raw DB error
        let err = HttpError::validation(
            "Validation error: Cannot modify archived session. Reopen it first.".to_string(),
        );
        let msg = err.message.unwrap();
        assert!(
            msg.contains("Reopen it first"),
            "Message should guide the user"
        );
        assert!(!msg.contains("SQLITE"), "Should not leak DB internals");
        assert!(
            !msg.contains("rusqlite"),
            "Should not leak internal library names"
        );
    }

    #[test]
    fn test_from_status_code_has_no_message() {
        let err = HttpError::from(StatusCode::NOT_FOUND);
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert!(
            err.message.is_none(),
            "StatusCode errors should have no body message"
        );
    }

    #[test]
    fn test_from_internal_server_error_has_no_message() {
        let err = HttpError::from(StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            err.message.is_none(),
            "Internal errors should not expose messages"
        );
    }

    #[tokio::test]
    async fn test_validation_error_into_response_status() {
        use axum::response::IntoResponse;
        let err = HttpError::validation("Cannot modify archived session.".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_status_only_error_into_response() {
        use axum::response::IntoResponse;
        let err = HttpError::from(StatusCode::NOT_FOUND);
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

// ============================================================================
// Request/Response Types - Managed Team (/api/managed_team/*)
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsureManagedTeamRequest {
    pub conversation_id: String,
    pub project_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedTeamSessionSummary {
    pub id: String,
    pub project_id: String,
    pub coordinator_conversation_id: String,
    pub status: String,
    pub configured_concurrency: u32,
    pub effective_concurrency: u32,
    pub automatic_wake_limit: u32,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedTeamMemberSummary {
    pub id: String,
    pub team_id: String,
    pub name: String,
    pub normalized_name: String,
    pub canonical_agent_name: String,
    pub role_summary: String,
    pub status: String,
    pub generation: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedTeamStatusResponse {
    pub session: ManagedTeamSessionSummary,
    pub members: Vec<ManagedTeamMemberSummary>,
}
