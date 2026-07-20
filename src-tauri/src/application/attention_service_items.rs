use crate::application::automation::pause_recovery;
use crate::domain::entities::{
    AttentionItem, Automation, AutomationRun, InternalStatus, NotificationCategory,
    NotificationTarget, NotificationTargetKind, ProjectId, Task,
};
use crate::domain::state_machine::Blocker;

pub(super) fn task_attention_category(task: &Task) -> Option<NotificationCategory> {
    match task.internal_status {
        InternalStatus::ReviewPassed => Some(NotificationCategory::ReviewNeeded),
        InternalStatus::Escalated => Some(NotificationCategory::ReviewEscalated),
        InternalStatus::QaFailed => Some(NotificationCategory::QaFailed),
        InternalStatus::MergeConflict => Some(NotificationCategory::MergeConflict),
        InternalStatus::MergeIncomplete => Some(NotificationCategory::MergeIncomplete),
        InternalStatus::Failed => Some(NotificationCategory::TaskFailed),
        InternalStatus::Blocked
            if Blocker::is_human_input_reason(task.blocked_reason.as_deref()) =>
        {
            Some(NotificationCategory::TaskBlocked)
        }
        _ => None,
    }
}

pub(super) fn task_attention_item(
    task: Task,
    category: NotificationCategory,
    created_at: Option<String>,
) -> AttentionItem {
    AttentionItem {
        id: format!("task:{}:{}", task.id, category_key(category)),
        category,
        title: task.title.clone(),
        detail: task.description.clone(),
        project_id: Some(task.project_id.to_string()),
        created_at,
        target: NotificationTarget {
            kind: NotificationTargetKind::Task,
            project_id: Some(task.project_id.to_string()),
            task_id: Some(task.id.to_string()),
            conversation_id: None,
            setup_conversation_id: None,
            automation_id: None,
            run_id: None,
        },
    }
}

pub(super) fn automation_plan_approval_item(
    automation: &Automation,
    run: &AutomationRun,
) -> AttentionItem {
    AttentionItem {
        id: format!("automation-run:{}:plan-approval", run.id),
        category: NotificationCategory::AutomationPlanApproval,
        title: format!("{} — run #{}", automation.name, run.run_index),
        detail: Some("Plan awaiting approval".to_string()),
        project_id: Some(automation.project_id.to_string()),
        created_at: Some(run.updated_at.to_rfc3339()),
        target: NotificationTarget {
            kind: NotificationTargetKind::AutomationRun,
            project_id: Some(automation.project_id.to_string()),
            task_id: None,
            conversation_id: run.conversation_id.as_ref().map(ToString::to_string),
            setup_conversation_id: automation
                .setup_conversation_id
                .as_ref()
                .map(ToString::to_string),
            automation_id: Some(automation.id.to_string()),
            run_id: Some(run.id.to_string()),
        },
    }
}

pub(super) fn automation_auto_merge_attention_item(
    automation: &Automation,
    run: &AutomationRun,
) -> AttentionItem {
    AttentionItem {
        id: format!("automation-run:{}:auto-merge", run.id),
        category: NotificationCategory::AutomationRunFailed,
        title: format!("Automatic merge needs attention: {}", automation.name),
        detail: run.error_detail.clone(),
        project_id: Some(automation.project_id.to_string()),
        created_at: Some(run.updated_at.to_rfc3339()),
        target: NotificationTarget {
            kind: NotificationTargetKind::AutomationRun,
            project_id: Some(automation.project_id.to_string()),
            task_id: None,
            conversation_id: run.conversation_id.as_ref().map(ToString::to_string),
            setup_conversation_id: automation
                .setup_conversation_id
                .as_ref()
                .map(ToString::to_string),
            automation_id: Some(automation.id.to_string()),
            run_id: Some(run.id.to_string()),
        },
    }
}

pub(super) fn automation_paused_item(automation: &Automation) -> AttentionItem {
    AttentionItem {
        id: format!("automation:{}:paused", automation.id),
        category: NotificationCategory::AutomationPaused,
        title: format!("Automation paused: {}", automation.name),
        detail: automation.paused_reason_detail.clone(),
        project_id: Some(automation.project_id.to_string()),
        created_at: Some(automation.updated_at.to_rfc3339()),
        target: NotificationTarget {
            kind: NotificationTargetKind::AutomationRun,
            project_id: Some(automation.project_id.to_string()),
            task_id: None,
            conversation_id: None,
            setup_conversation_id: automation
                .setup_conversation_id
                .as_ref()
                .map(ToString::to_string),
            automation_id: Some(automation.id.to_string()),
            run_id: None,
        },
    }
}

pub(super) fn is_actionable_paused_reason(reason: &str) -> bool {
    pause_recovery::is_actionable_paused_reason(reason)
}

pub(super) fn is_in_scope(project_id: Option<&str>, project_filter: Option<&ProjectId>) -> bool {
    project_id.is_none_or(|item_project_id| {
        project_filter.is_none_or(|filter| item_project_id == filter.as_str())
    })
}

pub(super) fn attention_group(category: NotificationCategory) -> u8 {
    match category {
        NotificationCategory::PermissionRequest | NotificationCategory::AgentQuestion => 0,
        NotificationCategory::ReviewNeeded | NotificationCategory::ReviewEscalated => 1,
        NotificationCategory::QaFailed
        | NotificationCategory::MergeConflict
        | NotificationCategory::MergeIncomplete
        | NotificationCategory::TaskFailed
        | NotificationCategory::TaskBlocked => 2,
        NotificationCategory::PlanApproval
        | NotificationCategory::AutomationPlanApproval
        | NotificationCategory::AutomationPaused => 3,
        NotificationCategory::PrReviewAction => 4,
        _ => 5,
    }
}

fn category_key(category: NotificationCategory) -> &'static str {
    match category {
        NotificationCategory::ReviewNeeded => "review",
        NotificationCategory::ReviewEscalated => "escalated",
        NotificationCategory::QaFailed => "qa-failed",
        NotificationCategory::MergeConflict => "merge-conflict",
        NotificationCategory::MergeIncomplete => "merge-incomplete",
        NotificationCategory::TaskFailed => "failed",
        NotificationCategory::TaskBlocked => "blocked",
        _ => "attention",
    }
}
