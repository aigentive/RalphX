use std::sync::Arc;
use std::time::Duration;

use super::data_retention_service::{
    compaction_recommended, DataRetentionService, RetentionCycleGuard, RetentionSkipReason,
    RetentionTuning, CYCLE_TEST_SERIALIZER,
};
use crate::domain::entities::data_retention::DataRetentionDefaults;
use crate::domain::entities::data_retention::DataRetentionPolicyUpdate;
use crate::error::AppError;
use crate::infrastructure::sqlite::sqlite_chat_payload_retention_repo::SqliteChatPayloadRetentionRepository;
use crate::infrastructure::sqlite::sqlite_data_retention_settings_repo::SqliteDataRetentionSettingsRepository;
use crate::testing::SqliteTestDb;
use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::params;

struct Fixture {
    db: SqliteTestDb,
    service: DataRetentionService,
    settings_repo: Arc<SqliteDataRetentionSettingsRepository>,
    payload_repo: Arc<SqliteChatPayloadRetentionRepository>,
}

fn defaults() -> DataRetentionDefaults {
    DataRetentionDefaults {
        enabled: true,
        days: 90,
        archived_days: 7,
        batch_rows: 2,
    }
}

fn tuning(advisory_threshold_bytes: u64) -> RetentionTuning {
    RetentionTuning {
        batch_pause: Duration::ZERO,
        checkpoint_batches: 2,
        advisory_threshold_bytes,
        compaction_recommended_freelist_percent: 20,
    }
}

fn build(name: &str, advisory_threshold_bytes: u64) -> Fixture {
    let db = SqliteTestDb::new(name);
    let payload_repo = Arc::new(SqliteChatPayloadRetentionRepository::from_shared(
        db.shared_conn(),
    ));
    let settings_repo = Arc::new(SqliteDataRetentionSettingsRepository::from_shared(
        db.shared_conn(),
    ));
    let service = DataRetentionService::new(
        Arc::clone(&payload_repo),
        Arc::clone(&settings_repo),
        defaults(),
        tuning(advisory_threshold_bytes),
    );
    Fixture {
        db,
        service,
        settings_repo,
        payload_repo,
    }
}

