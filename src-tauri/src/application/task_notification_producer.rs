use std::sync::Arc;

use async_trait::async_trait;

use crate::application::notification_service::NotificationService;
use crate::domain::entities::{
    InternalStatus, NewNotification, NotificationCategory, NotificationSeverity,
    NotificationTarget, NotificationTargetKind, Task,
};
use crate::domain::state_machine::{
    services::{NotificationContext, Notifier, TaskNotification},
    Blocker,
};

/// Application implementation of the domain notification seam.
///
/// All copy and task-status mapping live here so producers do not scatter user-facing strings.
///
/// Direct merge/freshness paths notify with the same non-CAS authority their status writes use.
/// A concurrent-overwrite stale alert is bounded by Tier 1, which re-derives the attention list
/// from live state; CAS-ifying those pre-existing writes is tracked outside this change.
///
/// If a history write fails, the durable notification is intentionally skipped as a fail-closed
/// effect. Tier 1 still surfaces the live state, and no fabricated dedupe key is emitted.
pub struct TaskPipelineNotificationProducer {
    service: Arc<NotificationService>,
}

impl TaskPipelineNotificationProducer {
    pub fn new(service: Arc<NotificationService>) -> Self {
        Self { service }
    }

    fn state_notification(
        context: &NotificationContext,
        status: InternalStatus,
    ) -> Option<NewNotification> {
        let (category, severity, title, body) = match status {
            InternalStatus::ReviewPassed => (
                NotificationCategory::ReviewNeeded,
                NotificationSeverity::ActionRequired,
                "Review ready",
                format!("AI approved “{}” — confirm to merge", context.task.title),
            ),
            InternalStatus::Escalated => (
                NotificationCategory::ReviewEscalated,
                NotificationSeverity::ActionRequired,
                "Review escalated",
                format!("AI couldn’t decide on “{}”", context.task.title),
            ),
            InternalStatus::QaFailed => (
                NotificationCategory::QaFailed,
                NotificationSeverity::ActionRequired,
                "QA failed",
                format!("QA checks failed for “{}”", context.task.title),
            ),
            InternalStatus::MergeConflict => (
                NotificationCategory::MergeConflict,
                NotificationSeverity::ActionRequired,
                "Merge conflict",
                format!("“{}” needs manual conflict resolution", context.task.title),
            ),
            InternalStatus::MergeIncomplete => (
                NotificationCategory::MergeIncomplete,
                NotificationSeverity::ActionRequired,
                "Merge incomplete",
                format!(
                    "“{}” could not be merged — open the task to continue",
                    context.task.title
                ),
            ),
            InternalStatus::Blocked
                if Blocker::is_human_input_reason(context.task.blocked_reason.as_deref()) =>
            {
                (
                    NotificationCategory::TaskBlocked,
                    NotificationSeverity::ActionRequired,
                    "Task blocked",
                    format!("“{}” is waiting for your input", context.task.title),
                )
            }
            InternalStatus::Blocked
                if Blocker::is_freshness_blocked_reason(context.task.blocked_reason.as_deref()) =>
            {
                (
                    NotificationCategory::TaskBlocked,
                    NotificationSeverity::Warning,
                    "Branch freshness blocked",
                    format!(
                        "“{}” needs its branch conflicts resolved before it can continue",
                        context.task.title
                    ),
                )
            }
            InternalStatus::Failed => (
                NotificationCategory::TaskFailed,
                NotificationSeverity::ActionRequired,
                "Task failed",
                format!(
                    "“{}” failed: {} — retry from the app",
                    context.task.title,
                    context
                        .task
                        .blocked_reason
                        .as_deref()
                        .unwrap_or("agent error")
                ),
            ),
            _ => return None,
        };

        Some(NewNotification {
            project_id: Some(context.project_id.to_string()),
            category,
            severity,
            title: title.to_string(),
            body: Some(body),
            target: NotificationTarget {
                kind: NotificationTargetKind::Task,
                project_id: Some(context.project_id.to_string()),
                task_id: Some(context.task.id.to_string()),
                conversation_id: None,
                setup_conversation_id: None,
                automation_id: None,
                run_id: None,
            },
            dedupe_key: Some(format!(
                "task:{}:{}:{}",
                context.task.id,
                status.as_str(),
                context.history_entry_id
            )),
        })
    }

