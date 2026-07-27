use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

pub(crate) const DEFAULT_REMOTE_PORT: u16 = 3849;
const SETTINGS_ROW_ID: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteExposureMode {
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
pub(crate) struct RemoteHostSettingsStore {
    db: DbConnection,
}

impl RemoteHostSettingsStore {
    pub(crate) fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub(crate) fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }

    /// Returns the singleton settings, creating the disabled default on first access.
    pub(crate) async fn get_or_create(&self) -> AppResult<RemoteHostSettings> {
        self.db
            .run_transaction(move |conn| {
                if let Some(settings) = read_settings(conn)? {
                    return Ok(settings);
                }

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

                read_settings(conn)?.ok_or_else(|| {
                    AppError::Database("remote host settings row missing after insert".to_string())
                })
            })
            .await
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
            let port = u16::try_from(port).map_err(|_| {
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
