use super::settings::{RemoteExposureMode, RemoteHostSettingsStore, DEFAULT_REMOTE_PORT};
use crate::testing::SqliteTestDb;
use uuid::Uuid;

#[tokio::test]
async fn first_access_mints_a_disabled_singleton_with_valid_defaults() {
    let db = SqliteTestDb::new("remote-host-settings-first-access");
    db.with_connection(|conn| {
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM remote_host_settings", [], |row| {
                row.get(0)
            })
            .expect("migration should leave the singleton row absent");
        assert_eq!(row_count, 0);
    });
    let store = RemoteHostSettingsStore::from_shared(db.shared_conn());

    let settings = store
        .get_or_create()
        .await
        .expect("first access should create settings");

    assert!(!settings.enabled);
    assert_eq!(settings.exposure_mode, RemoteExposureMode::Serve);
    assert_eq!(settings.port, DEFAULT_REMOTE_PORT);
    assert!(Uuid::parse_str(&settings.environment_id).is_ok());
    db.with_connection(|conn| {
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM remote_host_settings", [], |row| {
                row.get(0)
            })
            .expect("singleton row should be queryable");
        assert_eq!(row_count, 1);
    });
}

#[tokio::test]
async fn repeated_access_and_a_reopened_connection_keep_the_environment_id() {
    let db = SqliteTestDb::new("remote-host-settings-stable-environment-id");
    let first_store = RemoteHostSettingsStore::from_shared(db.shared_conn());
    let first = first_store
        .get_or_create()
        .await
        .expect("first access should create settings");
    let second = first_store
        .get_or_create()
        .await
        .expect("second access should read settings");
    let reopened_store = RemoteHostSettingsStore::new(db.new_connection());
    let reopened = reopened_store
        .get_or_create()
        .await
        .expect("reopened connection should read settings");

    assert_eq!(first.environment_id, second.environment_id);
    assert_eq!(first.environment_id, reopened.environment_id);
    assert!(Uuid::parse_str(&reopened.environment_id).is_ok());
    db.with_connection(|conn| {
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM remote_host_settings", [], |row| {
                row.get(0)
            })
            .expect("singleton row should be queryable");
        assert_eq!(row_count, 1);
    });
}
