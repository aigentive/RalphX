use super::{
    notification_category_group, NotificationCategory, NotificationCategoryGroup,
    NotificationSettings,
};

#[test]
fn notification_settings_defaults_match_product_policy() {
    let settings = NotificationSettings::default();

    assert!(settings.desktop_enabled);
    assert!(settings.desktop_only_when_unfocused);
    assert!(settings.focused_toasts_enabled);
    assert!(settings.desktop_agent_requests_enabled);
    assert!(settings.desktop_agent_waiting_enabled);
    assert!(settings.desktop_reviews_enabled);
    assert!(settings.desktop_task_failures_enabled);
    assert!(settings.desktop_automation_approvals_enabled);
    assert!(!settings.desktop_automation_run_completions_enabled);
    assert!(settings.desktop_git_github_enabled);
}

#[test]
fn notification_category_groups_cover_the_product_categories() {
    assert_eq!(
        notification_category_group(NotificationCategory::PermissionRequest),
        NotificationCategoryGroup::AgentRequests
    );
    assert_eq!(
        notification_category_group(NotificationCategory::PlanApproval),
        NotificationCategoryGroup::Reviews
    );
    assert_eq!(
        notification_category_group(NotificationCategory::MergeIncomplete),
        NotificationCategoryGroup::TaskFailures
    );
    assert_eq!(
        notification_category_group(NotificationCategory::AutomationRunCompleted),
        NotificationCategoryGroup::AutomationRunCompletions
    );
    assert_eq!(
        notification_category_group(NotificationCategory::PrReviewAction),
        NotificationCategoryGroup::GitGithub
    );
}

#[test]
fn notification_category_group_mapping_is_exhaustive() {
    let categories = [
        NotificationCategory::ReviewNeeded,
        NotificationCategory::ReviewEscalated,
        NotificationCategory::QaFailed,
        NotificationCategory::MergeConflict,
        NotificationCategory::MergeIncomplete,
        NotificationCategory::TaskFailed,
        NotificationCategory::TaskBlocked,
        NotificationCategory::TaskStuck,
        NotificationCategory::ProviderPaused,
        NotificationCategory::RecoveryPrompt,
        NotificationCategory::PermissionRequest,
        NotificationCategory::AgentQuestion,
        NotificationCategory::PlanApproval,
        NotificationCategory::AutomationPlanApproval,
        NotificationCategory::AutomationPaused,
        NotificationCategory::AutomationRunFailed,
        NotificationCategory::AutomationRunCompleted,
        NotificationCategory::AgentWaiting,
        NotificationCategory::GhAuth,
        NotificationCategory::GitAuthPreflight,
        NotificationCategory::PrReviewAction,
        NotificationCategory::Info,
    ];

    assert_eq!(categories.len(), 22);
    for category in categories {
        let _ = notification_category_group(category);
    }
}
