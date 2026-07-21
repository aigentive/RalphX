// Type definitions for task_commands module

use crate::domain::entities::Task;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct TaskHistoryAvailabilityResponse {
    pub has_history: bool,
    pub task_count: u32,
}

/// Input for creating a new task
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub project_id: String,
    pub title: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i32>,
    pub steps: Option<Vec<String>>,
    pub ideation_session_id: Option<String>,
    pub execution_plan_id: Option<String>,
}

/// Input for updating a task
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub priority: Option<i32>,
    pub internal_status: Option<String>,
}

/// Input for answering an agent's question
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerUserQuestionInput {
    pub task_id: String,
    pub selected_options: Vec<String>,
    #[serde(default)]
    pub custom_response: Option<String>,
}

/// Response for the answer_user_question command
#[derive(Debug, Serialize)]
pub struct AnswerUserQuestionResponse {
    pub task_id: String,
    pub resumed_status: String,
    pub answer_recorded: bool,
}

/// Input for injecting a task mid-loop
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectTaskInput {
    /// The project ID to inject the task into
    pub project_id: String,
    /// Title of the task
    pub title: String,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
    /// Category (defaults to "feature")
    #[serde(default)]
    pub category: Option<String>,
    /// Where to inject: "backlog" (deferred) or "planned" (immediate queue)
    #[serde(default = "super::helpers::default_target")]
    pub target: String,
    /// If true and target is "planned", make this task the highest priority
    #[serde(default)]
    pub make_next: bool,
}

/// Response for the inject_task command
#[derive(Debug, Serialize)]
pub struct InjectTaskResponse {
    pub task: TaskResponse,
    pub target: String,
    pub priority: i32,
    pub make_next_applied: bool,
}

/// Response wrapper for task operations
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub project_id: String,
    pub category: String,
    pub title: String,
    pub description: Option<String>,
    pub priority: i32,
    pub internal_status: String,
    pub needs_review_point: bool,
    pub ideation_session_id: Option<String>,
    pub execution_plan_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub archived_at: Option<String>,
    pub blocked_reason: Option<String>,
    pub task_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub metadata: Option<String>,
}

impl From<Task> for TaskResponse {
    fn from(task: Task) -> Self {
        Self {
            id: task.id.as_str().to_string(),
            project_id: task.project_id.as_str().to_string(),
            category: task.category.to_string(),
            title: task.title,
            description: task.description,
            priority: task.priority,
            internal_status: task.internal_status.as_str().to_string(),
            needs_review_point: task.needs_review_point,
            ideation_session_id: task.ideation_session_id.map(|id| id.as_str().to_string()),
            execution_plan_id: task.execution_plan_id.map(|id| id.as_str().to_string()),
            created_at: task.created_at.to_rfc3339(),
            updated_at: task.updated_at.to_rfc3339(),
            started_at: task.started_at.map(|dt| dt.to_rfc3339()),
            completed_at: task.completed_at.map(|dt| dt.to_rfc3339()),
            archived_at: task.archived_at.map(|dt| dt.to_rfc3339()),
            blocked_reason: task.blocked_reason,
            task_branch: task.task_branch,
            worktree_path: task.worktree_path,
            merge_commit_sha: task.merge_commit_sha,
            metadata: task.metadata,
        }
    }
}

/// Response for unblock_task — includes the updated task and an optional warning
/// when one or more dependencies are in Failed status.
#[derive(Debug, Serialize)]
pub struct UnblockTaskResponse {
    pub task: TaskResponse,
    /// Set when the task was unblocked despite having failed dependencies.
    /// e.g. "Task has failed dependencies: \"Setup DB\". Proceeding may produce broken output."
    pub warning: Option<String>,
}

/// Response for task cleanup operations
#[derive(Debug, Serialize)]
pub struct CleanupReportResponse {
    pub archived_count: usize,
    pub failed_count: usize,
    pub stopped_agents: usize,
}

/// Response for bulk cancel operations
#[derive(Debug, Serialize)]
pub struct BulkCancelResponse {
    pub cancelled_count: usize,
}

/// Response for bulk pause operations
#[derive(Debug, Serialize)]
pub struct BulkPauseResponse {
    pub paused_count: usize,
}

/// Response for bulk resume operations
#[derive(Debug, Serialize)]
pub struct BulkResumeResponse {
    pub resumed_count: usize,
}

/// Input for execution-plan-scoped lifecycle controls.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanControlInput {
    pub project_id: String,
    pub session_id: String,
    pub execution_plan_id: Option<String>,
}

/// Response for execution-plan-scoped lifecycle controls.
#[derive(Debug, Serialize)]
pub struct ExecutionPlanControlResponse {
    pub execution_plan_id: String,
    pub affected_count: usize,
}

/// Response for bulk archive operations
#[derive(Debug, Serialize)]
pub struct BulkArchiveResponse {
    pub archived_count: usize,
}

/// Response for paginated task list
#[derive(Debug, Serialize)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskResponse>,
    pub total: u32,
    pub has_more: bool,
    pub offset: u32,
}

/// Response for status transition options
#[derive(Debug, Serialize)]
pub struct StatusTransition {
    /// The internal status string (e.g., "ready", "cancelled")
    pub status: String,
    /// User-friendly label for the UI (e.g., "Ready for Work", "Cancel")
    pub label: String,
}

