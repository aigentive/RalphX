// Task entity - represents a task in RalphX
// Contains task metadata, status, and timestamps

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

use super::super::entities::artifact::ArtifactId;
use super::super::entities::types::{ExecutionPlanId, TaskProposalId};
use super::{IdeationSessionId, InternalStatus, ProjectId, TaskId};

/// Category of a task in the execution pipeline
/// Determines routing behavior in the scheduler and merge handler
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCategory {
    /// Standard task executed by an AI agent
    Regular,
    /// System-managed task that merges a plan branch into main
    PlanMerge,
}

impl serde::Serialize for TaskCategory {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for TaskCategory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(s.parse().unwrap_or(TaskCategory::Regular))
    }
}

impl Default for TaskCategory {
    fn default() -> Self {
        Self::Regular
    }
}

impl std::fmt::Display for TaskCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskCategory::Regular => write!(f, "regular"),
            TaskCategory::PlanMerge => write!(f, "plan_merge"),
        }
    }
}

impl std::str::FromStr for TaskCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "plan_merge" => Ok(TaskCategory::PlanMerge),
            // Treat all other strings (including legacy categories like "feature", "bug", etc.) as Regular
            _ => Ok(TaskCategory::Regular),
        }
    }
}

/// A task managed by RalphX
/// Tasks belong to a project and have an internal status that follows the state machine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier for this task
    pub id: TaskId,
    /// The project this task belongs to
    pub project_id: ProjectId,
    /// Category of the task (Regular or PlanMerge)
    pub category: TaskCategory,
    /// Short title describing the task
    pub title: String,
    /// Optional longer description with details
    pub description: Option<String>,
    /// Priority for ordering (higher = more important, 0 = default)
    pub priority: i32,
    /// Current internal status (follows state machine)
    pub internal_status: InternalStatus,
    /// Whether this task needs a review point (human-in-loop checkpoint)
    /// When true, execution will pause before this task for human approval
    #[serde(default)]
    pub needs_review_point: bool,
    /// Source proposal this task was created from (for traceability)
    /// Used by worker to fetch original proposal context (acceptance criteria, steps)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_proposal_id: Option<TaskProposalId>,
    /// Plan artifact linked to this task (inherited from proposal)
    /// Used by worker to fetch implementation context during execution
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_artifact_id: Option<ArtifactId>,
    /// Blueprint artifact paired with the plan overview at task creation.
    /// This immutable snapshot prevents later plan edits from changing execution context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_blueprint_artifact_id: Option<ArtifactId>,
    /// Direct link to the originating ideation session
    /// Always valid (no FK constraint issues unlike plan_artifact_id fallback)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ideation_session_id: Option<IdeationSessionId>,
    /// Execution plan this task belongs to (set at proposal-apply time)
    /// Enables unique branch naming per re-accept attempt
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_plan_id: Option<ExecutionPlanId>,
    /// When the task was created
    pub created_at: DateTime<Utc>,
    /// When the task was last updated
    pub updated_at: DateTime<Utc>,
    /// When execution started (first time status became Executing)
    pub started_at: Option<DateTime<Utc>>,
    /// When the task was completed (status became Approved)
    pub completed_at: Option<DateTime<Utc>>,
    /// When the task was archived (soft-deleted). None = active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    /// Reason why the task is blocked (only set when status is Blocked)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    /// Git branch name for this task (Phase 66 - Git Branch Isolation)
    /// Format: ralphx/{project-slug}/task-{task-id}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_branch: Option<String>,
    /// Branch/ref used when the task branch was first created.
    /// Used as display context for immutable task-owned diffs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_branch_base_ref: Option<String>,
    /// Exact commit SHA resolved from task_branch_base_ref at task branch creation.
    /// Diff, validation, and review guards use this immutable SHA instead of the moving plan branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_branch_base_sha: Option<String>,
    /// Worktree path for this task (Worktree mode only, Phase 66)
    /// Only set when project.git_mode == Worktree
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    /// Commit SHA of the merge commit after task branch is merged (Phase 66)
    /// Set when task transitions to Merged state
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_commit_sha: Option<String>,
    /// Generic JSON metadata (Phase 108)
    /// Used for merge error context (error message, branch names) and future structured data
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    /// RFC3339 timestamp set when merge pipeline is actively running (Phase 53 migration)
    /// Replaces the JSON metadata key to prevent race-condition clobbers by concurrent writers.
    /// NULL = not active. Non-null = pipeline running (auto-expires after attempt_merge_deadline_secs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_pipeline_active: Option<String>,
}

