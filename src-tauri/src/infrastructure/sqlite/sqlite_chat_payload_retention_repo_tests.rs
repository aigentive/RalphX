use chrono::{Duration, Utc};
use rusqlite::params;

use super::sqlite_chat_payload_retention_repo::SqliteChatPayloadRetentionRepository;
use super::DbConnection;
use crate::testing::SqliteTestDb;

fn seed_payload(
    db: &SqliteTestDb,
    block_id: &str,
    conversation_id: &str,
    created_at: chrono::DateTime<Utc>,
    archived: bool,
) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at, archived_at) VALUES (?1, 'project', 'project-1', ?2, ?2, ?3)",
            params![conversation_id, created_at.to_rfc3339(), archived.then(|| created_at.to_rfc3339())],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, 'assistant', 'message', ?3)",
            params![format!("message-{block_id}"), conversation_id, created_at.to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_blocks (id, conversation_id, message_id, sequence, block_index, role, kind, status, text, tool_input_preview, tool_result_preview, metadata, created_at, updated_at) VALUES (?1, ?2, ?3, 1, 0, 'assistant', 'tool_use', 'finalized', 'text remains', 'input preview', 'result preview', '{\"preserved\":true}', ?4, ?4)",
            params![block_id, conversation_id, format!("message-{block_id}"), created_at.to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_block_payloads (block_id, input_json, result_json, raw_block_json, updated_at) VALUES (?1, '{\"input\":true}', '{\"result\":true}', '{\"raw\":true}', ?2)",
            params![block_id, created_at.to_rfc3339()],
        )
        .unwrap();
    });
}

#[tokio::test]
async fn prune_batch_removes_old_payloads_but_keeps_recent_blocks_and_previews() {
    let db = SqliteTestDb::new("chat-payload-retention-old-and-recent");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    seed_payload(
        &db,
        "old",
        "old-conversation",
        now - Duration::days(91),
        false,
    );
    seed_payload(
        &db,
        "recent",
        "recent-conversation",
        now - Duration::days(89),
        false,
    );

    assert_eq!(
        repo.prune_batch(now - Duration::days(90), now - Duration::days(7), 10)
            .await
            .unwrap(),
        1
    );
    db.with_connection(|conn| {
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM chat_message_block_payloads WHERE block_id = 'old'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM chat_message_block_payloads WHERE block_id = 'recent'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        let block = conn.query_row("SELECT text, tool_input_preview, tool_result_preview, metadata FROM chat_message_blocks WHERE id = 'old'", [], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?))).unwrap();
        assert_eq!(block, ("text remains".into(), Some("input preview".into()), Some("result preview".into()), Some("{\"preserved\":true}".into())));
    });
}

#[tokio::test]
async fn prune_batch_uses_shorter_archived_conversation_window() {
    let db = SqliteTestDb::new("chat-payload-retention-archived");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    seed_payload(
        &db,
        "archived",
        "archived-conversation",
        now - Duration::days(8),
        true,
    );
    seed_payload(
        &db,
        "active",
        "active-conversation",
        now - Duration::days(8),
        false,
    );

    assert_eq!(
        repo.prune_batch(now - Duration::days(90), now - Duration::days(7), 10)
            .await
            .unwrap(),
        1
    );
    db.with_connection(|conn| {
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM chat_message_block_payloads WHERE block_id = 'archived'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM chat_message_block_payloads WHERE block_id = 'active'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    });
}

#[tokio::test]
async fn prune_batch_honors_limit_and_is_idempotent() {
    let db = SqliteTestDb::new("chat-payload-retention-batches");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    for id in ["one", "two", "three"] {
        seed_payload(
            &db,
            id,
            &format!("conversation-{id}"),
            now - Duration::days(91),
            false,
        );
    }

    assert_eq!(
        repo.prune_batch(now - Duration::days(90), now - Duration::days(7), 2)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        repo.prune_batch(now - Duration::days(90), now - Duration::days(7), 2)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        repo.prune_batch(now - Duration::days(90), now - Duration::days(7), 2)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn from_db_constructor_prunes_same_as_from_shared() {
    let db = SqliteTestDb::new("chat-payload-retention-from-db");
    let repo = SqliteChatPayloadRetentionRepository::from_db(DbConnection::from_shared(
        db.shared_conn(),
    ));
    let now = Utc::now();
    seed_payload(
        &db,
        "from-db-old",
        "from-db-conversation",
        now - Duration::days(91),
        false,
    );

    assert_eq!(
        repo.prune_batch(now - Duration::days(90), now - Duration::days(7), 10)
            .await
            .unwrap(),
        1
    );
}