    pub(crate) fn provider_paused_notification(paused_at: &str, category: &str) -> NewNotification {
        NewNotification {
            project_id: None,
            category: NotificationCategory::ProviderPaused,
            severity: NotificationSeverity::Warning,
            title: "Agents paused".to_string(),
            body: Some(format!(
                "{} reached — queue paused, auto-resumes",
                provider_pause_label(category)
            )),
            target: NotificationTarget::none(),
            dedupe_key: Some(format!("provider:{category}:paused:{paused_at}")),
        }
    }

    pub(crate) fn recovery_prompt_notification(
        task: &Task,
        status: InternalStatus,
        context_type: &str,
        reason: &str,
        instance_id: &str,
    ) -> NewNotification {
        Self::task_notification(
            task,
            NotificationCategory::RecoveryPrompt,
            NotificationSeverity::ActionRequired,
            "Recovery needs your decision",
            format!(
                "“{}” is stuck in {} ({context_type}): {reason}",
                task.title,
                status.as_str()
            ),
            format!("task:{}:recovery_prompt:{instance_id}", task.id),
        )
    }

    pub(crate) fn task_stuck_notification(
        task: &Task,
        instance_id: &str,
        message: impl Into<String>,
    ) -> NewNotification {
        Self::task_notification(
            task,
            NotificationCategory::TaskStuck,
            NotificationSeverity::Warning,
            "Task needs attention",
            format!(
                "Recovery failed on “{}” — task may be stuck. {}",
                task.title,
                message.into()
            ),
            format!("task:{}:stuck:{instance_id}", task.id),
        )
    }

    fn task_notification(
        task: &Task,
        category: NotificationCategory,
        severity: NotificationSeverity,
        title: &str,
        body: String,
        dedupe_key: String,
    ) -> NewNotification {
        NewNotification {
            project_id: Some(task.project_id.to_string()),
            category,
            severity,
            title: title.to_string(),
            body: Some(body),
            target: NotificationTarget {
                kind: NotificationTargetKind::Task,
                project_id: Some(task.project_id.to_string()),
                task_id: Some(task.id.to_string()),
                conversation_id: None,
                setup_conversation_id: None,
                automation_id: None,
                run_id: None,
            },
            dedupe_key: Some(dedupe_key),
        }
    }
}

fn provider_pause_label(category: &str) -> String {
    let mut label = category.replace('_', " ");
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    label
}

#[async_trait]
impl Notifier for TaskPipelineNotificationProducer {
    async fn notify(&self, context: NotificationContext, notification: TaskNotification) {
        let input = match notification {
            TaskNotification::StateEntered(status) => Self::state_notification(&context, status),
            // A review-start failure is advisory, not a terminal workflow failure. Keep it a
            // warning task-stuck entry, scoped to the transition attempt that started review.
            TaskNotification::ReviewError { message } => Some(NewNotification {
                project_id: Some(context.project_id.to_string()),
                category: NotificationCategory::TaskStuck,
                severity: NotificationSeverity::Warning,
                title: "Review could not start".to_string(),
                body: Some(format!("“{}”: {}", context.task.title, message)),
                target: NotificationTarget {
                    kind: NotificationTargetKind::Task,
                    project_id: Some(context.project_id.to_string()),
                    task_id: Some(context.task.id.to_string()),
                    conversation_id: None,
                    setup_conversation_id: None,
                    automation_id: None,
                    run_id: None,
                },
                dedupe_key: Some(format!(
                    "task:{}:review_error:{}",
                    context.task.id, context.history_entry_id
                )),
            }),
            TaskNotification::TaskStuck { message } => Some(Self::task_stuck_notification(
                &context.task,
                &context.history_entry_id,
                message,
            )),
        };

        if let Some(input) = input {
            self.service.record(input).await;
        }
    }
}
