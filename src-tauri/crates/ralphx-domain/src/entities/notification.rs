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
