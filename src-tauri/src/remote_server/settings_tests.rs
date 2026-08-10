use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::settings::{
    effective_remote_port, parse_remote_port, resolve_bind_address, validate_tailnet_bind_ip,
    RemoteBindError, RemoteExposureMode, RemoteHostSettingsStore, RemotePortOverrideError,
    TailnetProviderError, TailnetSelfAddressProvider, UnconfiguredTailnetProvider,
    DEFAULT_REMOTE_PORT,
};
use crate::infrastructure::sqlite::DbConnection;
use crate::testing::SqliteTestDb;
use uuid::Uuid;

/// Test double standing in for PR 1.6's `tailscale status --json` provider.
struct StaticTailnetProvider {
    result: Result<Vec<IpAddr>, TailnetProviderError>,
}

impl StaticTailnetProvider {
    fn with_addresses(addresses: &[&str]) -> Self {
        Self {
            result: Ok(addresses
                .iter()
                .map(|address| address.parse().expect("test address should parse"))
                .collect()),
        }
    }

    fn unavailable() -> Self {
        Self {
            result: Err(TailnetProviderError::Unavailable(
                "tailscale binary not found".to_string(),
            )),
        }
    }
}

#[async_trait::async_trait]
impl TailnetSelfAddressProvider for StaticTailnetProvider {
    async fn self_addresses(&self) -> Result<Vec<IpAddr>, TailnetProviderError> {
        self.result.clone()
    }
}

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
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));

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
    let first_store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    let first = first_store
        .get_or_create()
        .await
        .expect("first access should create settings");
    let second = first_store
        .get_or_create()
        .await
        .expect("second access should read settings");
    let reopened_store = RemoteHostSettingsStore::from_db(DbConnection::new(db.new_connection()));
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

#[tokio::test]
async fn setters_persist_enablement_and_exposure_mode_on_the_singleton_row() {
    let db = SqliteTestDb::new("remote-host-settings-setters");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));

    let enabled = store
        .set_enabled(true)
        .await
        .expect("enabling should mint and update the row");
    let switched = store
        .set_exposure_mode(RemoteExposureMode::TailnetDirect)
        .await
        .expect("exposure mode should persist");
    let disabled = store
        .set_enabled(false)
        .await
        .expect("disabling should persist");

    assert!(enabled.enabled);
    assert_eq!(enabled.exposure_mode, RemoteExposureMode::Serve);
    assert!(switched.enabled);
    assert_eq!(switched.exposure_mode, RemoteExposureMode::TailnetDirect);
    assert!(!disabled.enabled);
    assert_eq!(disabled.exposure_mode, RemoteExposureMode::TailnetDirect);
    assert_eq!(enabled.environment_id, disabled.environment_id);
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
async fn reading_settings_never_mints_the_row() {
    let db = SqliteTestDb::new("remote-host-settings-read-only");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));

    let missing = store.get().await.expect("read should succeed");

    assert!(missing.is_none());
    db.with_connection(|conn| {
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM remote_host_settings", [], |row| {
                row.get(0)
            })
            .expect("singleton row should be queryable");
        assert_eq!(row_count, 0);
    });
}

#[tokio::test]
async fn serve_mode_binds_loopback_only() {
    let provider = StaticTailnetProvider::with_addresses(&["100.101.102.103"]);

    let address = resolve_bind_address(RemoteExposureMode::Serve, DEFAULT_REMOTE_PORT, &provider)
        .await
        .expect("serve mode should resolve a bind address");

    assert_eq!(
        address,
        SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), DEFAULT_REMOTE_PORT))
    );
    assert!(address.ip().is_loopback());
}

#[tokio::test]
async fn tailnet_direct_binds_a_validated_cgnat_self_address() {
    let provider = StaticTailnetProvider::with_addresses(&["fd7a:115c:a1e0::1", "100.101.102.103"]);

    let address = resolve_bind_address(
        RemoteExposureMode::TailnetDirect,
        DEFAULT_REMOTE_PORT,
        &provider,
    )
    .await
    .expect("a CGNAT self address should be bindable");

    assert_eq!(
        address,
        SocketAddr::from((Ipv4Addr::new(100, 101, 102, 103), DEFAULT_REMOTE_PORT))
    );
}

