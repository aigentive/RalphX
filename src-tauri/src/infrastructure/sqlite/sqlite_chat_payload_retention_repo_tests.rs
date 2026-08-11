use chrono::{DateTime, Duration, Utc};
use rusqlite::params;

use super::sqlite_chat_payload_retention_repo::{
    PruneCursor, SqliteChatPayloadRetentionRepository, WalCheckpointOutcome,
};
use super::DbConnection;
use crate::testing::SqliteTestDb;

fn seed_payload(
    db: &SqliteTestDb,
    block_id: &str,
    conversation_id: &str,
    created_at: chrono::DateTime<Utc>,
    archived: bool,
) {
    seed_payload_sized(db, block_id, conversation_id, created_at, archived, 1);
}

/// Seeds one payload row whose JSON columns are padded so byte accounting is observable.
fn seed_payload_sized(
    db: &SqliteTestDb,
    block_id: &str,
    conversation_id: &str,
    created_at: chrono::DateTime<Utc>,
    archived: bool,
    padding: usize,
) {
    let filler = "x".repeat(padding);
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
            "INSERT INTO chat_message_block_payloads (block_id, input_json, result_json, raw_block_json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                block_id,
                format!("{{\"input\":\"{filler}\"}}"),
                format!("{{\"result\":\"{filler}\"}}"),
                format!("{{\"raw\":\"{filler}\"}}"),
                created_at.to_rfc3339()
            ],
        )
        .unwrap();
    });
}

fn payload_rows(db: &SqliteTestDb) -> i64 {
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM chat_message_block_payloads",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    })
}

fn payload_bytes(db: &SqliteTestDb) -> i64 {
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(COALESCE(input_json, '')) + LENGTH(COALESCE(result_json, '')) + LENGTH(COALESCE(raw_block_json, ''))), 0) FROM chat_message_block_payloads",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    })
}

async fn drain_time_window(
    repo: &SqliteChatPayloadRetentionRepository,
    before: DateTime<Utc>,
    archived_before: DateTime<Utc>,
    batch_rows: u32,
) -> (u64, u64, usize) {
    let mut cursor: PruneCursor = None;
    let (mut deleted, mut bytes, mut batches) = (0, 0, 0);
    loop {
        let outcome = repo
            .prune_batch(before, archived_before, batch_rows, cursor.clone())
            .await
            .unwrap();
        if outcome.deleted_rows == 0 {
            return (deleted, bytes, batches);
        }
        deleted += outcome.deleted_rows;
        bytes += outcome.payload_bytes;
        batches += 1;
        cursor = outcome.next_cursor;
    }
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

    let outcome = repo
        .prune_batch(now - Duration::days(90), now - Duration::days(7), 10, None)
        .await
        .unwrap();
    assert_eq!(outcome.deleted_rows, 1);
    assert!(outcome.payload_bytes > 0);
    assert!(outcome.next_cursor.is_some());

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

    let outcome = repo
        .prune_batch(now - Duration::days(90), now - Duration::days(7), 10, None)
        .await
        .unwrap();
    assert_eq!(outcome.deleted_rows, 1);
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
    for (index, id) in ["one", "two", "three"].into_iter().enumerate() {
        seed_payload(
            &db,
            id,
            &format!("conversation-{id}"),
            now - Duration::days(91) + Duration::seconds(index as i64),
            false,
        );
    }

    let first = repo
        .prune_batch(now - Duration::days(90), now - Duration::days(7), 2, None)
        .await
        .unwrap();
    assert_eq!(first.deleted_rows, 2);
    let second = repo
        .prune_batch(
            now - Duration::days(90),
            now - Duration::days(7),
            2,
            first.next_cursor.clone(),
        )
        .await
        .unwrap();
    assert_eq!(second.deleted_rows, 1);
    let third = repo
        .prune_batch(
            now - Duration::days(90),
            now - Duration::days(7),
            2,
            second.next_cursor,
        )
        .await
        .unwrap();
    assert_eq!(third.deleted_rows, 0);
    assert_eq!(third.next_cursor, None);
}

#[tokio::test]
async fn from_db_constructor_prunes_same_as_from_shared() {
    let db = SqliteTestDb::new("chat-payload-retention-from-db");
    let repo =
        SqliteChatPayloadRetentionRepository::from_db(DbConnection::from_shared(db.shared_conn()));
    let now = Utc::now();
    seed_payload(
        &db,
        "from-db-old",
        "from-db-conversation",
        now - Duration::days(91),
        false,
    );

    let outcome = repo
        .prune_batch(now - Duration::days(90), now - Duration::days(7), 10, None)
        .await
        .unwrap();
    assert_eq!(outcome.deleted_rows, 1);
}

#[tokio::test]
async fn the_cursor_advances_monotonically_and_never_revisits_the_pruned_prefix() {
    let db = SqliteTestDb::new("chat-payload-retention-cursor-linearity");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    for index in 0..6 {
        seed_payload(
            &db,
            &format!("block-{index}"),
            &format!("conversation-{index}"),
            now - Duration::days(91) + Duration::seconds(index),
            false,
        );
    }

    let mut cursor: PruneCursor = None;
    let mut seen_cursors = Vec::new();
    loop {
        let outcome = repo
            .prune_batch(now - Duration::days(90), now - Duration::days(7), 2, cursor)
            .await
            .unwrap();
        if outcome.deleted_rows == 0 {
            break;
        }
        let advanced = outcome
            .next_cursor
            .clone()
            .expect("cursor after a deletion");
        if let Some(previous) = seen_cursors.last() {
            assert!(
                &advanced > previous,
                "cursor must advance strictly forward: {advanced:?} after {previous:?}"
            );
        }
        seen_cursors.push(advanced);
        cursor = outcome.next_cursor;
    }

    assert_eq!(seen_cursors.len(), 3, "6 rows drain in 3 batches of 2");
    assert_eq!(payload_rows(&db), 0);
}

