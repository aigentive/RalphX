use crate::application::notification_context_resolver::ResolvedNotificationTarget;
use crate::application::permission_state::{PendingPermissionInfo, PERMISSION_REQUEST_TTL};
use crate::domain::entities::ChatContextType;
use crate::domain::entities::{
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget,
};

const QUESTION_BODY_LIMIT: usize = 240;

pub fn permission_notification_key(request_id: &str) -> String {
    format!("perm:{request_id}")
}

pub fn question_notification_key(request_id: &str) -> String {
    format!("question:{request_id}")
}

pub fn plan_notification_key(session_id: &str, artifact_id: &str) -> String {
    format!("plan:{session_id}:{artifact_id}")
}

pub fn pr_review_notification_key(conversation_id: impl AsRef<str>, action_id: &str) -> String {
    format!(
        "pr-review:{}:awaiting_user:{action_id}",
        conversation_id.as_ref()
    )
}

pub fn automation_plan_notification_key(run_id: &str) -> String {
    format!("run:{run_id}:plan_approval")
}

/// Builds the consistent user-facing copy for interactive notification producers.
pub struct InteractiveNotificationProducer;

impl InteractiveNotificationProducer {
    pub fn agent_waiting(
        project_id: Option<String>,
        conversation_id: &str,
        title: Option<&str>,
    ) -> NewNotification {
        let title = title.unwrap_or("this conversation");
        NewNotification {
            project_id: project_id.clone(),
            category: NotificationCategory::AgentWaiting,
            severity: NotificationSeverity::Info,
            title: "Your turn".to_string(),
            body: Some(format!(
                "Agent finished on “{title}” and is waiting for you"
            )),
            target: NotificationTarget {
                kind: crate::domain::entities::NotificationTargetKind::AgentConversation,
                project_id,
                task_id: None,
                conversation_id: Some(conversation_id.to_string()),
                setup_conversation_id: None,
                automation_id: None,
                run_id: None,
            },
            dedupe_key: None,
        }
    }

    pub fn permission_request(
        request: &PendingPermissionInfo,
        resolved: ResolvedNotificationTarget,
    ) -> NewNotification {
        let actor = request.agent_type.as_deref().unwrap_or("Agent");
        let location = resolved
            .context_label
            .or_else(|| request.context.clone())
            .map(|context| format!(" on “{context}”"))
            .unwrap_or_default();
        NewNotification {
            project_id: resolved.project_id,
            category: NotificationCategory::PermissionRequest,
            severity: NotificationSeverity::ActionRequired,
            title: "Permission needed".to_string(),
            body: Some(format!(
                "{actor} wants to run {}{location} — expires in {}m",
                request.tool_name,
                PERMISSION_REQUEST_TTL.as_secs() / 60,
            )),
            target: resolved.target,
            dedupe_key: Some(permission_notification_key(&request.request_id)),
        }
    }

    pub fn agent_question(
        request_id: &str,
        question: &str,
        resolved: ResolvedNotificationTarget,
    ) -> NewNotification {
        let body = agent_question_body(&resolved, truncate_question(question));
        NewNotification {
            project_id: resolved.project_id,
            category: NotificationCategory::AgentQuestion,
            severity: NotificationSeverity::ActionRequired,
            title: "Agent has a question".to_string(),
            body: Some(body),
            target: resolved.target,
            dedupe_key: Some(question_notification_key(request_id)),
        }
    }

    pub fn plan_approval(
        project_id: String,
        session_id: &str,
        artifact_id: &str,
        session_title: Option<&str>,
        target: NotificationTarget,
    ) -> NewNotification {
        let subject = session_title.unwrap_or("Workspace plan");
        NewNotification {
            project_id: Some(project_id),
            category: NotificationCategory::PlanApproval,
            severity: NotificationSeverity::ActionRequired,
            title: "Plan approval needed".to_string(),
            body: Some(format!("“{subject}” is ready for review")),
            target,
            dedupe_key: Some(plan_notification_key(session_id, artifact_id)),
        }
    }
}

fn agent_question_body(resolved: &ResolvedNotificationTarget, question: String) -> String {
    let quoted_question = format!("“{question}”");
    match (
        resolved
            .context_kind
            .as_ref()
            .and_then(question_context_kind),
        resolved.project_name.as_deref(),
    ) {
        (Some(kind), Some(project_name)) => format!("{kind} on {project_name}: {quoted_question}"),
        _ => quoted_question,
    }
}

fn question_context_kind(context_type: &ChatContextType) -> Option<&'static str> {
    match context_type {
        ChatContextType::Ideation => Some("ideation"),
        ChatContextType::Project => Some("project"),
        ChatContextType::Standalone => Some("standalone"),
        ChatContextType::Task
        | ChatContextType::TaskExecution
        | ChatContextType::Review
        | ChatContextType::Merge
        | ChatContextType::BranchUpdate => Some("task"),
        ChatContextType::Delegation => None,
    }
}

fn truncate_question(question: &str) -> String {
    let mut chars = question.chars();
    let truncated: String = chars.by_ref().take(QUESTION_BODY_LIMIT).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}