fn seed_payload(db: &SqliteTestDb, index: usize, age_days: i64, padding: usize) {
    let block_id = format!("block-{index}");
    let conversation_id = format!("conversation-{index}");
    let created_at = (Utc::now() - ChronoDuration::days(age_days)
        + ChronoDuration::seconds(index as i64))
    .to_rfc3339();
    let filler = "x".repeat(padding);
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at) VALUES (?1, 'project', 'project-1', ?2, ?2)",
            params![conversation_id, created_at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, 'assistant', 'message', ?3)",
            params![format!("message-{index}"), conversation_id, created_at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_blocks (id, conversation_id, message_id, sequence, block_index, role, kind, status, text, tool_input_preview, created_at, updated_at) VALUES (?1, ?2, ?3, 1, 0, 'assistant', 'tool_use', 'finalized', 'text remains', 'preview remains', ?4, ?4)",
            params![block_id, conversation_id, format!("message-{index}"), created_at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_block_payloads (block_id, input_json, result_json, raw_block_json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                block_id,
                format!("{{\"input\":\"{filler}\"}}"),
                format!("{{\"result\":\"{filler}\"}}"),
                format!("{{\"raw\":\"{filler}\"}}"),
                created_at
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

fn block_rows(db: &SqliteTestDb) -> i64 {
    db.with_connection(|conn| {
        conn.query_row("SELECT COUNT(*) FROM chat_message_blocks", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    })
}

/// Writes a confirmed budget straight to the row.
///
/// `update` enforces a 256 MiB floor (covered in the repo suite); these fixtures need
/// budgets measured in bytes to make the prune observable.
async fn force_confirmed_budget(fixture: &Fixture, budget: u64) {
    fixture.settings_repo.get_or_seed(defaults()).await.unwrap();
    fixture.db.with_connection(|conn| {
        conn.execute(
            "UPDATE data_retention_settings
             SET payload_size_budget_bytes = ?1, size_budget_confirmed_at = ?2, seeded_pristine = 0
             WHERE id = 1",
            params![budget as i64, Utc::now().to_rfc3339()],
        )
        .unwrap();
    });
}

#[tokio::test]
async fn a_default_install_prunes_only_outside_the_time_window() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let fixture = build("retention-cycle-default-install", u64::MAX);
    for index in 0..4 {
        seed_payload(&fixture.db, index, 5, 512);
    }
    seed_payload(&fixture.db, 99, 120, 512);

    let report = fixture
        .service
        .run_cycle(Utc::now())
        .await
        .expect("cycle runs");

    assert_eq!(report.pruned_rows, 1, "only the out-of-window payload goes");
    assert!(!report.size_budget_active);
    assert_eq!(
        report.skipped_reason,
        Some(RetentionSkipReason::SizeBudgetNotConfigured)
    );
    assert_eq!(payload_rows(&fixture.db), 4);
    assert_eq!(block_rows(&fixture.db), 5, "blocks and previews survive");
}

#[tokio::test]
async fn a_budget_without_a_confirmation_prunes_nothing_inside_the_window() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let fixture = build("retention-cycle-unconfirmed-budget", u64::MAX);
    for index in 0..4 {
        seed_payload(&fixture.db, index, 1, 512);
    }
    // Bypass `update`'s guard to prove the *service* is fail-closed on its own.
    fixture.settings_repo.get_or_seed(defaults()).await.unwrap();
    fixture.db.with_connection(|conn| {
        conn.execute(
            "UPDATE data_retention_settings SET payload_size_budget_bytes = 1, size_budget_confirmed_at = NULL WHERE id = 1",
            [],
        )
        .unwrap();
    });

    let report = fixture
        .service
        .run_cycle(Utc::now())
        .await
        .expect("cycle runs");

    assert_eq!(report.pruned_rows, 0);
    assert!(!report.size_budget_active);
    assert_eq!(
        report.skipped_reason,
        Some(RetentionSkipReason::SizeBudgetUnconfirmed)
    );
    assert_eq!(payload_rows(&fixture.db), 4);
}

#[tokio::test]
async fn a_confirmed_budget_prunes_oldest_first_inside_the_time_window() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let fixture = build("retention-cycle-confirmed-budget", u64::MAX);
    for index in 0..6 {
        seed_payload(&fixture.db, index, 1, 512);
    }
    let usage = fixture.payload_repo.payload_usage().await.unwrap();
    force_confirmed_budget(&fixture, usage.total_bytes / 2).await;

    let report = fixture
        .service
        .run_cycle(Utc::now())
        .await
        .expect("cycle runs");

    assert!(report.size_budget_active);
    assert!(report.pruned_rows > 0);
    assert!(payload_rows(&fixture.db) < 6);
    assert_eq!(block_rows(&fixture.db), 6, "only payload rows are deleted");
    fixture.db.with_connection(|conn| {
        let oldest_remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_message_block_payloads WHERE block_id = 'block-0'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(oldest_remaining, 0, "the oldest payload goes first");
    });
}

#[tokio::test]
async fn a_size_prune_terminates_when_the_budget_is_unreachable() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let fixture = build("retention-cycle-unreachable-budget", u64::MAX);
    for index in 0..4 {
        seed_payload(&fixture.db, index, 1, 256);
    }
    force_confirmed_budget(&fixture, 1).await;

    let report = fixture
        .service
        .run_cycle(Utc::now())
        .await
        .expect("cycle terminates");

    assert_eq!(report.pruned_rows, 4, "the table drains and the loop stops");
    assert_eq!(payload_rows(&fixture.db), 0);
}