#[tokio::test]
async fn tailnet_direct_is_refused_without_a_tailnet_self_address() {
    let lan_only = StaticTailnetProvider::with_addresses(&["192.168.1.20"]);
    let stub = UnconfiguredTailnetProvider;

    let lan_error = resolve_bind_address(
        RemoteExposureMode::TailnetDirect,
        DEFAULT_REMOTE_PORT,
        &lan_only,
    )
    .await
    .expect_err("a LAN address must never satisfy direct exposure");
    let stub_error = resolve_bind_address(
        RemoteExposureMode::TailnetDirect,
        DEFAULT_REMOTE_PORT,
        &stub,
    )
    .await
    .expect_err("the pre-1.6 stub provider must refuse direct exposure");

    assert_eq!(lan_error, RemoteBindError::TailnetAddressUnavailable);
    assert_eq!(stub_error, RemoteBindError::TailnetAddressUnavailable);
}

#[tokio::test]
async fn tailnet_direct_fails_closed_when_tailnet_status_cannot_be_read() {
    let provider = StaticTailnetProvider::unavailable();

    let error = resolve_bind_address(
        RemoteExposureMode::TailnetDirect,
        DEFAULT_REMOTE_PORT,
        &provider,
    )
    .await
    .expect_err("an unreadable tailnet status must not fall back to a bind");

    assert!(matches!(error, RemoteBindError::TailnetStatus(_)));
}

#[test]
fn bind_validation_refuses_wildcard_lan_loopback_and_foreign_tailnet_addresses() {
    let self_addresses: Vec<IpAddr> = vec!["100.101.102.103".parse().expect("valid address")];
    let refuse = |raw: &str| {
        validate_tailnet_bind_ip(raw.parse().expect("valid address"), &self_addresses)
            .expect_err("address must be refused")
    };

    assert_eq!(
        refuse("0.0.0.0"),
        RemoteBindError::WildcardRefused("0.0.0.0".parse().expect("valid address"))
    );
    assert_eq!(
        refuse("::"),
        RemoteBindError::WildcardRefused("::".parse().expect("valid address"))
    );
    assert_eq!(
        refuse("192.168.1.20"),
        RemoteBindError::NonTailnetAddressRefused("192.168.1.20".parse().expect("valid address"))
    );
    assert_eq!(
        refuse("10.0.0.5"),
        RemoteBindError::NonTailnetAddressRefused("10.0.0.5".parse().expect("valid address"))
    );
    assert_eq!(
        refuse("100.128.0.1"),
        RemoteBindError::NonTailnetAddressRefused("100.128.0.1".parse().expect("valid address"))
    );
    assert_eq!(
        refuse("127.0.0.1"),
        RemoteBindError::NonTailnetAddressRefused("127.0.0.1".parse().expect("valid address"))
    );
    assert_eq!(
        refuse("100.64.0.1"),
        RemoteBindError::ForeignTailnetAddressRefused("100.64.0.1".parse().expect("valid address"))
    );
    assert_eq!(
        validate_tailnet_bind_ip(
            "100.101.102.103".parse().expect("valid address"),
            &self_addresses
        ),
        Ok("100.101.102.103".parse().expect("valid address"))
    );
}

