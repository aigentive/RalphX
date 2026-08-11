//! TaskProposal entity

use chrono::{DateTime, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

use super::types::*;
use crate::entities::{ArtifactId, IdeationSessionId, TaskId, TaskProposalId};

/// A task proposal generated during an ideation session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProposal {
    /// Unique identifier for this proposal
    pub id: TaskProposalId,
    /// Session this proposal belongs to
    pub session_id: IdeationSessionId,
    /// Short title for the task
    pub title: String,
    /// Detailed description of what needs to be done
    pub description: Option<String>,
    /// Task category
    pub category: ProposalCategory,
    /// Implementation steps (JSON array of strings)
    pub steps: Option<String>,
    /// Acceptance criteria (JSON array of strings)
    pub acceptance_criteria: Option<String>,
    /// Coarse file/path scope hints captured during ideation (JSON array of strings)
    pub affected_paths: Option<String>,
    /// AI-suggested priority level
    pub suggested_priority: Priority,
    /// Numeric priority score (0-100, higher = more important)
    pub priority_score: i32,
    /// Explanation for why this priority was suggested
    pub priority_reason: Option<String>,
    /// Factors contributing to the priority score
    pub priority_factors: Option<PriorityFactors>,
    /// Estimated complexity
    pub estimated_complexity: Complexity,
    /// User-overridden priority (if different from suggested)
    pub user_priority: Option<Priority>,
    /// Whether the user has modified this proposal
    pub user_modified: bool,
    /// Current status in the workflow
    pub status: ProposalStatus,
    /// Whether this proposal is selected for conversion
    pub selected: bool,
    /// ID of the created task (if converted)
    pub created_task_id: Option<TaskId>,
    /// Reference to the implementation plan artifact
    pub plan_artifact_id: Option<ArtifactId>,
    /// Plan version when this proposal was created (for historical view)
    pub plan_version_at_creation: Option<u32>,
    /// Exact implementation blueprint linked to this proposal.
    pub blueprint_artifact_id: Option<ArtifactId>,
    /// Blueprint version when this proposal was created.
    pub blueprint_version_at_creation: Option<u32>,
    /// Sort order within the session
    pub sort_order: i32,
    /// When the proposal was created
    pub created_at: DateTime<Utc>,
    /// When the proposal was last updated
    pub updated_at: DateTime<Utc>,
    /// When the proposal was archived (soft-deleted). None = active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    /// Target project for task creation (optional override)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_project: Option<String>,
    /// ID of the source session this proposal was migrated from (traceability)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrated_from_session_id: Option<String>,
    /// ID of the original proposal this was migrated from (traceability)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrated_from_proposal_id: Option<String>,
}