#[tokio::test]
async fn batch_byte_accounting_matches_the_bytes_actually_removed() {
    let db = SqliteTestDb::new("chat-payload-retention-byte-accounting");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    for index in 0..4 {
        seed_payload_sized(
            &db,
            &format!("block-{index}"),
            &format!("conversation-{index}"),
            now - Duration::days(91) + Duration::seconds(index),
            false,
            64,
        );
    }
    let before_bytes = payload_bytes(&db);
    let usage = repo.payload_usage().await.unwrap();
    assert_eq!(usage.row_count, 4);
    assert_eq!(usage.total_bytes, before_bytes as u64);

    let (deleted, bytes, batches) =
        drain_time_window(&repo, now - Duration::days(90), now - Duration::days(7), 2).await;

    assert_eq!(deleted, 4);
    assert_eq!(batches, 2);
    assert_eq!(bytes, before_bytes as u64);
    assert_eq!(payload_bytes(&db), 0);
}

#[tokio::test]
async fn prune_oldest_batch_ignores_the_time_window_and_walks_oldest_first() {
    let db = SqliteTestDb::new("chat-payload-retention-oldest-first");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    seed_payload(&db, "younger", "conversation-younger", now, false);
    seed_payload(
        &db,
        "older",
        "conversation-older",
        now - Duration::days(1),
        false,
    );

    let outcome = repo.prune_oldest_batch(1, None).await.unwrap();
    assert_eq!(outcome.deleted_rows, 1);
    db.with_connection(|conn| {
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM chat_message_block_payloads WHERE block_id = 'older'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0,
            "the oldest payload goes first"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM chat_message_block_payloads WHERE block_id = 'younger'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    });
}

#[tokio::test]
async fn database_size_hint_bounds_payload_usage_from_above() {
    let db = SqliteTestDb::new("chat-payload-retention-size-hint");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    seed_payload_sized(&db, "sized", "conversation-sized", Utc::now(), false, 4_096);

    let hint = repo.database_size_hint().await.unwrap();
    let usage = repo.payload_usage().await.unwrap();
    assert!(
        hint >= usage.total_bytes,
        "page-count hint {hint} must bound payload bytes {}",
        usage.total_bytes
    );
}

#[tokio::test]
async fn preview_is_read_only_and_matches_what_a_real_size_prune_removes() {
    let db = SqliteTestDb::new("chat-payload-retention-preview");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    for index in 0..6 {
        seed_payload_sized(
            &db,
            &format!("block-{index}"),
            &format!("conversation-{index}"),
            now - Duration::days(10) + Duration::seconds(index),
            false,
            256,
        );
    }
    let total_bytes = payload_bytes(&db) as u64;
    let budget = total_bytes / 2;

    let preview = repo.preview_size_budget_prune(budget).await.unwrap();
    assert!(preview.rows > 0);
    assert!(preview.bytes > 0);
    assert!(preview.cut_created_at.is_some());
    assert_eq!(payload_rows(&db), 6, "preview must not delete anything");
    assert_eq!(payload_bytes(&db) as u64, total_bytes);

    // A real prune at the same budget removes exactly what the preview reported.
    let mut cursor: PruneCursor = None;
    let mut surviving = total_bytes;
    let (mut removed_rows, mut removed_bytes) = (0_u64, 0_u64);
    while surviving > budget {
        let outcome = repo.prune_oldest_batch(1, cursor).await.unwrap();
        if outcome.deleted_rows == 0 {
            break;
        }
        surviving -= outcome.payload_bytes;
        removed_rows += outcome.deleted_rows;
        removed_bytes += outcome.payload_bytes;
        cursor = outcome.next_cursor;
    }

    assert_eq!(removed_rows, preview.rows);
    assert_eq!(removed_bytes, preview.bytes);
}

#[tokio::test]
async fn preview_reports_nothing_when_usage_is_already_under_budget() {
    let db = SqliteTestDb::new("chat-payload-retention-preview-under-budget");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    seed_payload(&db, "small", "conversation-small", Utc::now(), false);

    let preview = repo.preview_size_budget_prune(1_073_741_824).await.unwrap();
    assert_eq!(preview.rows, 0);
    assert_eq!(preview.bytes, 0);
    assert_eq!(preview.cut_created_at, None);
}

#[tokio::test]
async fn checkpoint_truncate_reports_an_outcome_instead_of_failing_the_cycle() {
    let db = SqliteTestDb::new("chat-payload-retention-checkpoint");
    let repo = SqliteChatPayloadRetentionRepository::from_shared(db.shared_conn());
    seed_payload(
        &db,
        "checkpointed",
        "conversation-checkpoint",
        Utc::now(),
        false,
    );

    let outcome = repo.checkpoint_truncate().await.unwrap();
    assert!(matches!(
        outcome,
        WalCheckpointOutcome::Truncated | WalCheckpointOutcome::Busy
    ));
}
