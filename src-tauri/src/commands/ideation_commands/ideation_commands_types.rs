// Type definitions for Ideation commands
// Input types, response types, and conversions

use serde::{Deserialize, Serialize};

use crate::domain::entities::{
    DependencyGraph, IdeationAnalysisBaseRefKind, IdeationSession, TaskProposal,
};

// Re-export shared ChatMessageResponse
pub use crate::commands::chat_responses::ChatMessageResponse;

// ============================================================================
// Session Types
// ============================================================================

/// Team configuration input for ideation sessions
#[derive(Debug, Deserialize, Serialize)]
pub struct TeamConfigInput {
    pub max_teammates: u32,
    pub model_ceiling: String,
    pub budget_limit: Option<f64>,
    pub composition_mode: String,
}

/// Input for creating a cross-project session (imports a verified plan from another project).
///
/// The target project is identified by filesystem path; it will be auto-created if not found.
/// The source session must have a verified plan (Verified | Skipped | ImportedVerified).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCrossProjectSessionInput {
    /// Absolute filesystem path of the target project's working directory
    pub target_project_path: String,
    /// ID of the source ideation session whose plan is being inherited
    pub source_session_id: String,
    /// Optional human-readable title for the new session
    pub title: Option<String>,
}

/// Input for creating a new ideation session
#[derive(Debug, Deserialize)]
pub struct CreateSessionInput {
    pub project_id: String,
    pub title: Option<String>,
    pub seed_task_id: Option<String>,
    pub team_mode: Option<String>,
    pub team_config: Option<TeamConfigInput>,
    pub analysis_base_ref_kind: Option<IdeationAnalysisBaseRefKind>,
    pub analysis_base_ref: Option<String>,
    pub analysis_base_display_name: Option<String>,
}

/// Response wrapper for ideation session operations
#[derive(Debug, Serialize)]
pub struct IdeationSessionResponse {
    pub id: String,
    pub project_id: String,
    pub title: Option<String>,
    pub title_source: Option<String>,
    pub status: String,
    pub plan_artifact_id: Option<String>,
    pub seed_task_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub converted_at: Option<String>,
    pub team_mode: Option<String>,
    pub team_config: Option<serde_json::Value>,
    pub verification_status: String,
    pub verification_in_progress: bool,
    pub gap_score: Option<i32>,
    pub inherited_plan_artifact_id: Option<String>,
    pub source_project_id: Option<String>,
    pub source_session_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub spawn_reason: Option<String>,
    pub blocker_fingerprint: Option<String>,
    pub session_purpose: String,
    pub cross_project_checked: bool,
    pub analysis_base_ref_kind: Option<String>,
    pub analysis_base_ref: Option<String>,
    pub analysis_base_display_name: Option<String>,
    pub analysis_workspace_kind: String,
    pub analysis_workspace_path: Option<String>,
    pub analysis_base_commit: Option<String>,
    pub analysis_base_locked_at: Option<String>,
}

/// Lightweight latest child-session lookup response.
#[derive(Debug, Serialize)]
pub struct LatestChildSessionIdResponse {
    pub session_id: String,
    pub purpose: Option<String>,
    pub latest_child_session_id: Option<String>,
}

