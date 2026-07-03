use std::str::FromStr;

use chrono::Utc;

use crate::entities::{
    AgentConversationGranolaNoteLink, AgentConversationGranolaRefreshStatus, ChatConversationId,
    ProjectId,
};

#[test]
fn granola_refresh_status_round_trips_snake_case() {
    assert_eq!(
        AgentConversationGranolaRefreshStatus::NotLoaded.to_string(),
        "not_loaded"
    );
    assert_eq!(
        AgentConversationGranolaRefreshStatus::Loaded.to_string(),
        "loaded"
    );
    assert_eq!(
        AgentConversationGranolaRefreshStatus::Error.to_string(),
        "error"
    );
    assert_eq!(
        AgentConversationGranolaRefreshStatus::from_str("not_loaded").unwrap(),
        AgentConversationGranolaRefreshStatus::NotLoaded
    );
    assert!(AgentConversationGranolaRefreshStatus::from_str("stale").is_err());
}

#[test]
fn granola_note_link_defaults_to_unloaded_primary_note() {
    let assigned_at = Utc::now();
    let link = AgentConversationGranolaNoteLink::new(
        ChatConversationId::from_string("conversation-1".to_string()),
        ProjectId::from_string("project-1".to_string()),
        "not_1234567890ABCD".to_string(),
        assigned_at,
    );

    assert_eq!(link.provider, "granola");
    assert_eq!(link.note_id, "not_1234567890ABCD");
    assert!(link.include_transcript);
    assert_eq!(link.transcript_json, "[]");
    assert_eq!(
        link.refresh_status,
        AgentConversationGranolaRefreshStatus::NotLoaded
    );
    assert_eq!(link.assigned_at, assigned_at);
    assert_eq!(link.created_at, assigned_at);
    assert_eq!(link.updated_at, assigned_at);
}
