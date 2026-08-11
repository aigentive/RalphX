use chrono::Utc;

use super::sqlite_data_retention_settings_repo::SqliteDataRetentionSettingsRepository;
use crate::domain::entities::data_retention::{
    DataRetentionDefaults, DataRetentionPolicyUpdate, DataRetentionRunStatus, MIN_SIZE_BUDGET_BYTES,
};
use crate::error::AppError;
use crate::testing::SqliteTestDb;

fn shipped_defaults() -> DataRetentionDefaults {
    DataRetentionDefaults {
        enabled: true,
        days: 90,
        archived_days: 7,
        batch_rows: 500,
    }
}

fn repo(db: &SqliteTestDb) -> SqliteDataRetentionSettingsRepository {
    SqliteDataRetentionSettingsRepository::from_shared(db.shared_conn())
}

#[tokio::test]
async fn absent_row_is_seeded_from_config_defaults_without_a_size_budget() {
    let db = SqliteTestDb::new("data-retention-settings-seed");
    let settings = repo(&db)
        .get_or_seed(shipped_defaults())
        .await
        .expect("seed policy row");

    assert!(settings.enabled);
    assert_eq!(settings.days, 90);
    assert_eq!(settings.archived_days, 7);
    assert_eq!(settings.batch_rows, 500);
    assert_eq!(settings.size_budget_bytes, None);
    assert_eq!(settings.size_budget_confirmed_at, None);
    assert!(settings.seeded_pristine);
    assert!(!settings.size_budget_advised);
    assert!(!settings.size_budget_active());
}

#[tokio::test]
async fn pristine_rows_pick_up_changed_config_defaults() {
    let db = SqliteTestDb::new("data-retention-settings-refresh");
    let repo = repo(&db);
    repo.get_or_seed(shipped_defaults())
        .await
        .expect("initial seed");

    let refreshed = repo
        .get_or_seed(DataRetentionDefaults {
            enabled: true,
            days: 45,
            archived_days: 3,
            batch_rows: 250,
        })
        .await
        .expect("refresh pristine row");

    assert_eq!(refreshed.days, 45);
    assert_eq!(refreshed.archived_days, 3);
    assert_eq!(refreshed.batch_rows, 250);
    assert!(refreshed.seeded_pristine);
}

#[tokio::test]
async fn user_edited_rows_are_never_overwritten_by_config_defaults() {
    let db = SqliteTestDb::new("data-retention-settings-user-owned");
    let repo = repo(&db);
    repo.get_or_seed(shipped_defaults())
        .await
        .expect("initial seed");

    let updated = repo
        .update(DataRetentionPolicyUpdate {
            enabled: true,
            days: 30,
            archived_days: 2,
            batch_rows: 100,
            size_budget_bytes: None,
            size_budget_confirmed_at: None,
        })
        .await
        .expect("user policy update");
    assert!(!updated.seeded_pristine);

    let after_config_change = repo
        .get_or_seed(DataRetentionDefaults {
            enabled: false,
            days: 365,
            archived_days: 90,
            batch_rows: 9_000,
        })
        .await
        .expect("re-seed attempt");

    assert_eq!(after_config_change.days, 30);
    assert_eq!(after_config_change.archived_days, 2);
    assert_eq!(after_config_change.batch_rows, 100);
    assert!(after_config_change.enabled);
}

