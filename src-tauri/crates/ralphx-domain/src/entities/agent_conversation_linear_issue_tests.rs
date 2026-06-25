use std::str::FromStr;

use chrono::{DateTime, Utc};

use super::*;

fn assigned_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-18T18:14:05Z")
        .expect("timestamp")
        .with_timezone(&Utc)
}

#[test]
fn refresh_status_round_trips_display_values_and_rejects_unknown_values() {
    assert_eq!(
        AgentConversationLinearRefreshStatus::NotLoaded.to_string(),
        "not_loaded"
    );
    assert_eq!(
        AgentConversationLinearRefreshStatus::Loaded.to_string(),
        "loaded"
    );
    assert_eq!(
        AgentConversationLinearRefreshStatus::Error.to_string(),
        "error"
    );
    assert_eq!(
        AgentConversationLinearRefreshStatus::from_str("not_loaded").unwrap(),
        AgentConversationLinearRefreshStatus::NotLoaded
    );
    assert_eq!(
        AgentConversationLinearRefreshStatus::from_str("loaded").unwrap(),
        AgentConversationLinearRefreshStatus::Loaded
    );
    assert_eq!(
        AgentConversationLinearRefreshStatus::from_str("error").unwrap(),
        AgentConversationLinearRefreshStatus::Error
    );
    assert_eq!(
        AgentConversationLinearRefreshStatus::from_str("stale").unwrap_err(),
        "unknown Linear refresh status: 'stale'"
    );
}

#[test]
fn new_link_defaults_to_unloaded_linear_assignment_snapshot() {
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());
    let link = AgentConversationLinearIssueLink::new(
        conversation_id,
        project_id.clone(),
        "539068e2-ae88-4d09-bd75-22eb4a59612f".to_string(),
        assigned_at(),
    );

    assert_eq!(link.conversation_id, conversation_id);
    assert_eq!(link.project_id, project_id);
    assert_eq!(link.provider, "linear");
    assert_eq!(link.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(link.issue_key, None);
    assert_eq!(link.comments_json, "[]");
    assert_eq!(link.attachments_json, "[]");
    assert_eq!(
        link.refresh_status,
        AgentConversationLinearRefreshStatus::NotLoaded
    );
    assert_eq!(link.assigned_at, assigned_at());
    assert_eq!(link.created_at, assigned_at());
    assert_eq!(link.updated_at, assigned_at());
    assert!(!link.manually_assigned);
}