impl From<IdeationSession> for IdeationSessionResponse {
    fn from(session: IdeationSession) -> Self {
        let gap_score = session.verification_gap_score.map(|score| score as i32);
        Self {
            id: session.id.as_str().to_string(),
            project_id: session.project_id.as_str().to_string(),
            title: session.title,
            title_source: session.title_source,
            status: session.status.to_string(),
            plan_artifact_id: session.plan_artifact_id.map(|id| id.as_str().to_string()),
            seed_task_id: session.seed_task_id.map(|id| id.as_str().to_string()),
            parent_session_id: session.parent_session_id.map(|id| id.as_str().to_string()),
            created_at: session.created_at.to_rfc3339(),
            updated_at: session.updated_at.to_rfc3339(),
            archived_at: session.archived_at.map(|dt| dt.to_rfc3339()),
            converted_at: session.converted_at.map(|dt| dt.to_rfc3339()),
            team_mode: session.team_mode,
            team_config: session
                .team_config_json
                .and_then(|s| serde_json::from_str(&s).ok()),
            verification_status: session.verification_status.to_string(),
            verification_in_progress: session.verification_in_progress,
            gap_score,
            inherited_plan_artifact_id: session
                .inherited_plan_artifact_id
                .map(|id| id.as_str().to_string()),
            source_project_id: session.source_project_id,
            source_session_id: session.source_session_id,
            source_task_id: session.source_task_id.map(|id| id.as_str().to_string()),
            source_context_type: session.source_context_type,
            source_context_id: session.source_context_id,
            spawn_reason: session.spawn_reason,
            blocker_fingerprint: session.blocker_fingerprint,
            session_purpose: session.session_purpose.to_string(),
            cross_project_checked: session.cross_project_checked,
            analysis_base_ref_kind: session.analysis.base_ref_kind.map(|kind| kind.to_string()),
            analysis_base_ref: session.analysis.base_ref,
            analysis_base_display_name: session.analysis.base_display_name,
            analysis_workspace_kind: session.analysis.workspace_kind.to_string(),
            analysis_workspace_path: session.analysis.workspace_path,
            analysis_base_commit: session.analysis.base_commit,
            analysis_base_locked_at: session.analysis.base_locked_at.map(|dt| dt.to_rfc3339()),
        }
    }
}

/// Response for session with proposals and messages
#[derive(Debug, Serialize)]
pub struct SessionWithDataResponse {
    pub session: IdeationSessionResponse,
    pub proposals: Vec<TaskProposalResponse>,
    pub messages: Vec<ChatMessageResponse>,
}

// ============================================================================
// Session Group Types (for paginated session browser)
// ============================================================================

/// Response for session group counts (counts of sessions per display group)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionGroupCountsResponse {
    pub drafts: u32,
    pub in_progress: u32,
    pub accepted: u32,
    pub done: u32,
    pub archived: u32,
}

impl From<crate::domain::repositories::ideation_session_repository::SessionGroupCounts>
    for SessionGroupCountsResponse
{
    fn from(
        c: crate::domain::repositories::ideation_session_repository::SessionGroupCounts,
    ) -> Self {
        Self {
            drafts: c.drafts,
            in_progress: c.in_progress,
            accepted: c.accepted,
            done: c.done,
            archived: c.archived,
        }
    }
}

/// Task progress summary included in paginated session responses
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProgressResponse {
    pub idle: u32,
    pub active: u32,
    pub done: u32,
    pub total: u32,
}

/// Ideation session with optional progress data and parent title
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeationSessionWithProgressResponse {
    #[serde(flatten)]
    pub session: IdeationSessionResponse,
    /// Task progress — populated for accepted sub-groups (in_progress, accepted, done); None otherwise
    pub progress: Option<SessionProgressResponse>,
    /// Parent session title resolved server-side via LEFT JOIN
    pub parent_session_title: Option<String>,
    /// Count of verification child sessions (session_purpose = 'verification') for this session
    pub verification_child_count: u32,
    /// True when pending_initial_prompt IS NOT NULL — session is waiting for capacity
    pub has_pending_prompt: bool,
}

impl From<crate::domain::repositories::ideation_session_repository::IdeationSessionWithProgress>
    for IdeationSessionWithProgressResponse
{
    fn from(
        item: crate::domain::repositories::ideation_session_repository::IdeationSessionWithProgress,
    ) -> Self {
        let progress = item.progress.map(|p| SessionProgressResponse {
            idle: p.idle,
            active: p.active,
            done: p.done,
            total: p.total,
        });
        Self {
            session: IdeationSessionResponse::from(item.session),
            progress,
            parent_session_title: item.parent_session_title,
            verification_child_count: item.verification_child_count,
            has_pending_prompt: item.has_pending_prompt,
        }
    }
}

