use crate::application::notification_context_resolver::ResolvedNotificationTarget;
use crate::application::permission_state::PendingPermissionInfo;
use crate::domain::entities::{
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget,
};

const QUESTION_BODY_LIMIT: usize = 240;

/// Builds the consistent user-facing copy for interactive notification producers.
pub struct InteractiveNotificationProducer;

impl InteractiveNotificationProducer {
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
                "{actor} wants to run {}{location}",
                request.tool_name
            )),
            target: resolved.target,
            dedupe_key: Some(format!("perm:{}", request.request_id)),
        }
    }

    pub fn agent_question(
        request_id: &str,
        question: &str,
        resolved: ResolvedNotificationTarget,
    ) -> NewNotification {
        NewNotification {
            project_id: resolved.project_id,
            category: NotificationCategory::AgentQuestion,
            severity: NotificationSeverity::ActionRequired,
            title: "Agent has a question".to_string(),
            body: Some(truncate_question(question)),
            target: resolved.target,
            dedupe_key: Some(format!("question:{request_id}")),
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
            dedupe_key: Some(format!("plan:{session_id}:{artifact_id}")),
        }
    }

    pub fn team_plan_approval(
        plan_id: &str,
        process: &str,
        resolved: ResolvedNotificationTarget,
    ) -> NewNotification {
        NewNotification {
            project_id: resolved.project_id,
            category: NotificationCategory::TeamPlanApproval,
            severity: NotificationSeverity::ActionRequired,
            title: "Team plan approval needed".to_string(),
            body: Some(format!("{process} team plan is awaiting approval")),
            target: resolved.target,
            dedupe_key: Some(format!("team-plan:{plan_id}")),
        }
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
