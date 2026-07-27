use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

pub(crate) const DEFAULT_REMOTE_PORT: u16 = 3849;
/// Dev-parity override for the remote listener port, mirroring `BACKEND_PORT_ENV`.
pub(crate) const REMOTE_PORT_ENV: &str = "RALPHX_REMOTE_PORT";
const SETTINGS_ROW_ID: i64 = 1;
const LOOPBACK_BIND_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteExposureMode {
    Serve,
    TailnetDirect,
}

impl RemoteExposureMode {
    fn as_db_value(self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::TailnetDirect => "tailnet_direct",
        }
    }

    fn from_db_value(value: &str) -> AppResult<Self> {
        match value {
            "serve" => Ok(Self::Serve),
            "tailnet_direct" => Ok(Self::TailnetDirect),
            _ => Err(AppError::Database(format!(
                "invalid remote host exposure mode: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteHostSettings {
    pub enabled: bool,
    pub exposure_mode: RemoteExposureMode,
    pub port: u16,
    pub environment_id: String,
}

/// SQLite-backed singleton settings and stable host identity for remote access.
///
/// PR 1.4 adds the durable seq high-water to this same singleton row and must write it inside
/// the SAME `run_transaction` as each `remote_event_log` batch commit (§3.4 table note), so a
/// committed seq and its persisted high-water can never disagree.
pub(crate) struct RemoteHostSettingsStore {
    db: DbConnection,
}

impl RemoteHostSettingsStore {
    pub(crate) fn from_db(db: DbConnection) -> Self {
        Self { db }
    }

    /// Returns the singleton settings, creating the disabled default on first access.
    pub(crate) async fn get_or_create(&self) -> AppResult<RemoteHostSettings> {
        self.db
            .run_transaction(move |conn| ensure_settings_row(conn))
            .await
    }

    /// Reads the singleton settings without minting them.
    ///
    /// Startup auto-start uses this so a host that never configured remote access keeps an
    /// absent row (and therefore never listens).
    pub(crate) async fn get(&self) -> AppResult<Option<RemoteHostSettings>> {
        self.db.run(move |conn| read_settings(conn)).await
    }

    /// Persists the listener enablement flag, minting the settings row when absent.
    pub(crate) async fn set_enabled(&self, enabled: bool) -> AppResult<RemoteHostSettings> {
        self.db
            .run_transaction(move |conn| {
                ensure_settings_row(conn)?;
                conn.execute(
                    "UPDATE remote_host_settings
                     SET enabled = ?1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE id = ?2",
                    rusqlite::params![i64::from(enabled), SETTINGS_ROW_ID],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                read_settings_row(conn)
            })
            .await
    }

    /// Persists the exposure mode, minting the settings row when absent.
    pub(crate) async fn set_exposure_mode(
        &self,
        exposure_mode: RemoteExposureMode,
    ) -> AppResult<RemoteHostSettings> {
        self.db
            .run_transaction(move |conn| {
                ensure_settings_row(conn)?;
                conn.execute(
                    "UPDATE remote_host_settings
                     SET exposure_mode = ?1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE id = ?2",
                    rusqlite::params![exposure_mode.as_db_value(), SETTINGS_ROW_ID],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                read_settings_row(conn)
            })
            .await
    }
}

fn ensure_settings_row(conn: &Connection) -> AppResult<RemoteHostSettings> {
    if let Some(settings) = read_settings(conn)? {
        return Ok(settings);
    }

    // Permanent host identity. Pairings, per-environment client caches, and stream cursors all
    // bind to it (§3.1 descriptor, §3.2 "client discards any cursor whose environmentId ≠
    // hello.environmentId"), so a future "reset remote access" must NOT re-mint it — that would
    // silently orphan every paired device.
    let environment_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO remote_host_settings (
            id, enabled, exposure_mode, port, environment_id
         ) VALUES (?1, 0, ?2, ?3, ?4)",
        rusqlite::params![
            SETTINGS_ROW_ID,
            RemoteExposureMode::Serve.as_db_value(),
            i64::from(DEFAULT_REMOTE_PORT),
            environment_id,
        ],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    read_settings_row(conn)
}

fn read_settings_row(conn: &Connection) -> AppResult<RemoteHostSettings> {
    read_settings(conn)?
        .ok_or_else(|| AppError::Database("remote host settings row is missing".to_string()))
}

/// Failure to read the host's tailnet membership.
///
/// This type exists so a provider failure (CLI missing, daemon down, unparseable status) can
/// never be confused with "this host owns no tailnet address" — the logged-out case, which must
/// degrade to an empty address list rather than an error (§5.3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TailnetProviderError {
    /// Constructed by `infrastructure::tailscale`'s `tailscale status --json` provider.
    #[error("tailnet status is unavailable: {0}")]
    Unavailable(String),
}

/// Typed refusal reasons for the §4.4 bind-address policy.
///
/// Every variant is an in-code refusal: the listener never binds a wildcard, LAN, or
/// unverified address regardless of what the settings row or UI asked for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RemoteBindError {
    #[error("remote listener refuses the wildcard bind address {0}")]
    WildcardRefused(IpAddr),
    #[error("remote listener refuses the non-tailnet bind address {0}")]
    NonTailnetAddressRefused(IpAddr),
    #[error("remote listener refuses tailnet address {0}, which this host does not own")]
    ForeignTailnetAddressRefused(IpAddr),
    #[error("remote listener has no usable tailnet address for direct exposure")]
    TailnetAddressUnavailable,
    #[error(transparent)]
    TailnetStatus(#[from] TailnetProviderError),
}

/// Source of this host's own tailnet addresses.
///
/// Production implementation: `infrastructure::tailscale::TailscaleSelfAddressProvider` (PR 1.6
/// — `tailscale status --json` through the shared production CLI resolver). The seam stays so
/// the §4.4 bind policy is testable without a live tailnet.
#[async_trait::async_trait]
pub(crate) trait TailnetSelfAddressProvider: Send + Sync {
    async fn self_addresses(&self) -> Result<Vec<IpAddr>, TailnetProviderError>;
}

/// Stub provider reporting that this host has no tailnet addresses.
///
/// Test double since PR 1.6 wired the real `TailscaleSelfAddressProvider` into
/// production call sites; retained for listener/settings tests.
#[allow(dead_code)]
pub(crate) struct UnconfiguredTailnetProvider;

#[async_trait::async_trait]
impl TailnetSelfAddressProvider for UnconfiguredTailnetProvider {
    async fn self_addresses(&self) -> Result<Vec<IpAddr>, TailnetProviderError> {
        Ok(Vec::new())
    }
}

/// True when `address` falls inside the tailnet CGNAT range `100.64.0.0/10`.
pub(crate) fn is_tailnet_cgnat_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

/// Validates a candidate direct-exposure bind address against the §4.4 policy.
///
/// Refuses wildcard addresses, anything outside `100.64.0.0/10` (LAN, loopback, IPv6), and
/// CGNAT addresses this host does not actually own.
pub(crate) fn validate_tailnet_bind_ip(
    candidate: IpAddr,
    self_addresses: &[IpAddr],
) -> Result<IpAddr, RemoteBindError> {
    if candidate.is_unspecified() {
        return Err(RemoteBindError::WildcardRefused(candidate));
    }
    let IpAddr::V4(candidate_v4) = candidate else {
        return Err(RemoteBindError::NonTailnetAddressRefused(candidate));
    };
    if !is_tailnet_cgnat_ipv4(candidate_v4) {
        return Err(RemoteBindError::NonTailnetAddressRefused(candidate));
    }
    if !self_addresses.contains(&candidate) {
        return Err(RemoteBindError::ForeignTailnetAddressRefused(candidate));
    }
    Ok(candidate)
}

/// Computes the socket address the listener may bind for `exposure_mode`.
///
/// Serve mode is pinned to loopback; direct mode resolves and re-validates a CGNAT self
/// address. There is no code path that yields a wildcard or LAN address.
pub(crate) async fn resolve_bind_address(
    exposure_mode: RemoteExposureMode,
    port: u16,
    provider: &dyn TailnetSelfAddressProvider,
) -> Result<SocketAddr, RemoteBindError> {
    match exposure_mode {
        RemoteExposureMode::Serve => Ok(SocketAddr::from((LOOPBACK_BIND_IP, port))),
        RemoteExposureMode::TailnetDirect => {
            let self_addresses = provider.self_addresses().await?;
            let candidate = self_addresses
                .iter()
                .copied()
                .find(|address| match address {
                    IpAddr::V4(address) => is_tailnet_cgnat_ipv4(*address),
                    IpAddr::V6(_) => false,
                })
                .ok_or(RemoteBindError::TailnetAddressUnavailable)?;
            let validated = validate_tailnet_bind_ip(candidate, &self_addresses)?;
            Ok(SocketAddr::new(validated, port))
        }
    }
}

/// Typed rejection reasons for a malformed `RALPHX_REMOTE_PORT` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RemotePortOverrideError {
    #[error("value is empty")]
    Empty,
    #[error("value is not a valid u16 port")]
    NotAPort,
    #[error("port must be greater than zero")]
    Zero,
}

/// Shape-validates a raw `RALPHX_REMOTE_PORT` value before it can reach a bind sink.
pub(crate) fn parse_remote_port(
    raw: Option<&str>,
    configured_port: u16,
) -> Result<u16, RemotePortOverrideError> {
    let Some(raw) = raw else {
        return Ok(configured_port);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RemotePortOverrideError::Empty);
    }
    let port = trimmed
        .parse::<u16>()
        .map_err(|_| RemotePortOverrideError::NotAPort)?;
    if port == 0 {
        return Err(RemotePortOverrideError::Zero);
    }
    Ok(port)
}

/// Resolves the port the listener should use, honouring a validated env override.
///
/// An unparseable override is logged and ignored rather than propagated to the bind sink.
pub(crate) fn effective_remote_port(configured_port: u16) -> u16 {
    match std::env::var(REMOTE_PORT_ENV) {
        Ok(value) => match parse_remote_port(Some(value.as_str()), configured_port) {
            Ok(port) => port,
            Err(error) => {
                tracing::warn!(
                    env_var = REMOTE_PORT_ENV,
                    value = %value,
                    configured_port,
                    %error,
                    "Ignoring invalid remote listener port override"
                );
                configured_port
            }
        },
        Err(_) => configured_port,
    }
}

fn read_settings(conn: &Connection) -> AppResult<Option<RemoteHostSettings>> {
    let result = conn.query_row(
        "SELECT enabled, exposure_mode, port, environment_id
         FROM remote_host_settings
         WHERE id = ?1",
        [SETTINGS_ROW_ID],
        |row| {
            let enabled: i64 = row.get(0)?;
            let exposure_mode = row.get::<_, String>(1)?;
            let port = row.get::<_, i64>(2)?;
            let environment_id = row.get::<_, String>(3)?;
            Ok((enabled, exposure_mode, port, environment_id))
        },
    );

    match result {
        Ok((enabled, exposure_mode, port, environment_id)) => {
            let exposure_mode = RemoteExposureMode::from_db_value(&exposure_mode)?;
            let port = u16::try_from(port)
                .ok()
                // Port 0 reaches `TcpListener::bind` as "pick any ephemeral port" — on the
                // tailnet CGNAT address in direct mode. The migration CHECK blocks it, but the
                // bind sink needs sink-local proof (hand-restored DBs, `PRAGMA
                // ignore_check_constraints`, foreign-tool-created files), and the env override
                // path already rejects zero (`RemotePortOverrideError::Zero`).
                .filter(|port| *port != 0)
                .ok_or_else(|| {
                    AppError::Database(format!("invalid remote host settings port: {port}"))
                })?;
            Uuid::parse_str(&environment_id).map_err(|error| {
                AppError::Database(format!("invalid remote host environment id: {error}"))
            })?;
            Ok(Some(RemoteHostSettings {
                enabled: enabled != 0,
                exposure_mode,
                port,
                environment_id,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(AppError::Database(error.to_string())),
    }
}
