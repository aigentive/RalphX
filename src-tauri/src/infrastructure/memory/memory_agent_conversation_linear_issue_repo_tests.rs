use chrono::{DateTime, Duration, Utc};

use super::*;
use crate::domain::entities::{
    AgentConversationLinearRefreshStatus, ChatConversationId, ProjectId,
};

fn assigned_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-18T18:14:05Z")
        .expect("timestamp")
        .with_timezone(&Utc)
}

fn link(
    conversation_id: ChatConversationId,
    issue_id: &str,
    assigned_at: DateTime<Utc>,
) -> AgentConversationLinearIssueLink {
    AgentConversationLinearIssueLink::new(
        conversation_id,
        ProjectId::from_string("project-1".to_string()),
        issue_id.to_string(),
        assigned_at,
    )
}

#[tokio::test]
async fn upsert_preserves_created_at_while_replacing_assignment_snapshot() {
    let repo = MemoryAgentConversationLinearIssueRepository::new();
    let conversation_id = ChatConversationId::new();
    let first = link(
        conversation_id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f",
        assigned_at(),
    );
    let first_created_at = first.created_at;
    repo.upsert(first).await.expect("insert first");

    let mut replacement = link(
        conversation_id,
        "639068e2-ae88-4d09-bd75-22eb4a59612f",
        assigned_at() + Duration::minutes(5),
    );
    replacement.status = Some("In Review".to_string());
    replacement.refresh_status = AgentConversationLinearRefreshStatus::Loaded;

    let saved = repo.upsert(replacement).await.expect("replace link");
    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load link")
        .expect("stored link");

    assert_eq!(saved.issue_id, "639068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(saved.created_at, first_created_at);
    assert_eq!(loaded.issue_id, "639068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(loaded.status.as_deref(), Some("In Review"));
    assert_eq!(
        loaded.refresh_status,
        AgentConversationLinearRefreshStatus::Loaded
    );
    assert_eq!(loaded.created_at, first_created_at);
}

#[tokio::test]
async fn insert_if_absent_returns_existing_assignment_without_overwriting() {
    let repo = MemoryAgentConversationLinearIssueRepository::new();
    let conversation_id = ChatConversationId::new();
    let existing = link(
        conversation_id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f",
        assigned_at(),
    );
    repo.upsert(existing).await.expect("insert first");

    let returned = repo
        .insert_if_absent(link(
            conversation_id,
            "639068e2-ae88-4d09-bd75-22eb4a59612f",
            assigned_at() + Duration::minutes(5),
        ))
        .await
        .expect("insert if absent");
    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load link")
        .expect("stored link");

    assert_eq!(returned.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(loaded.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
}

#[tokio::test]
async fn clear_removes_existing_assignment_and_allows_new_assignment() {
    let repo = MemoryAgentConversationLinearIssueRepository::new();
    let conversation_id = ChatConversationId::new();
    repo.upsert(link(
        conversation_id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f",
        assigned_at(),
    ))
    .await
    .expect("insert first");

    repo.clear(&conversation_id).await.expect("clear link");
    assert!(repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load after clear")
        .is_none());

    let inserted = repo
        .insert_if_absent(link(
            conversation_id,
            "639068e2-ae88-4d09-bd75-22eb4a59612f",
            assigned_at() + Duration::minutes(5),
        ))
        .await
        .expect("insert after clear");
    assert_eq!(inserted.issue_id, "639068e2-ae88-4d09-bd75-22eb4a59612f");
}
