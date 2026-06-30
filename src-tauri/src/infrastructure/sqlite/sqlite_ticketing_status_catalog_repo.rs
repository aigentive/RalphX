use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::DbConnection;
use crate::domain::integrations::{
    TicketingStatusCatalogEntry, TicketingStatusCatalogRepository, TicketingStatusCatalogUpsert,
    TicketingStatusPresentationPatch,
};
use crate::error::{AppError, AppResult};

pub struct SqliteTicketingStatusCatalogRepository {
    db: DbConnection,
}

impl SqliteTicketingStatusCatalogRepository {
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

#[cfg(test)]
#[path = "sqlite_ticketing_status_catalog_repo_tests.rs"]
mod tests;

fn parse_datetime(raw: String) -> AppResult<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(&raw) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(&raw, "%Y-%m-%d %H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&ndt));
    }
    Err(AppError::Database(format!("invalid datetime: {raw}")))
}

fn parse_optional_datetime(raw: Option<String>) -> AppResult<Option<DateTime<Utc>>> {
    raw.map(parse_datetime).transpose()
}

fn now_text() -> String {
    Utc::now().to_rfc3339()
}

fn bool_from_i64(value: i64) -> bool {
    value != 0
}

fn i64_from_bool(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> AppResult<TicketingStatusCatalogEntry> {
    Ok(TicketingStatusCatalogEntry {
        id: row
            .get("id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        provider: row
            .get("provider")
            .map_err(|error| AppError::Database(error.to_string()))?,
        scope_kind: row
            .get("scope_kind")
            .map_err(|error| AppError::Database(error.to_string()))?,
        scope_id: row
            .get("scope_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        provider_status_id: row
            .get("provider_status_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        provider_status_name: row
            .get("provider_status_name")
            .map_err(|error| AppError::Database(error.to_string()))?,
        provider_category: row
            .get("provider_category")
            .map_err(|error| AppError::Database(error.to_string()))?,
        provider_color: row
            .get("provider_color")
            .map_err(|error| AppError::Database(error.to_string()))?,
        provider_order: row
            .get("provider_order")
            .map_err(|error| AppError::Database(error.to_string()))?,
        display_order: row
            .get("display_order")
            .map_err(|error| AppError::Database(error.to_string()))?,
        color_override: row
            .get("color_override")
            .map_err(|error| AppError::Database(error.to_string()))?,
        is_visible: bool_from_i64(
            row.get("is_visible")
                .map_err(|error| AppError::Database(error.to_string()))?,
        ),
        is_terminal: bool_from_i64(
            row.get("is_terminal")
                .map_err(|error| AppError::Database(error.to_string()))?,
        ),
        last_seen_at: parse_optional_datetime(
            row.get("last_seen_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
        stale_since: parse_optional_datetime(
            row.get("stale_since")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
        metadata_json: row
            .get("metadata_json")
            .map_err(|error| AppError::Database(error.to_string()))?,
        created_at: parse_datetime(
            row.get("created_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
        updated_at: parse_datetime(
            row.get("updated_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
    })
}

fn select_sql() -> &'static str {
    "SELECT id, provider, scope_kind, scope_id, provider_status_id, provider_status_name,
            provider_category, provider_color, provider_order, display_order, color_override,
            is_visible, is_terminal, last_seen_at, stale_since, metadata_json, created_at, updated_at
       FROM ticketing_status_catalog"
}

fn list_scope_entries(
    conn: &Connection,
    provider: &str,
    scope_kind: &str,
    scope_id: &str,
) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
    let mut stmt = conn.prepare(&format!(
        "{} WHERE provider = ?1 AND scope_kind = ?2 AND scope_id = ?3
         ORDER BY display_order ASC, provider_order ASC NULLS LAST, lower(provider_status_name) ASC, provider_status_id ASC",
        select_sql()
    ))?;
    let entries = stmt
        .query_map(params![provider, scope_kind, scope_id], |row| {
            row_to_entry(row).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

fn get_existing_id(
    conn: &Connection,
    input: &TicketingStatusCatalogUpsert,
) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM ticketing_status_catalog
         WHERE provider = ?1
           AND scope_kind = ?2
           AND scope_id = ?3
           AND provider_status_id = ?4
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![
        input.provider.as_str(),
        input.scope_kind.as_str(),
        input.scope_id.as_str(),
        input.provider_status_id.as_str(),
    ])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(row.get(0)?));
    }
    Ok(None)
}

#[async_trait]
impl TicketingStatusCatalogRepository for SqliteTicketingStatusCatalogRepository {
    async fn list_status_catalog(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        let provider = provider.to_string();
        let scope_kind = scope_kind.to_string();
        let scope_id = scope_id.to_string();
        self.db
            .run(move |conn| list_scope_entries(conn, &provider, &scope_kind, &scope_id))
            .await
    }

    async fn upsert_status_catalog_entry(
        &self,
        input: TicketingStatusCatalogUpsert,
    ) -> AppResult<TicketingStatusCatalogEntry> {
        self.db
            .run_transaction(move |conn| {
                let now = now_text();
                let id =
                    get_existing_id(conn, &input)?.unwrap_or_else(|| Uuid::new_v4().to_string());
                let last_seen_at = input.last_seen_at.to_rfc3339();
                if conn.execute(
                    "UPDATE ticketing_status_catalog
                     SET provider_status_name = ?5,
                         provider_category = ?6,
                         provider_color = ?7,
                         provider_order = ?8,
                         display_order = ?9,
                         is_terminal = ?10,
                         last_seen_at = ?11,
                         stale_since = NULL,
                         metadata_json = ?12,
                         updated_at = ?13
                     WHERE id = ?1
                       AND provider = ?2
                       AND scope_kind = ?3
                       AND scope_id = ?4",
                    params![
                        id.as_str(),
                        input.provider.as_str(),
                        input.scope_kind.as_str(),
                        input.scope_id.as_str(),
                        input.provider_status_name.as_str(),
                        input.provider_category.as_str(),
                        input.provider_color.as_deref(),
                        input.provider_order,
                        input.display_order,
                        i64_from_bool(input.is_terminal),
                        last_seen_at.as_str(),
                        input.metadata_json.as_deref(),
                        now.as_str(),
                    ],
                )? == 0
                {
                    conn.execute(
                        "INSERT INTO ticketing_status_catalog (
                            id, provider, scope_kind, scope_id, provider_status_id,
                            provider_status_name, provider_category, provider_color,
                            provider_order, display_order, is_visible, is_terminal,
                            last_seen_at, stale_since, metadata_json, created_at, updated_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11,
                            ?12, NULL, ?13, ?14, ?14
                        )",
                        params![
                            id.as_str(),
                            input.provider.as_str(),
                            input.scope_kind.as_str(),
                            input.scope_id.as_str(),
                            input.provider_status_id.as_str(),
                            input.provider_status_name.as_str(),
                            input.provider_category.as_str(),
                            input.provider_color.as_deref(),
                            input.provider_order,
                            input.display_order,
                            i64_from_bool(input.is_terminal),
                            last_seen_at.as_str(),
                            input.metadata_json.as_deref(),
                            now.as_str(),
                        ],
                    )?;
                }

                conn.query_row(
                    &format!("{} WHERE id = ?1", select_sql()),
                    [id.as_str()],
                    |row| {
                        row_to_entry(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                )
                .map_err(Into::into)
            })
            .await
    }

    async fn update_status_presentation(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
        patches: Vec<TicketingStatusPresentationPatch>,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        let provider = provider.to_string();
        let scope_kind = scope_kind.to_string();
        let scope_id = scope_id.to_string();
        self.db
            .run_transaction(move |conn| {
                for patch in patches {
                    let now = now_text();
                    if let Some(display_order) = patch.display_order {
                        conn.execute(
                            "UPDATE ticketing_status_catalog
                             SET display_order = ?5, updated_at = ?6
                             WHERE provider = ?1 AND scope_kind = ?2 AND scope_id = ?3 AND provider_status_id = ?4",
                            params![
                                provider.as_str(),
                                scope_kind.as_str(),
                                scope_id.as_str(),
                                patch.provider_status_id.as_str(),
                                display_order,
                                now.as_str()
                            ],
                        )?;
                    }
                    if let Some(color_override) = patch.color_override {
                        conn.execute(
                            "UPDATE ticketing_status_catalog
                             SET color_override = ?5, updated_at = ?6
                             WHERE provider = ?1 AND scope_kind = ?2 AND scope_id = ?3 AND provider_status_id = ?4",
                            params![
                                provider.as_str(),
                                scope_kind.as_str(),
                                scope_id.as_str(),
                                patch.provider_status_id.as_str(),
                                color_override.as_deref(),
                                now.as_str()
                            ],
                        )?;
                    }
                    if let Some(is_visible) = patch.is_visible {
                        conn.execute(
                            "UPDATE ticketing_status_catalog
                             SET is_visible = ?5, updated_at = ?6
                             WHERE provider = ?1 AND scope_kind = ?2 AND scope_id = ?3 AND provider_status_id = ?4",
                            params![
                                provider.as_str(),
                                scope_kind.as_str(),
                                scope_id.as_str(),
                                patch.provider_status_id.as_str(),
                                i64_from_bool(is_visible),
                                now.as_str()
                            ],
                        )?;
                    }
                }
                list_scope_entries(conn, &provider, &scope_kind, &scope_id)
            })
            .await
    }

    async fn mark_missing_statuses_stale(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
        observed_provider_status_ids: &[String],
        stale_since: DateTime<Utc>,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>> {
        let provider = provider.to_string();
        let scope_kind = scope_kind.to_string();
        let scope_id = scope_id.to_string();
        let observed: std::collections::HashSet<String> =
            observed_provider_status_ids.iter().cloned().collect();
        self.db
            .run_transaction(move |conn| {
                let entries = list_scope_entries(conn, &provider, &scope_kind, &scope_id)?;
                let stale_since = stale_since.to_rfc3339();
                for entry in entries {
                    if observed.contains(&entry.provider_status_id) {
                        continue;
                    }
                    conn.execute(
                        "UPDATE ticketing_status_catalog
                         SET stale_since = COALESCE(stale_since, ?5),
                             updated_at = ?6
                         WHERE provider = ?1 AND scope_kind = ?2 AND scope_id = ?3 AND provider_status_id = ?4",
                        params![
                            provider.as_str(),
                            scope_kind.as_str(),
                            scope_id.as_str(),
                            entry.provider_status_id.as_str(),
                            stale_since.as_str(),
                            now_text().as_str()
                        ],
                    )?;
                }
                list_scope_entries(conn, &provider, &scope_kind, &scope_id)
            })
            .await
    }
}
