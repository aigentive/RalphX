use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::repositories::{McpCatalogSnapshot, McpCatalogSnapshotRepository};
use crate::error::{AppError, AppResult};

use super::DbConnection;

pub struct SqliteMcpCatalogSnapshotRepository {
    db: DbConnection,
}

impl SqliteMcpCatalogSnapshotRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl McpCatalogSnapshotRepository for SqliteMcpCatalogSnapshotRepository {
    async fn get(
        &self,
        scope_project_id: Option<&str>,
        provider: &str,
    ) -> AppResult<Option<McpCatalogSnapshot>> {
        let scope_project_id = scope_project_id.map(str::to_string);
        let provider = provider.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT scope_project_id, provider, response_json, captured_at
                     FROM mcp_catalog_snapshot
                     WHERE scope_project_id IS ?1 AND provider = ?2",
                    rusqlite::params![scope_project_id, provider],
                    |row| {
                        Ok(McpCatalogSnapshot {
                            scope_project_id: row.get(0)?,
                            provider: row.get(1)?,
                            response_json: row.get(2)?,
                            captured_at: row.get(3)?,
                        })
                    },
                )
                .optional()
                .map_err(|error| AppError::Database(error.to_string()))
            })
            .await
    }

    async fn upsert(&self, snapshot: McpCatalogSnapshot) -> AppResult<McpCatalogSnapshot> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO mcp_catalog_snapshot
                        (scope_project_id, provider, response_json, captured_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT DO UPDATE SET
                        response_json = excluded.response_json,
                        captured_at = excluded.captured_at",
                    rusqlite::params![
                        snapshot.scope_project_id,
                        snapshot.provider,
                        snapshot.response_json,
                        snapshot.captured_at,
                    ],
                )?;
                Ok(snapshot)
            })
            .await
    }
}