#[test]
fn remote_port_override_is_shape_validated_before_any_bind() {
    assert_eq!(parse_remote_port(None, DEFAULT_REMOTE_PORT), Ok(3849));
    assert_eq!(
        parse_remote_port(Some(" 4100 "), DEFAULT_REMOTE_PORT),
        Ok(4100)
    );
    assert_eq!(
        parse_remote_port(Some(""), DEFAULT_REMOTE_PORT),
        Err(RemotePortOverrideError::Empty)
    );
    assert_eq!(
        parse_remote_port(Some("0"), DEFAULT_REMOTE_PORT),
        Err(RemotePortOverrideError::Zero)
    );
    assert_eq!(
        parse_remote_port(Some("70000"), DEFAULT_REMOTE_PORT),
        Err(RemotePortOverrideError::NotAPort)
    );
    assert_eq!(
        parse_remote_port(Some("3849; rm -rf /"), DEFAULT_REMOTE_PORT),
        Err(RemotePortOverrideError::NotAPort)
    );
    assert_eq!(
        parse_remote_port(Some("0.0.0.0:3849"), DEFAULT_REMOTE_PORT),
        Err(RemotePortOverrideError::NotAPort)
    );
}

#[test]
fn effective_remote_port_falls_back_to_the_configured_port_without_an_override() {
    // The override env var is not set in the focused test environment; an ambient value would
    // be shape-validated by `parse_remote_port` before reaching a bind sink either way.
    if std::env::var(super::settings::REMOTE_PORT_ENV).is_ok() {
        return;
    }

    assert_eq!(
        effective_remote_port(DEFAULT_REMOTE_PORT),
        DEFAULT_REMOTE_PORT
    );
    assert_eq!(effective_remote_port(4321), 4321);
}

#[tokio::test]
async fn malformed_persisted_exposure_mode_is_rejected() {
    let db = SqliteTestDb::new("remote-host-settings-invalid-exposure-mode");
    db.with_connection(|conn| {
        conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
            .expect("test should permit seeding a restored malformed row");
        conn.execute(
            "INSERT INTO remote_host_settings
                (id, enabled, exposure_mode, port, environment_id)
             VALUES (1, 0, 'public_internet', 3849, ?1)",
            [Uuid::new_v4().to_string()],
        )
        .expect("malformed settings row should be seeded");
    });
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));

    let error = store
        .get()
        .await
        .expect_err("an unknown exposure mode must be rejected");

    assert!(matches!(error, crate::error::AppError::Database(message) if
        message.contains("invalid remote host exposure mode")));
}

#[tokio::test]
async fn zero_persisted_port_is_rejected_even_for_a_restored_database() {
    let db = SqliteTestDb::new("remote-host-settings-zero-port");
    db.with_connection(|conn| {
        conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
            .expect("test should permit seeding a restored malformed row");
        conn.execute(
            "INSERT INTO remote_host_settings
                (id, enabled, exposure_mode, port, environment_id)
             VALUES (1, 0, 'serve', 0, ?1)",
            [Uuid::new_v4().to_string()],
        )
        .expect("malformed settings row should be seeded");
    });
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));

    let error = store
        .get()
        .await
        .expect_err("port zero must never reach the bind sink");

    assert!(matches!(error, crate::error::AppError::Database(message) if
        message.contains("invalid remote host settings port: 0")));
}

#[tokio::test]
async fn malformed_persisted_environment_id_is_rejected() {
    let db = SqliteTestDb::new("remote-host-settings-invalid-environment-id");
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO remote_host_settings
                (id, enabled, exposure_mode, port, environment_id)
             VALUES (1, 0, 'serve', 3849, 'not-a-uuid')",
            [],
        )
        .expect("malformed settings row should be seeded");
    });
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));

    let error = store
        .get()
        .await
        .expect_err("an invalid host identity must be rejected");

    assert!(matches!(error, crate::error::AppError::Database(message) if
        message.contains("invalid remote host environment id")));
}

#[tokio::test]
async fn sqlite_read_errors_are_mapped_to_database_errors() {
    let db = SqliteTestDb::new("remote-host-settings-read-error");
    db.with_connection(|conn| {
        conn.execute("DROP TABLE remote_host_settings", [])
            .expect("test should remove the settings table");
    });
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));

    let error = store
        .get()
        .await
        .expect_err("sqlite read failures must be surfaced");

    assert!(matches!(error, crate::error::AppError::Database(message) if
        message.contains("no such table")));
}
