use chrono::Utc;

use crate::domain::entities::{
    AgentConversationGranolaNoteLink, AgentConversationGranolaRefreshStatus, ChatConversationId,
    ProjectId,
};
use crate::domain::repositories::AgentConversationGranolaNoteRepository;
use crate::infrastructure::sqlite::SqliteAgentConversationGranolaNoteRepository;
use crate::testing::SqliteTestDb;

fn db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_agent_conversation_granola_note_repo_tests")
}

fn seed_conversation(db: &SqliteTestDb, conversation_id: &ChatConversationId) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (
                id, context_type, context_id, agent_mode, created_at, updated_at
             ) VALUES (?1, 'project', 'project-1', 'edit', '2026-06-27T10:45:00Z', '2026-06-27T10:45:00Z')",
            [conversation_id.as_str()],
        )
        .unwrap();
    });
}

fn link(conversation_id: &ChatConversationId, note_id: &str) -> AgentConversationGranolaNoteLink {
    AgentConversationGranolaNoteLink::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        note_id.to_string(),
        Utc::now(),
    )
}

#[tokio::test]
async fn sqlite_granola_note_repo_upserts_fetches_and_clears() {
    let db = db();
    let repo = SqliteAgentConversationGranolaNoteRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conversation-1".to_string());
    seed_conversation(&db, &conversation_id);
    let first = repo
        .upsert(link(&conversation_id, "not_1234567890ABCD"))
        .await
        .unwrap();
    let mut replacement = link(&conversation_id, "not_ABCDEFGHIJKLMN");
    replacement.title = Some("Planning sync".to_string());
    replacement.summary_markdown = Some("Decided next steps".to_string());
    replacement.transcript_json = r#"[{"speaker":"Alex","text":"Ship it"}]"#.to_string();
    replacement.refresh_status = AgentConversationGranolaRefreshStatus::Loaded;

    let updated = repo.upsert(replacement).await.unwrap();
    assert_eq!(updated.created_at, first.created_at);
    assert_eq!(updated.note_id, "not_ABCDEFGHIJKLMN");
    assert_eq!(updated.title.as_deref(), Some("Planning sync"));

    let fetched = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.summary_markdown.as_deref(),
        Some("Decided next steps")
    );
    assert_eq!(
        fetched.refresh_status,
        AgentConversationGranolaRefreshStatus::Loaded
    );

    repo.clear(&conversation_id).await.unwrap();
    assert!(repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_granola_note_repo_insert_if_absent_keeps_original_assignment() {
    let db = db();
    let repo = SqliteAgentConversationGranolaNoteRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conversation-1".to_string());
    seed_conversation(&db, &conversation_id);

    repo.insert_if_absent(link(&conversation_id, "not_1234567890ABCD"))
        .await
        .unwrap();
    let kept = repo
        .insert_if_absent(link(&conversation_id, "not_ABCDEFGHIJKLMN"))
        .await
        .unwrap();

    assert_eq!(kept.note_id, "not_1234567890ABCD");
}
