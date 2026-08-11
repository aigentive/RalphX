// TaskStep entity - represents a step within a task execution
// Provides deterministic progress tracking for worker agents

use chrono::{DateTime, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

use super::{TaskId, TaskStepId};

/// Status of a task step
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStepStatus {
    /// Waiting to be worked on
    Pending,
    /// Currently being executed
    InProgress,
    /// Finished successfully
    Completed,
    /// Not applicable or deferred
    Skipped,
    /// Needs attention
    Failed,
    /// Task was cancelled
    Cancelled,
}

impl TaskStepStatus {
    /// Returns true if this is a terminal status (no further transitions)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStepStatus::Completed
                | TaskStepStatus::Skipped
                | TaskStepStatus::Failed
                | TaskStepStatus::Cancelled
        )
    }

    /// Converts status to database string representation
    pub fn to_db_string(&self) -> &'static str {
        match self {
            TaskStepStatus::Pending => "pending",
            TaskStepStatus::InProgress => "in_progress",
            TaskStepStatus::Completed => "completed",
            TaskStepStatus::Skipped => "skipped",
            TaskStepStatus::Failed => "failed",
            TaskStepStatus::Cancelled => "cancelled",
        }
    }

    /// Parses status from database string representation
    pub fn from_db_string(s: &str) -> Result<Self, String> {
        match s {
            "pending" => Ok(TaskStepStatus::Pending),
            "in_progress" => Ok(TaskStepStatus::InProgress),
            "completed" => Ok(TaskStepStatus::Completed),
            "skipped" => Ok(TaskStepStatus::Skipped),
            "failed" => Ok(TaskStepStatus::Failed),
            "cancelled" => Ok(TaskStepStatus::Cancelled),
            _ => Err(format!("Invalid TaskStepStatus: {}", s)),
        }
    }
}

/// A step within a task execution
/// Tracks discrete checkpoints that agents progress through
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    /// Unique identifier for this step
    pub id: TaskStepId,
    /// The task this step belongs to
    pub task_id: TaskId,
    /// Short title describing the step
    pub title: String,
    /// Optional longer description with details
    pub description: Option<String>,
    /// Current status of the step
    pub status: TaskStepStatus,
    /// Order within the task (0-indexed)
    pub sort_order: i32,
    /// Optional dependency - this step depends on another step completing first
    pub depends_on: Option<TaskStepId>,
    /// Who created this step (e.g., "user", "agent", "proposal", "system")
    pub created_by: String,
    /// Optional note about completion (used for skip/fail reasons or completion notes)
    pub completion_note: Option<String>,
    /// When the step was created
    pub created_at: DateTime<Utc>,
    /// When the step was last updated
    pub updated_at: DateTime<Utc>,
    /// When execution started (status became InProgress)
    pub started_at: Option<DateTime<Utc>>,
    /// When the step reached a terminal state
    pub completed_at: Option<DateTime<Utc>>,
    /// Optional parent step ID - if set, this step is a sub-step
    pub parent_step_id: Option<TaskStepId>,
    /// Optional scope context - JSON containing STRICT SCOPE for sub-steps
    pub scope_context: Option<String>,
}

impl TaskStep {
    /// Creates a new task step with the given task_id, title, sort_order, and created_by
    /// Uses sensible defaults:
    /// - status: Pending
    /// - timestamps set to now
    /// - no description, dependencies, or completion notes
    pub fn new(task_id: TaskId, title: String, sort_order: i32, created_by: String) -> Self {
        let now = Utc::now();
        Self {
            id: TaskStepId::new(),
            task_id,
            title,
            description: None,
            status: TaskStepStatus::Pending,
            sort_order,
            depends_on: None,
            created_by,
            completion_note: None,
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
            parent_step_id: None,
            scope_context: None,
        }
    }

    /// Returns true if this step can be started
    /// A step can start if it's in Pending status
    pub fn can_start(&self) -> bool {
        self.status == TaskStepStatus::Pending
    }