#[tokio::test]
async fn a_disabled_policy_is_a_no_op() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let fixture = build("retention-cycle-disabled", u64::MAX);
    seed_payload(&fixture.db, 0, 200, 128);
    fixture
        .settings_repo
        .update(DataRetentionPolicyUpdate {
            enabled: false,
            days: 90,
            archived_days: 7,
            batch_rows: 2,
            size_budget_bytes: None,
            size_budget_confirmed_at: None,
        })
        .await
        .unwrap();

    let report = fixture.service.run_cycle(Utc::now()).await.unwrap();

    assert_eq!(report.pruned_rows, 0);
    assert_eq!(
        report.skipped_reason,
        Some(RetentionSkipReason::RetentionDisabled)
    );
    assert_eq!(payload_rows(&fixture.db), 1);
}

#[tokio::test]
async fn a_large_database_persists_the_advisory_without_deleting_anything() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    // Threshold of zero makes every database "large", exercising the advisory path.
    let fixture = build("retention-cycle-advisory", 0);
    for index in 0..3 {
        seed_payload(&fixture.db, index, 1, 256);
    }

    let report = fixture.service.run_cycle(Utc::now()).await.unwrap();

    assert!(report.size_budget_advised);
    assert_eq!(report.pruned_rows, 0);
    assert_eq!(payload_rows(&fixture.db), 3, "the advisory never deletes");
    assert!(report.payload_bytes_after.unwrap_or(0) > 0);

    let persisted = fixture.settings_repo.get_or_seed(defaults()).await.unwrap();
    assert!(
        persisted.size_budget_advised,
        "Settings must render the advisory on open, without running a cycle"
    );
    assert!(persisted.last_run_payload_bytes.unwrap_or(0) > 0);
}

#[tokio::test]
async fn a_small_database_skips_the_scan_and_clears_a_stale_advisory() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let fixture = build("retention-cycle-no-advisory", u64::MAX);
    seed_payload(&fixture.db, 0, 1, 128);
    fixture.settings_repo.get_or_seed(defaults()).await.unwrap();
    fixture.db.with_connection(|conn| {
        conn.execute(
            "UPDATE data_retention_settings SET size_budget_advised = 1 WHERE id = 1",
            [],
        )
        .unwrap();
    });

    let report = fixture.service.run_cycle(Utc::now()).await.unwrap();

    assert!(!report.size_budget_advised);
    assert_eq!(
        report.payload_bytes_after, None,
        "the constant-time gate must skip the full payload scan"
    );
    let persisted = fixture.settings_repo.get_or_seed(defaults()).await.unwrap();
    assert!(!persisted.size_budget_advised);
}

#[tokio::test]
async fn the_cycle_guard_is_process_global_across_separately_constructed_services() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let fixture = build("retention-cycle-guard", u64::MAX);
    let second = DataRetentionService::new(
        Arc::clone(&fixture.payload_repo),
        Arc::clone(&fixture.settings_repo),
        defaults(),
        tuning(u64::MAX),
    );

    let guard = RetentionCycleGuard::try_acquire().expect("first claim succeeds");
    let error = second
        .run_cycle(Utc::now())
        .await
        .expect_err("a second instance must be rejected while a cycle is in flight");
    assert!(matches!(error, AppError::Conflict(_)));

    drop(guard);
    fixture
        .service
        .run_cycle(Utc::now())
        .await
        .expect("the guard is released once the first cycle ends");
}

#[tokio::test]
async fn the_guard_is_released_after_a_failing_cycle() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let fixture = build("retention-cycle-guard-error", u64::MAX);
    fixture.db.with_connection(|conn| {
        conn.execute_batch("DROP TABLE data_retention_settings")
            .unwrap();
    });

    fixture
        .service
        .run_cycle(Utc::now())
        .await
        .expect_err("a broken policy table fails the cycle");

    assert!(
        RetentionCycleGuard::try_acquire().is_some(),
        "the guard must be released on the error path too"
    );
}

#[test]
fn compaction_is_recommended_only_when_the_freelist_is_a_significant_share() {
    assert!(compaction_recommended(1_000, 300, 20));
    assert!(compaction_recommended(1_000, 200, 20));
    assert!(!compaction_recommended(1_000, 100, 20));
    assert!(!compaction_recommended(0, 100, 20));
}
