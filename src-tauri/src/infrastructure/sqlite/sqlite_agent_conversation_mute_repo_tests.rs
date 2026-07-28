use chrono::{TimeZone, Utc};

use crate::domain::entities::{AgentConversationMute, ChatConversationId};
use crate::domain::repositories::AgentConversationMuteRepository;
use crate::infrastructure::sqlite::SqliteAgentConversationMuteRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_agent_conversation_mute_repo_tests")
}

fn seed_conversation(db: &SqliteTestDb, conversation_id: &ChatConversationId) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (
                id, context_type, context_id, agent_mode, created_at, updated_at
             ) VALUES (?1, 'project', 'project-1', 'edit', '2026-07-28T15:00:00Z', '2026-07-28T15:00:00Z')",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
    });
}

fn mute(conversation_id: &ChatConversationId, fingerprint: &str) -> AgentConversationMute {
    AgentConversationMute {
        conversation_id: conversation_id.clone(),
        muted_at: Utc.with_ymd_and_hms(2026, 7, 28, 15, 0, 0).unwrap(),
        state_fingerprint: fingerprint.to_string(),
    }
}

#[tokio::test]
async fn set_muted_round_trips() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationMuteRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("11111111-1111-4111-8111-111111111111");
    seed_conversation(&db, &conversation_id);
    let expected = mute(&conversation_id, "fingerprint-1");

    repo.set_muted(expected.clone()).await.unwrap();

    assert_eq!(
        repo.get_by_conversation_id(&conversation_id).await.unwrap(),
        Some(expected)
    );
}

#[tokio::test]
async fn set_muted_upserts_existing_mute() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationMuteRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("11111111-1111-4111-8111-111111111111");
    seed_conversation(&db, &conversation_id);

    repo.set_muted(mute(&conversation_id, "fingerprint-1"))
        .await
        .unwrap();
    let updated = AgentConversationMute {
        muted_at: Utc.with_ymd_and_hms(2026, 7, 28, 16, 0, 0).unwrap(),
        ..mute(&conversation_id, "fingerprint-2")
    };
    repo.set_muted(updated.clone()).await.unwrap();

    assert_eq!(
        repo.get_by_conversation_id(&conversation_id).await.unwrap(),
        Some(updated)
    );
    let count = db.with_connection(|conn| {
        conn.query_row("SELECT COUNT(*) FROM agent_conversation_mutes", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    });
    assert_eq!(count, 1);
}

#[tokio::test]
async fn clear_removes_mute() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationMuteRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("11111111-1111-4111-8111-111111111111");
    seed_conversation(&db, &conversation_id);
    repo.set_muted(mute(&conversation_id, "fingerprint-1"))
        .await
        .unwrap();

    repo.clear(&conversation_id).await.unwrap();

    assert!(repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn list_by_conversation_ids_filters_requested_ids_and_handles_empty_input() {
    let db = setup_test_db();
    let repo = SqliteAgentConversationMuteRepository::from_shared(db.shared_conn());
    let first = ChatConversationId::from_string("11111111-1111-4111-8111-111111111111");
    let second = ChatConversationId::from_string("22222222-2222-4222-8222-222222222222");
    let unrequested = ChatConversationId::from_string("33333333-3333-4333-8333-333333333333");
    for conversation_id in [&first, &second, &unrequested] {
        seed_conversation(&db, conversation_id);
    }
    repo.set_muted(mute(&first, "fingerprint-1")).await.unwrap();
    repo.set_muted(mute(&second, "fingerprint-2"))
        .await
        .unwrap();
    repo.set_muted(mute(&unrequested, "fingerprint-3"))
        .await
        .unwrap();

    let listed = repo
        .list_by_conversation_ids(&[first.clone(), second.clone()])
        .await
        .unwrap();

    assert_eq!(listed.len(), 2);
    assert!(listed
        .iter()
        .all(|mute| { mute.conversation_id == first || mute.conversation_id == second }));
    assert!(repo.list_by_conversation_ids(&[]).await.unwrap().is_empty());
}
