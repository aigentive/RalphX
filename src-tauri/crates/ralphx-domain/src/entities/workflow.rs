// Workflow entities for custom workflow schemas
// These enable custom Kanban columns that map to internal statuses

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::status::InternalStatus;

// ============================================
// State Grouping Types (Multi-State Columns)
// ============================================

/// State group within a column
/// Allows multiple internal statuses to be grouped and displayed within a single column
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateGroup {
    /// Unique group identifier within the column
    pub id: String,
    /// Display label for the group header (e.g., "Fresh Tasks", "Needs Revision")
    pub label: String,
    /// Internal statuses that belong to this group
    pub statuses: Vec<InternalStatus>,
    /// Optional Lucide icon name for the group
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Optional accent color for the group (CSS color value)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    /// Whether tasks can be dragged FROM this group (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_drag_from: Option<bool>,
    /// Whether tasks can be dropped TO this group (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_drop_to: Option<bool>,
}

impl StateGroup {
    /// Creates a new state group with the given id, label, and statuses
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        statuses: Vec<InternalStatus>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            statuses,
            icon: None,
            accent_color: None,
            can_drag_from: None,
            can_drop_to: None,
        }
    }

    /// Sets the icon
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Sets the accent color
    pub fn with_accent_color(mut self, color: impl Into<String>) -> Self {
        self.accent_color = Some(color.into());
        self
    }

    /// Sets whether tasks can be dragged from this group
    pub fn with_can_drag_from(mut self, can_drag: bool) -> Self {
        self.can_drag_from = Some(can_drag);
        self
    }

    /// Sets whether tasks can be dropped to this group
    pub fn with_can_drop_to(mut self, can_drop: bool) -> Self {
        self.can_drop_to = Some(can_drop);
        self
    }

    /// Convenience method to lock the group (no drag, no drop)
    /// Use for system-managed states where users should not manually move tasks
    pub fn locked(self) -> Self {
        self.with_can_drag_from(false).with_can_drop_to(false)
    }
}

/// A unique identifier for a Workflow
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(pub String);

impl WorkflowId {
    /// Creates a new WorkflowId with a random UUID v4
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Creates a WorkflowId from an existing string
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Returns the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for WorkflowId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A workflow schema defines the Kanban board layout with custom columns
/// that map to internal statuses for consistent side effects
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSchema {
    /// Unique identifier
    pub id: WorkflowId,
    /// Display name for the workflow
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Ordered list of columns in the Kanban board
    pub columns: Vec<WorkflowColumn>,
    /// External sync configuration (future implementation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_sync: Option<ExternalSyncConfig>,
    /// Default agent profile overrides
    #[serde(default)]
    pub defaults: WorkflowDefaults,
    /// Whether this is the default workflow
    #[serde(default)]
    pub is_default: bool,
}

impl WorkflowSchema {
    /// Creates a new workflow schema with the given name and columns
    pub fn new(name: impl Into<String>, columns: Vec<WorkflowColumn>) -> Self {
        Self {
            id: WorkflowId::new(),
            name: name.into(),
            description: None,
            columns,
            external_sync: None,
            defaults: WorkflowDefaults::default(),
            is_default: false,
        }
    }

    /// Sets the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets this as the default workflow
    pub fn as_default(mut self) -> Self {
        self.is_default = true;
        self
    }