impl TaskProposal {
    /// Creates a new task proposal with required fields
    pub fn new(
        session_id: IdeationSessionId,
        title: impl Into<String>,
        category: ProposalCategory,
        suggested_priority: Priority,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: TaskProposalId::new(),
            session_id,
            title: title.into(),
            description: None,
            category,
            steps: None,
            acceptance_criteria: None,
            affected_paths: None,
            suggested_priority,
            priority_score: 50,
            priority_reason: None,
            priority_factors: None,
            estimated_complexity: Complexity::default(),
            user_priority: None,
            user_modified: false,
            status: ProposalStatus::default(),
            selected: false,
            created_task_id: None,
            plan_artifact_id: None,
            plan_version_at_creation: None,
            blueprint_artifact_id: None,
            blueprint_version_at_creation: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            target_project: None,
            migrated_from_session_id: None,
            migrated_from_proposal_id: None,
        }
    }

    /// Returns the effective priority (user override or suggested)
    pub fn effective_priority(&self) -> Priority {
        self.user_priority.unwrap_or(self.suggested_priority)
    }

    /// Returns true if the proposal is pending
    pub fn is_pending(&self) -> bool {
        self.status == ProposalStatus::Pending
    }

    /// Returns true if the proposal has been accepted
    pub fn is_accepted(&self) -> bool {
        self.status == ProposalStatus::Accepted
    }

    /// Returns true if the proposal has been applied and created a task
    pub fn has_created_task(&self) -> bool {
        self.created_task_id.is_some()
    }

    /// Accepts the proposal
    pub fn accept(&mut self) {
        self.status = ProposalStatus::Accepted;
        self.updated_at = Utc::now();
    }

    /// Rejects the proposal
    pub fn reject(&mut self) {
        self.status = ProposalStatus::Rejected;
        self.selected = false;
        self.updated_at = Utc::now();
    }

    /// Sets the user priority override
    pub fn set_user_priority(&mut self, priority: Priority) {
        self.user_priority = Some(priority);
        self.user_modified = true;
        self.status = ProposalStatus::Modified;
        self.updated_at = Utc::now();
    }

    /// Links this proposal to a created task
    pub fn link_to_task(&mut self, task_id: TaskId) {
        self.created_task_id = Some(task_id);
        self.updated_at = Utc::now();
    }

    /// Toggles selection state
    pub fn toggle_selection(&mut self) {
        self.selected = !self.selected;
        self.updated_at = Utc::now();
    }

    /// Updates the updated_at timestamp to now
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    /// Deserialize a TaskProposal from a SQLite row
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let priority_factors_json: Option<String> = row.get("priority_factors")?;
        let priority_factors =
            priority_factors_json.and_then(|json| serde_json::from_str(&json).ok());

        Ok(Self {
            id: TaskProposalId::from_string(row.get::<_, String>("id")?),
            session_id: IdeationSessionId::from_string(row.get::<_, String>("session_id")?),
            title: row.get("title")?,
            description: row.get("description")?,
            category: row
                .get::<_, String>("category")?
                .parse()
                .unwrap_or(ProposalCategory::Feature),
            steps: row.get("steps")?,
            acceptance_criteria: row.get("acceptance_criteria")?,
            affected_paths: row.get("affected_paths")?,
            suggested_priority: row
                .get::<_, String>("suggested_priority")?
                .parse()
                .unwrap_or(Priority::Medium),
            priority_score: row.get("priority_score")?,
            priority_reason: row.get("priority_reason")?,
            priority_factors,
            estimated_complexity: row
                .get::<_, String>("estimated_complexity")?
                .parse()
                .unwrap_or(Complexity::Moderate),
            user_priority: row
                .get::<_, Option<String>>("user_priority")?
                .and_then(|s| s.parse().ok()),
            user_modified: row.get::<_, i32>("user_modified")? != 0,
            status: row
                .get::<_, String>("status")?
                .parse()
                .unwrap_or(ProposalStatus::Pending),
            selected: row.get::<_, i32>("selected")? != 0,
            created_task_id: row
                .get::<_, Option<String>>("created_task_id")?
                .map(TaskId::from_string),
            plan_artifact_id: row
                .get::<_, Option<String>>("plan_artifact_id")?
                .map(ArtifactId::from_string),
            plan_version_at_creation: row.get::<_, Option<u32>>("plan_version_at_creation")?,
            blueprint_artifact_id: row
                .get::<_, Option<String>>("blueprint_artifact_id")
                .unwrap_or(None)
                .map(ArtifactId::from_string),
            blueprint_version_at_creation: row
                .get::<_, Option<u32>>("blueprint_version_at_creation")
                .unwrap_or(None),
            sort_order: row.get("sort_order")?,
            created_at: parse_datetime_helper(row.get("created_at")?),
            updated_at: parse_datetime_helper(row.get("updated_at")?),
            archived_at: row
                .get::<_, Option<String>>("archived_at")?
                .map(parse_datetime_helper),
            target_project: row
                .get::<_, Option<String>>("target_project")
                .unwrap_or(None),
            migrated_from_session_id: row
                .get::<_, Option<String>>("migrated_from_session_id")
                .unwrap_or(None),
            migrated_from_proposal_id: row
                .get::<_, Option<String>>("migrated_from_proposal_id")
                .unwrap_or(None),
        })
    }
}