/// Paginated list of sessions in a display group
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListResponse {
    pub sessions: Vec<IdeationSessionWithProgressResponse>,
    pub total: u32,
    pub has_more: bool,
    pub offset: u32,
}

// ============================================================================
// Proposal Types
// ============================================================================

/// Input for creating a new task proposal
#[derive(Debug, Deserialize)]
pub struct CreateProposalInput {
    pub session_id: String,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub steps: Option<Vec<String>>,
    pub acceptance_criteria: Option<Vec<String>>,
    pub affected_paths: Option<Vec<String>>,
    pub priority: Option<String>,
    pub complexity: Option<String>,
    /// Optional target project ID for cross-project proposals
    pub target_project: Option<String>,
    /// Optional list of proposal IDs this proposal depends on
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Expected total number of proposals for this session (set-once gating)
    pub expected_proposal_count: Option<u32>,
}

/// Input for updating a task proposal
#[derive(Debug, Deserialize)]
pub struct UpdateProposalInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub steps: Option<Vec<String>>,
    pub acceptance_criteria: Option<Vec<String>>,
    pub affected_paths: Option<Vec<String>>,
    pub user_priority: Option<String>,
    pub complexity: Option<String>,
    /// Optional target project ID for cross-project proposals
    pub target_project: Option<String>,
    /// Additive: proposal IDs this proposal should depend on
    #[serde(default)]
    pub add_depends_on: Vec<String>,
    /// Additive: proposal IDs this proposal should block (reverse direction)
    #[serde(default)]
    pub add_blocks: Vec<String>,
}

/// Response for TaskProposal
#[derive(Debug, Serialize)]
pub struct TaskProposalResponse {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub affected_paths: Vec<String>,
    pub suggested_priority: String,
    pub priority_score: i32,
    pub priority_reason: Option<String>,
    pub estimated_complexity: String,
    pub user_priority: Option<String>,
    pub user_modified: bool,
    pub status: String,
    pub selected: bool,
    pub created_task_id: Option<String>,
    pub plan_artifact_id: Option<String>,
    pub plan_version_at_creation: Option<u32>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    /// Optional target project ID for cross-project proposals
    pub target_project: Option<String>,
    /// Partial failure contract: non-fatal dependency errors encountered during create/update
    #[serde(default)]
    pub dependency_errors: Vec<String>,
    /// Whether the auto-accept pipeline was triggered for this session
    pub auto_accept_triggered: bool,
}

impl From<TaskProposal> for TaskProposalResponse {
    fn from(proposal: TaskProposal) -> Self {
        // Parse JSON strings to Vec<String>, defaulting to empty vec
        let steps: Vec<String> = proposal
            .steps
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let acceptance_criteria: Vec<String> = proposal
            .acceptance_criteria
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let affected_paths: Vec<String> = proposal
            .affected_paths
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Self {
            id: proposal.id.as_str().to_string(),
            session_id: proposal.session_id.as_str().to_string(),
            title: proposal.title,
            description: proposal.description,
            category: proposal.category.to_string(),
            steps,
            acceptance_criteria,
            affected_paths,
            suggested_priority: proposal.suggested_priority.to_string(),
            priority_score: proposal.priority_score,
            priority_reason: proposal.priority_reason,
            estimated_complexity: proposal.estimated_complexity.to_string(),
            user_priority: proposal.user_priority.map(|p| p.to_string()),
            user_modified: proposal.user_modified,
            status: proposal.status.to_string(),
            selected: proposal.selected,
            created_task_id: proposal.created_task_id.map(|id| id.as_str().to_string()),
            plan_artifact_id: proposal.plan_artifact_id.map(|id| id.as_str().to_string()),
            plan_version_at_creation: proposal.plan_version_at_creation,
            sort_order: proposal.sort_order,
            created_at: proposal.created_at.to_rfc3339(),
            updated_at: proposal.updated_at.to_rfc3339(),
            target_project: proposal.target_project.clone(),
            dependency_errors: Vec::new(),
            auto_accept_triggered: false,
        }
    }
}