    /// Returns the default RalphX workflow
    /// Multi-state columns use groups to provide visibility into task state
    pub fn default_ralphx() -> Self {
        Self {
            id: WorkflowId::from_string("ralphx-default"),
            name: "RalphX Default".to_string(),
            description: Some("Default RalphX workflow".to_string()),
            columns: vec![
                WorkflowColumn::new("draft", "Draft", InternalStatus::Backlog),
                WorkflowColumn::new("ready", "Ready", InternalStatus::Ready).with_groups(vec![
                    StateGroup::new("fresh", "Fresh Tasks", vec![InternalStatus::Ready])
                        .with_can_drag_from(true)
                        .with_can_drop_to(true),
                    StateGroup::new(
                        "needs_revision",
                        "Needs Revision",
                        vec![InternalStatus::RevisionNeeded],
                    )
                    .with_icon("RotateCcw")
                    .with_accent_color("hsl(var(--warning))")
                    .with_can_drag_from(true)
                    .with_can_drop_to(false), // Only review process can add here
                    StateGroup::new("blocked", "Blocked", vec![InternalStatus::Blocked])
                        .with_icon("Ban")
                        .with_accent_color("hsl(var(--warning))")
                        .with_can_drag_from(true)
                        .with_can_drop_to(true),
                    StateGroup::new("paused", "Paused", vec![InternalStatus::Paused])
                        .with_icon("Pause")
                        .with_accent_color("hsl(var(--warning))")
                        .locked(),
                ]),
                WorkflowColumn::new("in_progress", "In Progress", InternalStatus::Executing)
                    .with_groups(vec![
                        StateGroup::new(
                            "first_attempt",
                            "First Attempt",
                            vec![InternalStatus::Executing],
                        )
                        .locked(), // System-managed (agent working)
                        StateGroup::new("revising", "Revising", vec![InternalStatus::ReExecuting])
                            .with_icon("RefreshCw")
                            .with_accent_color("hsl(var(--warning))")
                            .locked(), // System-managed (agent revising)
                    ]),
                WorkflowColumn::new("in_review", "In Review", InternalStatus::PendingReview)
                    .with_groups(vec![
                        StateGroup::new(
                            "waiting_ai",
                            "Waiting for AI",
                            vec![InternalStatus::PendingReview],
                        )
                        .with_icon("Clock")
                        .locked(), // System-managed
                        StateGroup::new(
                            "ai_reviewing",
                            "AI Reviewing",
                            vec![InternalStatus::Reviewing],
                        )
                        .with_icon("Bot")
                        .with_accent_color("hsl(var(--primary))")
                        .locked(), // System-managed (AI working)
                        StateGroup::new(
                            "ready_approval",
                            "Ready for Approval",
                            vec![InternalStatus::ReviewPassed],
                        )
                        .with_icon("CheckCircle")
                        .with_accent_color("hsl(var(--success))")
                        .locked(), // User interacts via Approve/Revise buttons
                        StateGroup::new("escalated", "Escalated", vec![InternalStatus::Escalated])
                            .with_icon("AlertTriangle")
                            .with_accent_color("hsl(var(--warning))")
                            .locked(),
                    ]),
                WorkflowColumn::new("done", "Done", InternalStatus::Approved).with_groups(vec![
                    StateGroup::new(
                        "merging",
                        "Merging",
                        vec![
                            InternalStatus::PendingMerge,
                            InternalStatus::Merging,
                            InternalStatus::WaitingOnPr,
                        ],
                    )
                    .with_icon("GitMerge")
                    .locked(),
                    StateGroup::new(
                        "needs_attention",
                        "Escalated",
                        vec![InternalStatus::MergeIncomplete, InternalStatus::MergeConflict],
                    )
                    .with_icon("AlertTriangle")
                    .with_accent_color("hsl(var(--warning))")
                    .locked(),
                    StateGroup::new(
                        "completed",
                        "Completed",
                        vec![InternalStatus::Merged, InternalStatus::Approved],
                    )
                    .with_icon("CheckCircle")
                    .with_accent_color("hsl(var(--success))")
                    .locked(),
                    StateGroup::new("cancelled", "Cancelled", vec![InternalStatus::Cancelled])
                        .with_icon("XCircle")
                        .locked(),
                    StateGroup::new("failed", "Failed", vec![InternalStatus::Failed])
                        .with_icon("XOctagon")
                        .with_accent_color("hsl(var(--destructive))")
                        .locked(),
                    StateGroup::new("stopped", "Stopped", vec![InternalStatus::Stopped])
                        .with_icon("StopCircle")
                        .with_accent_color("hsl(var(--destructive))")
                        .locked(),
                ]),
            ],
            external_sync: None,
            defaults: WorkflowDefaults::default(),
            is_default: true,
        }
    }

