use chrono::Utc;

use crate::domain::entities::{
    AgentConversationLinearIssueLink, AgentConversationLinearRefreshStatus, ChatConversationId,
    ChatMessageId, ProjectId,
};
use crate::domain::repositories::AgentConversationLinearIssueRepository;
use crate::infrastructure::sqlite::SqliteAgentConversationLinearIssueRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_agent_conversation_linear_issue_repo_tests")
}

fn seed_conversation(db: &SqliteTestDb, conversation_id: &ChatConversationId) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (
                id, context_type, context_id, agent_mode, created_at, updated_at
             ) VALUES (?1, 'project', 'project-1', 'edit', '2026-06-18T18:14:05Z', '2026-06-18T18:14:05Z')",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_messages (
                id, conversation_id, role, content, created_at
             ) VALUES ('msg-1', ?1, 'user', 'Work on Linear', '2026-06-18T18:14:05Z')",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
    });
}

fn link(conversation_id: &ChatConversationId, issue_id: &str) -> AgentConversationLinearIssueLink {
    let assigned_at = Utc::now();
    AgentConversationLinearIssueLink::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        issue_id.to_string(),
        assigned_at,
    )
    .with_reference_metadata(
        Some("LIN-123".to_string()),
        Some("Linear title".to_string()),
        Some("https://linear.app/acme/issue/LIN-123/example".to_string()),
    )
    .with_assignment_source(Some(ChatMessageId::from_string("msg-1")), false)
}

#[tokio::test]
async fn upsert_round_trips_assignment_and_rich_snapshot_fields() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationLinearIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conv-1");
    seed_conversation(&db, &conversation_id);
    let mut assignment = link(&conversation_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assignment.status = Some("In Progress".to_string());
    assignment.assignee = Some("Ada".to_string());
    assignment.reporter = Some("Grace".to_string());
    assignment.updated_at_remote = Some("2026-06-18T18:10:00Z".to_string());
    assignment.description_markdown = Some("**Fix it**".to_string());
    assignment.description_text = Some("Fix it".to_string());
    assignment.comments_json = "[]".to_string();
    assignment.attachments_json = "[]".to_string();
    assignment.refresh_status = AgentConversationLinearRefreshStatus::Loaded;
    assignment.last_refreshed_at = Some(Utc::now());

    let saved = repo.upsert(assignment.clone()).await.unwrap();
    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(saved.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(loaded.issue_key.as_deref(), Some("LIN-123"));
    assert_eq!(loaded.title.as_deref(), Some("Linear title"));
    assert_eq!(loaded.status.as_deref(), Some("In Progress"));
    assert_eq!(loaded.assignee.as_deref(), Some("Ada"));
    assert_eq!(loaded.reporter.as_deref(), Some("Grace"));
    assert_eq!(loaded.description_markdown.as_deref(), Some("**Fix it**"));
    assert_eq!(
        loaded.refresh_status,
        AgentConversationLinearRefreshStatus::Loaded
    );
    assert_eq!(
        loaded
            .assigned_from_message_id
            .as_ref()
            .map(|id| id.as_str()),
        Some("msg-1")
    );
}

#[tokio::test]
async fn insert_if_absent_does_not_overwrite_existing_assignment() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationLinearIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conv-1");
    seed_conversation(&db, &conversation_id);

    repo.upsert(link(
        &conversation_id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f",
    ))
    .await
    .unwrap();
    let returned = repo
        .insert_if_absent(link(
            &conversation_id,
            "639068e2-ae88-4d09-bd75-22eb4a59612f",
        ))
        .await
        .unwrap();

    assert_eq!(returned.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
}

#[tokio::test]
async fn get_by_conversation_id_falls_back_for_legacy_bad_status_and_datetime_values() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationLinearIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conv-1");
    seed_conversation(&db, &conversation_id);

    repo.upsert(link(
        &conversation_id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f",
    ))
    .await
    .unwrap();
    db.with_connection(|conn| {
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .unwrap();
        conn.execute(
            "UPDATE agent_conversation_linear_issue_links
             SET refresh_status = 'legacy',
                 assigned_at = 'bad-date',
                 created_at = 'bad-date',
                 updated_at = 'bad-date',
                 last_refreshed_at = 'bad-date'
             WHERE conversation_id = ?1",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
    });

    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(loaded.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(
        loaded.refresh_status,
        AgentConversationLinearRefreshStatus::NotLoaded
    );
    assert!(loaded.last_refreshed_at.is_some());
}

#[tokio::test]
async fn clear_removes_assignment() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationLinearIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conv-1");
    seed_conversation(&db, &conversation_id);

    repo.upsert(link(
        &conversation_id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f",
    ))
    .await
    .unwrap();
    repo.clear(&conversation_id).await.unwrap();

    assert!(repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .is_none());
}