impl Task {
    /// Creates a new task with the given project_id and title
    /// Uses sensible defaults:
    /// - category: TaskCategory::Regular
    /// - internal_status: Backlog
    /// - priority: 0
    /// - needs_review_point: false
    /// - timestamps set to now
    pub fn new(project_id: ProjectId, title: String) -> Self {
        let now = Utc::now();
        Self {
            id: TaskId::new(),
            project_id,
            category: TaskCategory::Regular,
            title,
            description: None,
            priority: 0,
            internal_status: InternalStatus::Backlog,
            needs_review_point: false,
            source_proposal_id: None,
            plan_artifact_id: None,
            plan_blueprint_artifact_id: None,
            ideation_session_id: None,
            execution_plan_id: None,
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
            archived_at: None,
            blocked_reason: None,
            task_branch: None,
            task_branch_base_ref: None,
            task_branch_base_sha: None,
            worktree_path: None,
            merge_commit_sha: None,
            metadata: None,
            merge_pipeline_active: None,
        }
    }

    /// Creates a new task with a specific category
    pub fn new_with_category(project_id: ProjectId, title: String, category: TaskCategory) -> Self {
        let mut task = Self::new(project_id, title);
        task.category = category;
        task
    }

    /// Updates the updated_at timestamp to now
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    /// Sets the description and updates the timestamp
    pub fn set_description(&mut self, description: Option<String>) {
        self.description = description;
        self.touch();
    }

    /// Sets the priority and updates the timestamp
    pub fn set_priority(&mut self, priority: i32) {
        self.priority = priority;
        self.touch();
    }

    /// Sets whether this task needs a review point (human-in-loop checkpoint)
    pub fn set_needs_review_point(&mut self, needs_review: bool) {
        self.needs_review_point = needs_review;
        self.touch();
    }

    /// Returns true if this task is in a terminal state.
    /// Delegates to InternalStatus::is_terminal() as the single source of truth.
    /// Terminal: Merged, Failed, Cancelled, Stopped, MergeIncomplete.
    /// NOT terminal: Approved (→ PendingMerge), Paused (can resume).
    pub fn is_terminal(&self) -> bool {
        self.internal_status.is_terminal()
    }

    /// Returns true if this task is currently being worked on
    pub fn is_active(&self) -> bool {
        matches!(
            self.internal_status,
            InternalStatus::Executing
                | InternalStatus::QaRefining
                | InternalStatus::QaTesting
                | InternalStatus::PendingReview
        )
    }

    /// Deserialize a Task from a SQLite row.
    /// Expects columns: id, project_id, category, title, description, priority,
    /// internal_status, needs_review_point, source_proposal_id, plan_artifact_id,
    /// created_at, updated_at, started_at, completed_at, archived_at
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: TaskId::from_string(row.get("id")?),
            project_id: ProjectId::from_string(row.get("project_id")?),
            category: row
                .get::<_, String>("category")?
                .parse()
                .unwrap_or(TaskCategory::Regular),
            title: row.get("title")?,
            description: row.get("description")?,
            priority: row.get("priority")?,
            internal_status: row
                .get::<_, String>("internal_status")?
                .parse()
                .unwrap_or(InternalStatus::Backlog),
            needs_review_point: row
                .get::<_, Option<bool>>("needs_review_point")?
                .unwrap_or(false),
            source_proposal_id: row
                .get::<_, Option<String>>("source_proposal_id")?
                .map(TaskProposalId::from_string),
            plan_artifact_id: row
                .get::<_, Option<String>>("plan_artifact_id")?
                .map(ArtifactId::from_string),
            plan_blueprint_artifact_id: row
                .get::<_, Option<String>>("plan_blueprint_artifact_id")
                .unwrap_or(None)
                .map(ArtifactId::from_string),
            ideation_session_id: row
                .get::<_, Option<String>>("ideation_session_id")?
                .map(IdeationSessionId::from_string),
            execution_plan_id: row
                .get::<_, Option<String>>("execution_plan_id")?
                .map(ExecutionPlanId::from_string),
            created_at: Self::parse_datetime(row.get("created_at")?),
            updated_at: Self::parse_datetime(row.get("updated_at")?),
            started_at: row
                .get::<_, Option<String>>("started_at")?
                .map(Self::parse_datetime),
            completed_at: row
                .get::<_, Option<String>>("completed_at")?
                .map(Self::parse_datetime),
            archived_at: row
                .get::<_, Option<String>>("archived_at")?
                .map(Self::parse_datetime),
            blocked_reason: row.get("blocked_reason")?,
            task_branch: row.get("task_branch")?,
            task_branch_base_ref: row.get("task_branch_base_ref")?,
            task_branch_base_sha: row.get("task_branch_base_sha")?,
            worktree_path: row.get("worktree_path")?,
            merge_commit_sha: row.get("merge_commit_sha")?,
            metadata: row.get("metadata")?,
            merge_pipeline_active: row.get("merge_pipeline_active")?,
        })
    }

    /// Parse a datetime string from SQLite into a DateTime<Utc>
    /// Handles both RFC3339 format and SQLite's CURRENT_TIMESTAMP format
    pub fn parse_datetime(s: String) -> DateTime<Utc> {
        // Try RFC3339 first (our preferred format)
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return dt.with_timezone(&Utc);
        }
        // Try SQLite's default datetime format (YYYY-MM-DD HH:MM:SS)
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S") {
            return Utc.from_utc_datetime(&dt);
        }
        // Fallback to now if parsing fails
        Utc::now()
    }
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
