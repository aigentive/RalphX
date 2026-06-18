use std::str::FromStr;

use chrono::{DateTime, Utc};

use super::*;

fn assigned_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-17T12:00:00Z")
        .expect("timestamp")
        .with_timezone(&Utc)
}

#[test]
fn refresh_status_round_trips_display_values_and_rejects_unknown_values() {
    assert_eq!(
        AgentConversationJiraRefreshStatus::NotLoaded.to_string(),
        "not_loaded"
    );
    assert_eq!(
        AgentConversationJiraRefreshStatus::Loaded.to_string(),
        "loaded"
    );
    assert_eq!(
        AgentConversationJiraRefreshStatus::Error.to_string(),
        "error"
    );
    assert_eq!(
        AgentConversationJiraRefreshStatus::from_str("not_loaded").unwrap(),
        AgentConversationJiraRefreshStatus::NotLoaded
    );
    assert_eq!(
        AgentConversationJiraRefreshStatus::from_str("loaded").unwrap(),
        AgentConversationJiraRefreshStatus::Loaded
    );
    assert_eq!(
        AgentConversationJiraRefreshStatus::from_str("error").unwrap(),
        AgentConversationJiraRefreshStatus::Error
    );
    assert_eq!(
        AgentConversationJiraRefreshStatus::from_str("stale").unwrap_err(),
        "unknown Jira refresh status: 'stale'"
    );
}

#[test]
fn new_link_defaults_to_unloaded_atlassian_assignment_snapshot() {
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());
    let link = AgentConversationJiraIssueLink::new(
        conversation_id,
        project_id.clone(),
        "RX-42".to_string(),
        assigned_at(),
    );

    assert_eq!(link.conversation_id, conversation_id);
    assert_eq!(link.project_id, project_id);
    assert_eq!(link.provider, "atlassian");
    assert_eq!(link.issue_key, "RX-42");
    assert_eq!(link.comments_json, "[]");
    assert_eq!(link.attachments_json, "[]");
    assert_eq!(
        link.refresh_status,
        AgentConversationJiraRefreshStatus::NotLoaded
    );
    assert_eq!(link.assigned_at, assigned_at());
    assert_eq!(link.created_at, assigned_at());
    assert_eq!(link.updated_at, assigned_at());
    assert!(!link.manually_assigned);
}
