use super::interactive_notification_producer::InteractiveNotificationProducer;
use super::notification_context_resolver::ResolvedNotificationTarget;
use crate::application::PendingPermissionInfo;
use crate::domain::entities::{
    NotificationCategory, NotificationSeverity, NotificationTarget, NotificationTargetKind,
};

fn resolved_target(label: Option<&str>) -> ResolvedNotificationTarget {
    ResolvedNotificationTarget {
        project_id: Some("project-1".to_string()),
        target: NotificationTarget {
            kind: NotificationTargetKind::AgentConversation,
            project_id: Some("project-1".to_string()),
            task_id: None,
            conversation_id: Some("conversation-1".to_string()),
            setup_conversation_id: None,
            automation_id: None,
            run_id: None,
        },
        context_label: label.map(str::to_string),
    }
}

#[test]
fn agent_waiting_uses_conversation_title_or_safe_fallback() {
    let named = InteractiveNotificationProducer::agent_waiting(
        Some("project-1".to_string()),
        "conversation-1",
        Some("Implement notifications"),
    );
    let fallback = InteractiveNotificationProducer::agent_waiting(None, "conversation-2", None);

    assert_eq!(named.category, NotificationCategory::AgentWaiting);
    assert_eq!(named.severity, NotificationSeverity::Info);
    assert_eq!(
        named.body.as_deref(),
        Some("Agent finished on “Implement notifications” and is waiting for you")
    );
    assert_eq!(named.target.kind, NotificationTargetKind::AgentConversation);
    assert_eq!(
        named.target.conversation_id.as_deref(),
        Some("conversation-1")
    );
    assert_eq!(
        fallback.body.as_deref(),
        Some("Agent finished on “this conversation” and is waiting for you")
    );
    assert!(fallback.project_id.is_none());
}

#[test]
fn permission_request_prefers_resolved_context_and_falls_back_to_request_context() {
    let request = PendingPermissionInfo {
        request_id: "permission-1".to_string(),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({ "command": "git status" }),
        context: Some("fallback context".to_string()),
        agent_type: Some("Planner".to_string()),
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-11T00:00:00Z".to_string(),
    };
    let resolved = InteractiveNotificationProducer::permission_request(
        &request,
        resolved_target(Some("Resolved task")),
    );
    let fallback =
        InteractiveNotificationProducer::permission_request(&request, resolved_target(None));

    assert_eq!(resolved.category, NotificationCategory::PermissionRequest);
    assert_eq!(resolved.severity, NotificationSeverity::ActionRequired);
    assert_eq!(
        resolved.body.as_deref(),
        Some("Planner wants to run Bash on “Resolved task”")
    );
    assert_eq!(
        fallback.body.as_deref(),
        Some("Planner wants to run Bash on “fallback context”")
    );
    assert_eq!(resolved.dedupe_key.as_deref(), Some("perm:permission-1"));
}

#[test]
fn agent_question_preserves_short_copy_and_truncates_long_copy_at_char_boundary() {
    let short = InteractiveNotificationProducer::agent_question(
        "question-short",
        "Which deployment window works?",
        resolved_target(None),
    );
    let long_question = format!("{}é", "x".repeat(240));
    let long = InteractiveNotificationProducer::agent_question(
        "question-long",
        &long_question,
        resolved_target(None),
    );

    assert_eq!(
        short.body.as_deref(),
        Some("Which deployment window works?")
    );
    assert_eq!(long.body, Some(format!("{}…", "x".repeat(240))));
    assert_eq!(long.dedupe_key.as_deref(), Some("question:question-long"));
}

#[test]
fn plan_approval_producers_build_reviewable_copy_and_stable_dedupe_keys() {
    let target = NotificationTarget::none();
    let plan = InteractiveNotificationProducer::plan_approval(
        "project-1".to_string(),
        "session-1",
        "artifact-1",
        None,
        target.clone(),
    );
    let team = InteractiveNotificationProducer::team_plan_approval(
        "team-plan-1",
        "verification",
        resolved_target(Some("ignored")),
    );

    assert_eq!(plan.category, NotificationCategory::PlanApproval);
    assert_eq!(
        plan.body.as_deref(),
        Some("“Workspace plan” is ready for review")
    );
    assert_eq!(
        plan.dedupe_key.as_deref(),
        Some("plan:session-1:artifact-1")
    );
    assert_eq!(team.category, NotificationCategory::TeamPlanApproval);
    assert_eq!(
        team.body.as_deref(),
        Some("verification team plan is awaiting approval")
    );
    assert_eq!(team.dedupe_key.as_deref(), Some("team-plan:team-plan-1"));
}
