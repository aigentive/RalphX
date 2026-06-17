use chrono::Utc;

use crate::domain::entities::{
    AgentConversationJiraIssueLink, AgentConversationJiraRefreshStatus, ChatConversationId,
    ChatMessageId, ProjectId,
};
use crate::domain::repositories::AgentConversationJiraIssueRepository;
use crate::infrastructure::sqlite::SqliteAgentConversationJiraIssueRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_agent_conversation_jira_issue_repo_tests")
}

fn seed_conversation(db: &SqliteTestDb, conversation_id: &ChatConversationId) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (
                id, context_type, context_id, agent_mode, created_at, updated_at
             ) VALUES (?1, 'project', 'project-1', 'edit', '2026-06-17T12:00:00Z', '2026-06-17T12:00:00Z')",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_messages (
                id, conversation_id, role, content, created_at
             ) VALUES ('msg-1', ?1, 'user', 'Work on Jira', '2026-06-17T12:00:00Z')",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
    });
}

fn link(conversation_id: &ChatConversationId, issue_key: &str) -> AgentConversationJiraIssueLink {
    let assigned_at = Utc::now();
    AgentConversationJiraIssueLink::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        issue_key.to_string(),
        assigned_at,
    )
    .with_reference_metadata(
        Some(format!("{issue_key}-id")),
        Some(format!("{issue_key} title")),
        Some(format!("https://jira.test/browse/{issue_key}")),
    )
    .with_assignment_source(Some(ChatMessageId::from_string("msg-1")), false)
}

#[tokio::test]
async fn upsert_round_trips_assignment_and_rich_snapshot_fields() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationJiraIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conv-1");
    seed_conversation(&db, &conversation_id);
    let mut assignment = link(&conversation_id, "RX-42");
    assignment.status = Some("In Progress".to_string());
    assignment.description_markdown = Some("**Fix it**".to_string());
    assignment.description_text = Some("Fix it".to_string());
    assignment.acceptance_criteria_markdown = Some("- Done".to_string());
    assignment.comments_json = r#"[{"id":"c1","bodyMarkdown":"Looks good"}]"#.to_string();
    assignment.attachments_json = r#"[{"id":"a1","filename":"log.txt"}]"#.to_string();
    assignment.refresh_status = AgentConversationJiraRefreshStatus::Loaded;
    assignment.last_refreshed_at = Some(Utc::now());

    let saved = repo.upsert(assignment.clone()).await.unwrap();
    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(saved.issue_key, "RX-42");
    assert_eq!(loaded.issue_key, "RX-42");
    assert_eq!(loaded.title.as_deref(), Some("RX-42 title"));
    assert_eq!(loaded.status.as_deref(), Some("In Progress"));
    assert_eq!(loaded.description_markdown.as_deref(), Some("**Fix it**"));
    assert_eq!(
        loaded.refresh_status,
        AgentConversationJiraRefreshStatus::Loaded
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
    let repo = SqliteAgentConversationJiraIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conv-1");
    seed_conversation(&db, &conversation_id);

    repo.upsert(link(&conversation_id, "RX-42")).await.unwrap();
    let returned = repo
        .insert_if_absent(link(&conversation_id, "RX-77"))
        .await
        .unwrap();

    assert_eq!(returned.issue_key, "RX-42");
    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.issue_key, "RX-42");
}

#[tokio::test]
async fn clear_removes_assignment() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationJiraIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conv-1");
    seed_conversation(&db, &conversation_id);

    repo.upsert(link(&conversation_id, "RX-42")).await.unwrap();
    repo.clear(&conversation_id).await.unwrap();

    assert!(repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .is_none());
}