/// Response for priority assessment
#[derive(Debug, Serialize)]
pub struct PriorityAssessmentResponse {
    pub proposal_id: String,
    pub priority: String,
    pub score: i32,
    pub reason: String,
}

// ============================================================================
// Dependency and Graph Types
// ============================================================================

/// Response for DependencyGraph
#[derive(Debug, Serialize)]
pub struct DependencyGraphResponse {
    pub nodes: Vec<DependencyGraphNodeResponse>,
    pub edges: Vec<DependencyGraphEdgeResponse>,
    pub critical_path: Vec<String>,
    pub has_cycles: bool,
    pub cycles: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Serialize)]
pub struct DependencyGraphNodeResponse {
    pub proposal_id: String,
    pub title: String,
    pub in_degree: usize,
    pub out_degree: usize,
}

#[derive(Debug, Serialize)]
pub struct DependencyGraphEdgeResponse {
    pub from: String,
    pub to: String,
}

impl From<DependencyGraph> for DependencyGraphResponse {
    fn from(graph: DependencyGraph) -> Self {
        Self {
            nodes: graph
                .nodes
                .into_iter()
                .map(|n| DependencyGraphNodeResponse {
                    proposal_id: n.proposal_id.as_str().to_string(),
                    title: n.title,
                    in_degree: n.in_degree,
                    out_degree: n.out_degree,
                })
                .collect(),
            edges: graph
                .edges
                .into_iter()
                .map(|e| DependencyGraphEdgeResponse {
                    from: e.from.as_str().to_string(),
                    to: e.to.as_str().to_string(),
                })
                .collect(),
            critical_path: graph
                .critical_path
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            has_cycles: graph.has_cycles,
            cycles: graph.cycles.map(|cs| {
                cs.into_iter()
                    .map(|c| c.into_iter().map(|id| id.as_str().to_string()).collect())
                    .collect()
            }),
        }
    }
}

/// Input for apply proposals
#[derive(Debug, Deserialize)]
pub struct ApplyProposalsInput {
    pub session_id: String,
    pub proposal_ids: Vec<String>,
    pub target_column: String,
    /// Per-plan override for base branch (None = use project default)
    #[serde(default)]
    pub base_branch_override: Option<String>,
}

/// Core result of apply proposals — transport-agnostic, usable from Tauri IPC and HTTP contexts.
///
/// Contains all information produced by [`apply_proposals_core`], plus context fields required
/// by Tauri-specific side effects (scheduler trigger, session namer).
#[derive(Debug)]
pub struct ApplyProposalsResult {
    pub created_task_ids: Vec<String>,
    /// Number of proposal-to-proposal dependency edges created (excludes merge task edges).
    pub dependencies_created: usize,
    /// Number of plan tasks created (excludes the auto-generated merge task).
    pub tasks_created: usize,
    /// Human-readable summary of the finalization result.
    pub message: Option<String>,
    pub warnings: Vec<String>,
    pub session_converted: bool,
    pub execution_plan_id: Option<String>,
    /// Project ID — for Tauri `emit_queue_changed` and HTTP scope validation.
    pub project_id: String,
    /// Session ID — for Tauri session namer re-trigger.
    pub session_id: String,
    /// Whether any tasks were set to Ready status — triggers Tauri scheduler.
    pub any_ready_tasks: bool,
    /// Whether the session title was set by the user — suppresses session namer.
    pub is_user_title: bool,
    /// Applied proposal titles — context for session namer prompt.
    pub proposal_titles: Vec<String>,
}

