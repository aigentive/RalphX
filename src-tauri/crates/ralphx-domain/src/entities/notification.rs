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

/// User-configurable desktop-notification groups.
///
/// Keep [`notification_category_group`] exhaustive: adding a category requires an
/// explicit product decision about which desktop preference controls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationCategoryGroup {
    AgentRequests,
    AgentWaiting,
    Reviews,
    TaskFailures,
    AutomationApprovals,
    AutomationRunCompletions,
    GitGithub,
}

/// Maps every notification category to the desktop preference group that gates it.
pub const fn notification_category_group(
    category: NotificationCategory,
) -> NotificationCategoryGroup {
    match category {
        NotificationCategory::PermissionRequest | NotificationCategory::AgentQuestion => {
            NotificationCategoryGroup::AgentRequests
        }
        NotificationCategory::AgentWaiting | NotificationCategory::ProviderPaused => {
            NotificationCategoryGroup::AgentWaiting
        }
        NotificationCategory::ReviewNeeded
        | NotificationCategory::ReviewEscalated
        | NotificationCategory::PlanApproval => NotificationCategoryGroup::Reviews,
        NotificationCategory::QaFailed
        | NotificationCategory::MergeConflict
        | NotificationCategory::MergeIncomplete
        | NotificationCategory::TaskFailed
        | NotificationCategory::TaskBlocked
        | NotificationCategory::TaskStuck
        | NotificationCategory::RecoveryPrompt => NotificationCategoryGroup::TaskFailures,
        NotificationCategory::AutomationPlanApproval
        | NotificationCategory::AutomationPaused
        | NotificationCategory::AutomationRunFailed => {
            NotificationCategoryGroup::AutomationApprovals
        }
        NotificationCategory::AutomationRunCompleted | NotificationCategory::Info => {
            NotificationCategoryGroup::AutomationRunCompletions
        }
        NotificationCategory::GhAuth
        | NotificationCategory::GitAuthPreflight
        | NotificationCategory::PrReviewAction => NotificationCategoryGroup::GitGithub,
    }
}

/// Persisted global preferences used by desktop dispatch and focused in-app toasts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub desktop_enabled: bool,
    pub desktop_only_when_unfocused: bool,
    pub focused_toasts_enabled: bool,
    pub desktop_agent_requests_enabled: bool,
    pub desktop_agent_waiting_enabled: bool,
    pub desktop_reviews_enabled: bool,
    pub desktop_task_failures_enabled: bool,
    pub desktop_automation_approvals_enabled: bool,
    pub desktop_automation_run_completions_enabled: bool,
    pub desktop_git_github_enabled: bool,
    pub muted_project_ids: Vec<String>,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            desktop_enabled: true,
            desktop_only_when_unfocused: true,
            focused_toasts_enabled: true,
            desktop_agent_requests_enabled: true,
            desktop_agent_waiting_enabled: true,
            desktop_reviews_enabled: true,
            desktop_task_failures_enabled: true,
            desktop_automation_approvals_enabled: true,
            desktop_automation_run_completions_enabled: false,
            desktop_git_github_enabled: true,
            muted_project_ids: Vec::new(),
        }
    }
}

impl NotificationSettings {
    /// Returns whether the category's desktop preference group is enabled.
    pub const fn desktop_category_enabled(&self, category: NotificationCategory) -> bool {
        match notification_category_group(category) {
            NotificationCategoryGroup::AgentRequests => self.desktop_agent_requests_enabled,
            NotificationCategoryGroup::AgentWaiting => self.desktop_agent_waiting_enabled,
            NotificationCategoryGroup::Reviews => self.desktop_reviews_enabled,
            NotificationCategoryGroup::TaskFailures => self.desktop_task_failures_enabled,
            NotificationCategoryGroup::AutomationApprovals => {
                self.desktop_automation_approvals_enabled
            }
            NotificationCategoryGroup::AutomationRunCompletions => {
                self.desktop_automation_run_completions_enabled
            }
            NotificationCategoryGroup::GitGithub => self.desktop_git_github_enabled,
        }
    }
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

#[cfg(test)]
#[path = "notification_tests.rs"]
mod tests;