/// Response for historical state transition records
/// Used by StateTimelineNav for displaying task state history
#[derive(Debug, Serialize)]
pub struct StateTransitionResponse {
    /// Status transitioned from (null for initial state)
    pub from_status: Option<String>,
    /// Status transitioned to
    pub to_status: String,
    /// What triggered this transition (e.g., "user", "agent", "system")
    pub trigger: String,
    /// When the transition occurred (RFC3339 format)
    pub timestamp: String,
    /// Conversation ID associated with this state (for executing/reviewing states)
    pub conversation_id: Option<String>,
    /// Agent run ID that was started for this state
    pub agent_run_id: Option<String>,
}

// ============================================================================
// Task Graph Types (Phase 67)
// ============================================================================

/// Node in the task dependency graph
#[derive(Debug, Clone, Serialize)]
pub struct TaskGraphNode {
    /// Task ID
    pub task_id: String,
    /// Task title
    pub title: String,
    /// Task description (for card display)
    pub description: Option<String>,
    /// Task category (e.g., "feature", "bug", "chore")
    pub category: String,
    /// Internal status (e.g., "ready", "executing", "approved")
    pub internal_status: String,
    /// Task priority
    pub priority: i32,
    /// Number of tasks this task depends on (blockers)
    pub in_degree: u32,
    /// Number of tasks that depend on this task
    pub out_degree: u32,
    /// Computed tier level (0 = no dependencies, higher = more dependencies)
    pub tier: u32,
    /// Plan artifact ID if task came from a plan
    pub plan_artifact_id: Option<String>,
    /// Source proposal ID if task came from ideation
    pub source_proposal_id: Option<String>,
    /// Execution plan ID if task is linked to an execution plan
    pub execution_plan_id: Option<String>,
}

/// Edge in the task dependency graph
#[derive(Debug, Clone, Serialize)]
pub struct TaskGraphEdge {
    /// Source task ID (the blocking task)
    pub source: String,
    /// Target task ID (the blocked task)
    pub target: String,
    /// Whether this edge is on the critical path
    pub is_critical_path: bool,
}

/// Status summary for a plan group
#[derive(Debug, Clone, Serialize, Default)]
pub struct StatusSummary {
    /// Count of tasks in each status category
    pub backlog: u32,
    pub ready: u32,
    pub blocked: u32,
    pub executing: u32,
    pub qa: u32,
    pub review: u32,
    pub merge: u32,
    pub completed: u32,
    pub terminal: u32,
}

/// Information about a plan group in the graph
#[derive(Debug, Clone, Serialize)]
pub struct PlanGroupInfo {
    /// Plan artifact ID
    pub plan_artifact_id: String,
    /// Ideation session ID
    pub session_id: String,
    /// Session title (if set)
    pub session_title: Option<String>,
    /// Task IDs belonging to this plan
    pub task_ids: Vec<String>,
    /// Status counts for tasks in this plan
    pub status_summary: StatusSummary,
    /// Execution plan ID for this plan group (if any task has one)
    pub execution_plan_id: Option<String>,
}

/// Response for the task dependency graph
#[derive(Debug, Serialize)]
pub struct TaskDependencyGraphResponse {
    /// All task nodes in the graph
    pub nodes: Vec<TaskGraphNode>,
    /// All dependency edges
    pub edges: Vec<TaskGraphEdge>,
    /// Plan groups with their tasks
    pub plan_groups: Vec<PlanGroupInfo>,
    /// Task IDs on the critical path (longest dependency chain)
    pub critical_path: Vec<String>,
    /// Whether the graph has cycles (should be false in valid graphs)
    pub has_cycles: bool,
}

// ============================================================================
// Timeline Event Types (Phase 67 - Task D.1)
// ============================================================================

/// Event type for timeline entries
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventType {
    /// Task status changed
    StatusChange,
    /// Plan was accepted (creates tasks)
    PlanAccepted,
    /// All tasks in a plan completed
    PlanCompleted,
}

/// A single event in the execution timeline
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEvent {
    /// Unique event ID
    pub id: String,
    /// When the event occurred (RFC3339 format)
    pub timestamp: String,
    /// Task ID (if applicable)
    pub task_id: Option<String>,
    /// Task title for display (if task-related)
    pub task_title: Option<String>,
    /// Event type (status_change, plan_accepted, plan_completed)
    pub event_type: TimelineEventType,
    /// Previous status (for status_change events)
    pub from_status: Option<String>,
    /// New status (for status_change events)
    pub to_status: Option<String>,
    /// Human-readable description of the event
    pub description: String,
    /// Who/what triggered this event (user, agent, system)
    pub trigger: Option<String>,
    /// Plan artifact ID (for plan-level events)
    pub plan_artifact_id: Option<String>,
    /// Session title (for plan-level events)
    pub session_title: Option<String>,
}

/// Response for the timeline events query
#[derive(Debug, Serialize)]
pub struct TimelineEventsResponse {
    /// Timeline events in chronological order (newest first)
    pub events: Vec<TimelineEvent>,
    /// Total count (for pagination)
    pub total: u32,
    /// Whether there are more events
    pub has_more: bool,
}