/// Response for apply proposals
#[derive(Debug, Serialize)]
pub struct ApplyProposalsResultResponse {
    pub created_task_ids: Vec<String>,
    pub dependencies_created: usize,
    pub tasks_created: usize,
    pub message: Option<String>,
    pub warnings: Vec<String>,
    pub session_converted: bool,
    pub execution_plan_id: Option<String>,
}

impl From<ApplyProposalsResult> for ApplyProposalsResultResponse {
    fn from(r: ApplyProposalsResult) -> Self {
        Self {
            created_task_ids: r.created_task_ids,
            dependencies_created: r.dependencies_created,
            tasks_created: r.tasks_created,
            message: r.message,
            warnings: r.warnings,
            session_converted: r.session_converted,
            execution_plan_id: r.execution_plan_id,
        }
    }
}

// ============================================================================
// Chat Message Types
// ============================================================================

/// Input for sending a chat message
#[derive(Debug, Deserialize)]
pub struct SendChatMessageInput {
    /// Session ID for session context (optional, mutually exclusive with project_id and task_id)
    pub session_id: Option<String>,
    /// Project ID for project context (optional, only used if session_id is None)
    pub project_id: Option<String>,
    /// Task ID for task context (optional, only used if session_id and project_id are None)
    pub task_id: Option<String>,
    /// Role of the message sender
    pub role: String,
    /// Message content (supports Markdown)
    pub content: String,
    /// Optional metadata (JSON)
    pub metadata: Option<String>,
    /// Optional parent message ID for threading
    pub parent_message_id: Option<String>,
}

// ============================================================================
// Orchestrator Types
// ============================================================================

/// Input for sending a message to the orchestrator
#[derive(Debug, Deserialize)]
pub struct SendOrchestratorMessageInput {
    pub session_id: String,
    pub content: String,
}

/// Response from the orchestrator
#[derive(Debug, Serialize)]
pub struct OrchestratorMessageResponse {
    pub response_text: String,
    pub proposals_created: Vec<TaskProposalResponse>,
    pub tool_calls: Vec<ToolCallResultResponse>,
}

/// Tool call result response
#[derive(Debug, Serialize)]
pub struct ToolCallResultResponse {
    pub tool_name: String,
    pub success: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

// ============================================================================
// Proposal Migration Types
// ============================================================================

/// Input for migrating proposals from one session to another.
///
/// Used by both the Tauri IPC command and the HTTP endpoint.
/// If proposal_ids is omitted, all proposals from the source session are considered.
/// If target_project_filter is provided, only proposals with a matching target_project are migrated.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateProposalsInput {
    /// ID of the source ideation session
    pub source_session_id: String,
    /// ID of the target ideation session
    pub target_session_id: String,
    /// Optional list of proposal IDs to migrate. If omitted, migrate all (subject to filter).
    pub proposal_ids: Option<Vec<String>>,
    /// Optional: only migrate proposals whose target_project matches this string.
    pub target_project_filter: Option<String>,
}

/// A single migrated proposal mapping (source → target).
#[derive(Debug, Serialize)]
pub struct MigratedProposalEntry {
    pub source_id: String,
    pub target_id: String,
}

/// A dependency that was dropped because one or both ends were not in the migration set.
#[derive(Debug, Serialize)]
pub struct DroppedDependency {
    /// The proposal ID that had the dependency (in the source session)
    pub proposal_id: String,
    /// The dependency that was dropped
    pub dropped_dep_id: String,
    /// Why it was dropped
    pub reason: String,
}

/// Response from migrate_proposals.
#[derive(Debug, Serialize)]
pub struct MigrateProposalsResult {
    /// Successfully migrated proposal mappings
    pub migrated: Vec<MigratedProposalEntry>,
    /// Dependencies that were dropped with explanations
    pub dropped_dependencies: Vec<DroppedDependency>,
}
