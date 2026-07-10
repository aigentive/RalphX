use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Categories shared by live attention items and the durable notification log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationCategory {
    ReviewNeeded,
    ReviewEscalated,
    QaFailed,
    MergeConflict,
    MergeIncomplete,
    TaskFailed,
    TaskBlocked,
    TaskStuck,
    ProviderPaused,
    RecoveryPrompt,
    PermissionRequest,
    AgentQuestion,
    PlanApproval,
    TeamPlanApproval,
    AutomationPlanApproval,
    AutomationPaused,
    AutomationRunFailed,
    AutomationRunCompleted,
    AgentWaiting,
    GhAuth,
    GitAuthPreflight,
    PrReviewAction,
    Info,
}

/// Urgency for a durable notification-log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    ActionRequired,
    Warning,
    Info,
}

/// The frontend navigation surface that owns an attention item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationTargetKind {
    Task,
    AgentConversation,
    AutomationRun,
    Project,
    None,
}

/// Typed navigation payload returned with an attention item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationTarget {
    pub kind: NotificationTargetKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

impl NotificationTarget {
    pub fn none() -> Self {
        Self {
            kind: NotificationTargetKind::None,
            project_id: None,
            task_id: None,
            conversation_id: None,
            setup_conversation_id: None,
            automation_id: None,
            run_id: None,
        }
    }
}

/// A currently human-actionable item derived from authoritative application state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionItem {
    pub id: String,
    pub category: NotificationCategory,
    pub title: String,
    pub detail: Option<String>,
    pub project_id: Option<String>,
    pub created_at: Option<String>,
    pub target: NotificationTarget,
}

/// A durable point-in-time notification. Notification rows are read/unread history, not live
/// workflow state; use [`AttentionItem`] for currently actionable work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub project_id: Option<String>,
    pub category: NotificationCategory,
    pub severity: NotificationSeverity,
    pub title: String,
    pub body: Option<String>,
    pub target: NotificationTarget,
    pub dedupe_key: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
}

/// Input for recording a durable notification. The service assigns the id and timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotification {
    pub project_id: Option<String>,
    pub category: NotificationCategory,
    pub severity: NotificationSeverity,
    pub title: String,
    pub body: Option<String>,
    pub target: NotificationTarget,
    pub dedupe_key: Option<String>,
}

impl NewNotification {
    pub fn into_notification(self, now: DateTime<Utc>) -> Notification {
        Notification {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: now,
            project_id: self.project_id,
            category: self.category,
            severity: self.severity,
            title: self.title,
            body: self.body,
            target: self.target,
            dedupe_key: self.dedupe_key,
            read_at: None,
        }
    }
}