#[tokio::test]
async fn seeding_can_never_write_a_size_budget_or_a_confirmation() {
    let db = SqliteTestDb::new("data-retention-settings-config-cannot-delete");
    let repo = repo(&db);
    repo.get_or_seed(shipped_defaults())
        .await
        .expect("initial seed");

    let reseeded = repo
        .get_or_seed(DataRetentionDefaults {
            enabled: true,
            days: 60,
            archived_days: 5,
            batch_rows: 400,
        })
        .await
        .expect("refresh pristine row");

    assert_eq!(reseeded.size_budget_bytes, None);
    assert_eq!(reseeded.size_budget_confirmed_at, None);
    assert!(!reseeded.size_budget_active());

    db.with_connection(|conn| {
        let (budget, confirmed): (Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT payload_size_budget_bytes, size_budget_confirmed_at
                 FROM data_retention_settings WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read size budget columns");
        assert_eq!(budget, None);
        assert_eq!(confirmed, None);
    });
}

#[tokio::test]
async fn seeded_values_are_clamped_so_a_zero_batch_never_persists() {
    let db = SqliteTestDb::new("data-retention-settings-clamped");
    let settings = repo(&db)
        .get_or_seed(DataRetentionDefaults {
            enabled: true,
            days: 0,
            archived_days: 0,
            batch_rows: 0,
        })
        .await
        .expect("seed clamped policy row");

    assert!(settings.batch_rows >= 1);
    assert!(settings.days >= 1);
    assert!(settings.archived_days >= 1);
}

#[tokio::test]
async fn an_unconfirmed_budget_is_unrepresentable_in_the_database() {
    let db = SqliteTestDb::new("data-retention-settings-unconfirmed-budget");
    let repo = repo(&db);
    repo.get_or_seed(shipped_defaults()).await.expect("seed");

    let error = repo
        .update(DataRetentionPolicyUpdate {
            enabled: true,
            days: 90,
            archived_days: 7,
            batch_rows: 500,
            size_budget_bytes: Some(MIN_SIZE_BUDGET_BYTES),
            size_budget_confirmed_at: None,
        })
        .await
        .expect_err("unconfirmed budget must be rejected at the repo boundary");
    assert!(matches!(error, AppError::Validation(_)));

    db.with_connection(|conn| {
        let budget: Option<i64> = conn
            .query_row(
                "SELECT payload_size_budget_bytes FROM data_retention_settings WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("read size budget");
        assert_eq!(budget, None);
    });
}

#[tokio::test]
async fn a_confirmed_budget_round_trips_and_clearing_it_clears_consent() {
    let db = SqliteTestDb::new("data-retention-settings-confirmed-budget");
    let repo = repo(&db);
    repo.get_or_seed(shipped_defaults()).await.expect("seed");
    let confirmed_at = Utc::now();

    let opted_in = repo
        .update(DataRetentionPolicyUpdate {
            enabled: true,
            days: 90,
            archived_days: 7,
            batch_rows: 500,
            size_budget_bytes: Some(MIN_SIZE_BUDGET_BYTES),
            size_budget_confirmed_at: Some(confirmed_at),
        })
        .await
        .expect("confirmed opt-in persists");
    assert_eq!(opted_in.size_budget_bytes, Some(MIN_SIZE_BUDGET_BYTES));
    assert!(opted_in.size_budget_confirmed_at.is_some());
    assert!(opted_in.size_budget_active());

    let cleared = repo
        .update(DataRetentionPolicyUpdate {
            enabled: true,
            days: 90,
            archived_days: 7,
            batch_rows: 500,
            size_budget_bytes: None,
            size_budget_confirmed_at: None,
        })
        .await
        .expect("clearing the budget succeeds");
    assert_eq!(cleared.size_budget_bytes, None);
    assert_eq!(cleared.size_budget_confirmed_at, None);
    assert!(!cleared.size_budget_active());
}

#[tokio::test]
async fn run_status_is_persisted_so_settings_render_without_a_scan() {
    let db = SqliteTestDb::new("data-retention-settings-run-status");
    let repo = repo(&db);
    repo.get_or_seed(shipped_defaults()).await.expect("seed");
    let ran_at = Utc::now();

    repo.record_run_status(DataRetentionRunStatus {
        pruned_rows: 42,
        payload_bytes: Some(12_884_901_888),
        payload_rows: Some(873_000),
        size_budget_advised: true,
        ran_at,
    })
    .await
    .expect("record run status");

    let settings = repo
        .get_or_seed(shipped_defaults())
        .await
        .expect("read back status");
    assert_eq!(settings.last_run_pruned_rows, Some(42));
    assert_eq!(settings.last_run_payload_bytes, Some(12_884_901_888));
    assert_eq!(settings.last_run_payload_rows, Some(873_000));
    assert!(settings.size_budget_advised);
    assert!(settings.last_run_at.is_some());
}

#[tokio::test]
async fn a_later_cycle_clears_a_stale_advisory() {
    let db = SqliteTestDb::new("data-retention-settings-advisory-clears");
    let repo = repo(&db);
    repo.get_or_seed(shipped_defaults()).await.expect("seed");

    repo.record_run_status(DataRetentionRunStatus {
        pruned_rows: 0,
        payload_bytes: Some(20_000_000_000),
        payload_rows: Some(500_000),
        size_budget_advised: true,
        ran_at: Utc::now(),
    })
    .await
    .expect("advisory cycle");
    repo.record_run_status(DataRetentionRunStatus {
        pruned_rows: 0,
        payload_bytes: Some(1_000),
        payload_rows: Some(2),
        size_budget_advised: false,
        ran_at: Utc::now(),
    })
    .await
    .expect("healthy cycle");

    let settings = repo.get_or_seed(shipped_defaults()).await.expect("read");
    assert!(!settings.size_budget_advised);
}