    /// Returns the Jira-compatible workflow
    pub fn jira_compatible() -> Self {
        Self {
            id: WorkflowId::from_string("jira-compat"),
            name: "Jira Compatible".to_string(),
            description: Some("Jira-style workflow with familiar columns".to_string()),
            columns: vec![
                WorkflowColumn::new("backlog", "Backlog", InternalStatus::Backlog),
                WorkflowColumn::new("selected", "Selected for Dev", InternalStatus::Ready),
                WorkflowColumn::new("in_progress", "In Progress", InternalStatus::Executing),
                WorkflowColumn::new("in_qa", "In QA", InternalStatus::PendingReview),
                WorkflowColumn::new("done", "Done", InternalStatus::Approved),
            ],
            external_sync: Some(ExternalSyncConfig {
                provider: SyncProvider::Jira,
                mapping: std::collections::HashMap::new(),
                sync: SyncSettings {
                    direction: SyncDirection::Bidirectional,
                    webhook: Some(true),
                },
                conflict_resolution: ConflictResolution::ExternalWins,
            }),
            defaults: WorkflowDefaults::default(),
            is_default: false,
        }
    }
}

/// A column in a workflow's Kanban board
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowColumn {
    /// Unique identifier within the workflow
    pub id: String,
    /// Display name for the column
    pub name: String,
    /// Optional color (hex format, e.g., "#ff6b35")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Optional icon name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The internal status this column maps to for side effects (primary status)
    pub maps_to: InternalStatus,
    /// Optional behavior overrides for this column
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior: Option<ColumnBehavior>,
    /// Optional state groups for multi-state columns
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<StateGroup>>,
}

impl WorkflowColumn {
    /// Creates a new workflow column
    pub fn new(id: impl Into<String>, name: impl Into<String>, maps_to: InternalStatus) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            color: None,
            icon: None,
            maps_to,
            behavior: None,
            groups: None,
        }
    }

    /// Sets the column color
    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Sets the column icon
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Sets the column behavior
    pub fn with_behavior(mut self, behavior: ColumnBehavior) -> Self {
        self.behavior = Some(behavior);
        self
    }

    /// Sets the column groups for multi-state columns
    pub fn with_groups(mut self, groups: Vec<StateGroup>) -> Self {
        self.groups = Some(groups);
        self
    }
}

/// Behavior overrides for a workflow column
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ColumnBehavior {
    /// Skip review for tasks in this column
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_review: Option<bool>,
    /// Auto-advance to next column when complete
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_advance: Option<bool>,
    /// Override agent profile for this column
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_profile: Option<String>,
}

impl ColumnBehavior {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_skip_review(mut self, skip: bool) -> Self {
        self.skip_review = Some(skip);
        self
    }

    pub fn with_auto_advance(mut self, advance: bool) -> Self {
        self.auto_advance = Some(advance);
        self
    }

    pub fn with_agent_profile(mut self, profile: impl Into<String>) -> Self {
        self.agent_profile = Some(profile.into());
        self
    }
}

/// Default agent profile configuration for a workflow
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WorkflowDefaults {
    /// Default worker agent profile
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_profile: Option<String>,
    /// Default reviewer agent profile
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewer_profile: Option<String>,
}

/// External sync configuration (placeholder for future implementation)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalSyncConfig {
    /// The external provider
    pub provider: SyncProvider,
    /// Status mapping from external to internal
    #[serde(default)]
    pub mapping: std::collections::HashMap<String, ExternalStatusMapping>,
    /// Sync settings
    pub sync: SyncSettings,
    /// How to resolve conflicts
    pub conflict_resolution: ConflictResolution,
}

/// Supported external sync providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncProvider {
    Jira,
    Github,
    Linear,
    Notion,
}

impl fmt::Display for SyncProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncProvider::Jira => write!(f, "jira"),
            SyncProvider::Github => write!(f, "github"),
            SyncProvider::Linear => write!(f, "linear"),
            SyncProvider::Notion => write!(f, "notion"),
        }
    }
}

/// Mapping from an external status to internal status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalStatusMapping {
    /// The external status name
    pub external_status: String,
    /// The internal status to map to
    pub internal_status: InternalStatus,
    /// The workflow column to display in
    pub column_id: String,
}

/// Sync direction settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncSettings {
    /// Sync direction
    pub direction: SyncDirection,
    /// Enable webhook for real-time sync
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook: Option<bool>,
}

/// Sync direction options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncDirection {
    Pull,
    Push,
    Bidirectional,
}

impl fmt::Display for SyncDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncDirection::Pull => write!(f, "pull"),
            SyncDirection::Push => write!(f, "push"),
            SyncDirection::Bidirectional => write!(f, "bidirectional"),
        }
    }
}

/// Error for parsing SyncDirection from string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSyncDirectionError {
    pub value: String,
}

impl fmt::Display for ParseSyncDirectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown sync direction: '{}'", self.value)
    }
}

impl std::error::Error for ParseSyncDirectionError {}

impl FromStr for SyncDirection {
    type Err = ParseSyncDirectionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pull" => Ok(SyncDirection::Pull),
            "push" => Ok(SyncDirection::Push),
            "bidirectional" => Ok(SyncDirection::Bidirectional),
            _ => Err(ParseSyncDirectionError {
                value: s.to_string(),
            }),
        }
    }
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    ExternalWins,
    InternalWins,
    Manual,
}

impl fmt::Display for ConflictResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConflictResolution::ExternalWins => write!(f, "external_wins"),
            ConflictResolution::InternalWins => write!(f, "internal_wins"),
            ConflictResolution::Manual => write!(f, "manual"),
        }
    }
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