    /// Returns true if this step is in a terminal state
    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    /// Updates the updated_at timestamp to now
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    /// Deserializes a TaskStep from a SQLite row
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let id: String = row.get(0)?;
        let task_id: String = row.get(1)?;
        let title: String = row.get(2)?;
        let description: Option<String> = row.get(3)?;
        let status_str: String = row.get(4)?;
        let sort_order: i32 = row.get(5)?;
        let depends_on_str: Option<String> = row.get(6)?;
        let created_by: String = row.get(7)?;
        let completion_note: Option<String> = row.get(8)?;
        let created_at_str: String = row.get(9)?;
        let updated_at_str: String = row.get(10)?;
        let started_at_str: Option<String> = row.get(11)?;
        let completed_at_str: Option<String> = row.get(12)?;
        let parent_step_id_str: Option<String> = row.get(13)?;
        let scope_context: Option<String> = row.get(14)?;

        let status = TaskStepStatus::from_db_string(&status_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            )
        })?;

        let depends_on = depends_on_str.map(TaskStepId::from_string);

        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .with_timezone(&Utc);

        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .with_timezone(&Utc);

        let started_at = started_at_str
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

        let completed_at = completed_at_str
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    12,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

        Ok(Self {
            id: TaskStepId::from_string(id),
            task_id: TaskId::from_string(task_id),
            title,
            description,
            status,
            sort_order,
            depends_on,
            created_by,
            completion_note,
            created_at,
            updated_at,
            started_at,
            completed_at,
            parent_step_id: parent_step_id_str.map(TaskStepId::from_string),
            scope_context,
        })
    }
}

/// Summary of step progress for a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepProgressSummary {
    /// The task this progress summary is for
    pub task_id: String,
    /// Total number of steps
    pub total: u32,
    /// Number of completed steps
    pub completed: u32,
    /// Number of in-progress steps
    pub in_progress: u32,
    /// Number of pending steps
    pub pending: u32,
    /// Number of skipped steps
    pub skipped: u32,
    /// Number of failed steps
    pub failed: u32,
    /// Current step being worked on (first InProgress step)
    pub current_step: Option<TaskStep>,
    /// Next step to be started (first Pending step)
    pub next_step: Option<TaskStep>,
    /// Percentage complete over non-skipped steps: completed / (total - skipped) * 100
    pub percent_complete: f32,
}

impl StepProgressSummary {
    /// Calculate progress summary from a list of steps
    pub fn from_steps(task_id: &TaskId, steps: &[TaskStep]) -> Self {
        let total = steps.len() as u32;
        let completed = steps
            .iter()
            .filter(|s| s.status == TaskStepStatus::Completed)
            .count() as u32;
        let in_progress = steps
            .iter()
            .filter(|s| s.status == TaskStepStatus::InProgress)
            .count() as u32;
        let pending = steps
            .iter()
            .filter(|s| s.status == TaskStepStatus::Pending)
            .count() as u32;
        let skipped = steps
            .iter()
            .filter(|s| s.status == TaskStepStatus::Skipped)
            .count() as u32;
        let failed = steps
            .iter()
            .filter(|s| s.status == TaskStepStatus::Failed)
            .count() as u32;

        let current_step = steps
            .iter()
            .find(|s| s.status == TaskStepStatus::InProgress)
            .cloned();

        let next_step = steps
            .iter()
            .find(|s| s.status == TaskStepStatus::Pending)
            .cloned();

        let completable_total = total.saturating_sub(skipped);
        let completed_for_progress = completed.min(completable_total);
        let percent_complete = if completable_total > 0 {
            (completed_for_progress as f32 / completable_total as f32) * 100.0
        } else {
            0.0
        };

        Self {
            task_id: task_id.as_str().to_string(),
            total,
            completed,
            in_progress,
            pending,
            skipped,
            failed,
            current_step,
            next_step,
            percent_complete,
        }
    }
}

#[cfg(test)]
#[path = "task_step_tests.rs"]
mod tests;
