use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::error::{AppError, AppResult};

/// Byte size of one payload row. `COALESCE` keeps NULL columns from poisoning the sum.
const PAYLOAD_BYTES_EXPR: &str = "LENGTH(COALESCE(payload.input_json, '')) \
     + LENGTH(COALESCE(payload.result_json, '')) \
     + LENGTH(COALESCE(payload.raw_block_json, ''))";

/// `(block.created_at, block.id)` of the last row a prune loop consumed.
///
/// Without it every batch re-walks the already-pruned prefix of
/// `idx_chat_message_blocks_created_at`, making a large drain quadratic.
pub type PruneCursor = Option<(String, String)>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneBatchOutcome {
    pub deleted_rows: u64,
    /// Bytes freed by this batch, measured from the bounded selection — never a table scan.
    pub payload_bytes: u64,
    pub next_cursor: PruneCursor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PayloadUsage {
    pub total_bytes: u64,
    pub row_count: u64,
}

/// What a size-budget prune would delete, computed without writing anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeBudgetPreview {
    pub rows: u64,
    pub bytes: u64,
    pub cut_created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalCheckpointOutcome {
    Truncated,
    Busy,
}

/// Deletes payload-only rows. Timeline hydration treats a missing row as absent payload data.
pub struct SqliteChatPayloadRetentionRepository {
    db: DbConnection,
}

impl SqliteChatPayloadRetentionRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }

    pub fn from_db(db: DbConnection) -> Self {
        Self { db }
    }

    /// Deletes one batch of payloads that fell outside the retention windows.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the batch selection or delete fails.
    pub async fn prune_batch(
        &self,
        before: DateTime<Utc>,
        archived_before: DateTime<Utc>,
        batch_rows: u32,
        cursor: PruneCursor,
    ) -> AppResult<PruneBatchOutcome> {
        let before = before.to_rfc3339();
        let archived_before = archived_before.to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                let (cursor_created_at, cursor_id) = split_cursor(&cursor);
                let mut statement = conn.prepare(
                    &format!(
                        r#"
                        SELECT payload.block_id, block.created_at, {PAYLOAD_BYTES_EXPR} AS payload_bytes
                        FROM chat_message_block_payloads AS payload
                        INNER JOIN chat_message_blocks AS block ON block.id = payload.block_id
                        INNER JOIN chat_conversations AS conversation ON conversation.id = block.conversation_id
                        WHERE ((conversation.archived_at IS NULL AND block.created_at < ?1)
                            OR (conversation.archived_at IS NOT NULL AND block.created_at < ?2))
                          AND (?4 IS NULL OR (block.created_at, block.id) > (?4, ?5))
                        ORDER BY block.created_at ASC, block.id ASC
                        LIMIT ?3
                        "#
                    ),
                )?;
                let selected = collect_batch(&mut statement, params![
                    before,
                    archived_before,
                    batch_rows,
                    cursor_created_at,
                    cursor_id
                ])?;
                delete_selected(conn, selected)
            })
            .await
    }

    /// Deletes the oldest batch of payloads regardless of age — size-budget enforcement.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the batch selection or delete fails.
    pub async fn prune_oldest_batch(
        &self,
        batch_rows: u32,
        cursor: PruneCursor,
    ) -> AppResult<PruneBatchOutcome> {
        self.db
            .run_transaction(move |conn| {
                let (cursor_created_at, cursor_id) = split_cursor(&cursor);
                let mut statement = conn.prepare(&format!(
                    r#"
                    SELECT payload.block_id, block.created_at, {PAYLOAD_BYTES_EXPR} AS payload_bytes
                    FROM chat_message_block_payloads AS payload
                    INNER JOIN chat_message_blocks AS block ON block.id = payload.block_id
                    WHERE (?2 IS NULL OR (block.created_at, block.id) > (?2, ?3))
                    ORDER BY block.created_at ASC, block.id ASC
                    LIMIT ?1
                    "#
                ))?;
                let selected = collect_batch(
                    &mut statement,
                    params![batch_rows, cursor_created_at, cursor_id],
                )?;
                delete_selected(conn, selected)
            })
            .await
    }

    /// One bounded slice of the payload measurement, keyset-paginated on `block_id` (the payload
    /// table's own key, so no join is needed and the walk is stable under concurrent deletes).
    ///
    /// Returns the slice's partial usage plus the cursor to resume from, or `None` at the end.
    /// Callers sum the slices; each call holds a pooled connection for one bounded read instead of
    /// the whole table.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the batch query fails.
    pub async fn payload_usage_batch(
        &self,
        batch_rows: u32,
        cursor: Option<String>,
    ) -> AppResult<(PayloadUsage, Option<String>)> {
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(&format!(
                    r#"
                    SELECT payload.block_id, {PAYLOAD_BYTES_EXPR} AS payload_bytes
                    FROM chat_message_block_payloads AS payload
                    WHERE (?2 IS NULL OR payload.block_id > ?2)
                    ORDER BY payload.block_id ASC
                    LIMIT ?1
                    "#
                ))?;
                let mut rows = statement.query(params![batch_rows, cursor])?;
                let mut usage = PayloadUsage::default();
                let mut last_block_id: Option<String> = None;
                while let Some(row) = rows.next()? {
                    last_block_id = Some(row.get::<_, String>(0)?);
                    usage.total_bytes += row.get::<_, i64>(1)?.max(0) as u64;
                    usage.row_count += 1;
                }
                // A short batch means the walk is done; only a full batch can have more behind it.
                let next_cursor = if usage.row_count == u64::from(batch_rows) {
                    last_block_id
                } else {
                    None
                };
                Ok((usage, next_cursor))
            })
            .await
    }

    /// Full payload scan in a single statement. Multi-GB of reads on a large database, held for
    /// the whole query — production measurement goes through [`Self::payload_usage_batch`]; this
    /// remains as the equivalence oracle for tests.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the aggregate query fails.
    pub async fn payload_usage(&self) -> AppResult<PayloadUsage> {
        self.db
            .run(move |conn| {
                let (row_count, total_bytes) = conn.query_row(
                    &format!(
                        "SELECT COUNT(*), COALESCE(SUM({PAYLOAD_BYTES_EXPR}), 0)
                         FROM chat_message_block_payloads AS payload"
                    ),
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )?;
                Ok(PayloadUsage {
                    total_bytes: total_bytes.max(0) as u64,
                    row_count: row_count.max(0) as u64,
                })
            })
            .await
    }

    /// Constant-time upper bound on payload bytes, used to skip [`Self::payload_usage`]
    /// entirely when the whole database is already under budget.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the page pragmas cannot be read.
    pub async fn database_size_hint(&self) -> AppResult<u64> {
        self.db
            .run(move |conn| {
                let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
                let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
                Ok((page_count.max(0) as u64).saturating_mul(page_size.max(0) as u64))
            })
            .await
    }

    /// Bytes already returned to the SQLite freelist — space a compaction would reclaim.
    ///
    /// Deleting rows never shrinks `ralphx.db`; this is what tells the user the job is
    /// only half done.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the page pragmas cannot be read.
    pub async fn reclaimable_hint_bytes(&self) -> AppResult<u64> {
        self.db
            .run(move |conn| {
                let freelist_count: i64 =
                    conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
                let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
                Ok((freelist_count.max(0) as u64).saturating_mul(page_size.max(0) as u64))
            })
            .await
    }

    /// Read-only projection of a size-budget prune: what would be deleted, and up to when.
    ///
    /// Costs about one [`Self::payload_usage`] scan, so it is user-initiated only.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the payload walk fails.
    pub async fn preview_size_budget_prune(&self, budget: u64) -> AppResult<SizeBudgetPreview> {
        self.db
            .run(move |conn| {
                let total_bytes: i64 = conn.query_row(
                    &format!(
                        "SELECT COALESCE(SUM({PAYLOAD_BYTES_EXPR}), 0)
                         FROM chat_message_block_payloads AS payload"
                    ),
                    [],
                    |row| row.get(0),
                )?;
                let total_bytes = total_bytes.max(0) as u64;
                if total_bytes <= budget {
                    return Ok(SizeBudgetPreview::default());
                }

                let mut surviving = total_bytes;
                let mut preview = SizeBudgetPreview::default();
                let mut statement = conn.prepare(&format!(
                    r#"
                    SELECT block.created_at, {PAYLOAD_BYTES_EXPR} AS payload_bytes
                    FROM chat_message_block_payloads AS payload
                    INNER JOIN chat_message_blocks AS block ON block.id = payload.block_id
                    ORDER BY block.created_at ASC, block.id ASC
                    "#
                ))?;
                let mut rows = statement.query([])?;
                while surviving > budget {
                    let Some(row) = rows.next()? else { break };
                    let created_at: String = row.get(0)?;
                    let bytes = row.get::<_, i64>(1)?.max(0) as u64;
                    surviving = surviving.saturating_sub(bytes);
                    preview.rows += 1;
                    preview.bytes = preview.bytes.saturating_add(bytes);
                    preview.cut_created_at = parse_timestamp(&created_at);
                }
                Ok(preview)
            })
            .await
    }

    /// Truncating WAL checkpoint. Mass deletes under WAL otherwise grow `ralphx.db-wal`
    /// without bound, turning cleanup into the disk-pressure event it exists to prevent.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the pragma itself fails; a *busy* checkpoint is a
    /// successful [`WalCheckpointOutcome::Busy`], not an error.
    pub async fn checkpoint_truncate(&self) -> AppResult<WalCheckpointOutcome> {
        self.db
            .run(move |conn| {
                let busy: i64 = conn
                    .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
                    .map_err(|error| {
                        AppError::Database(format!("WAL checkpoint failed: {error}"))
                    })?;
                Ok(if busy == 0 {
                    WalCheckpointOutcome::Truncated
                } else {
                    WalCheckpointOutcome::Busy
                })
            })
            .await
    }
}

type SelectedBatch = Vec<(String, String, u64)>;

fn split_cursor(cursor: &PruneCursor) -> (Option<String>, Option<String>) {
    match cursor {
        Some((created_at, id)) => (Some(created_at.clone()), Some(id.clone())),
        None => (None, None),
    }
}

fn collect_batch(
    statement: &mut rusqlite::Statement<'_>,
    parameters: &[&dyn rusqlite::ToSql],
) -> AppResult<SelectedBatch> {
    let rows = statement.query_map(parameters, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?.max(0) as u64,
        ))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn delete_selected(conn: &Connection, selected: SelectedBatch) -> AppResult<PruneBatchOutcome> {
    let Some((last_block_id, last_created_at, _)) = selected.last().cloned() else {
        return Ok(PruneBatchOutcome::default());
    };

    let placeholders = vec!["?"; selected.len()].join(", ");
    let deleted = conn.execute(
        &format!("DELETE FROM chat_message_block_payloads WHERE block_id IN ({placeholders})"),
        params_from_iter(selected.iter().map(|(block_id, _, _)| block_id)),
    )?;

    Ok(PruneBatchOutcome {
        deleted_rows: deleted as u64,
        payload_bytes: selected.iter().map(|(_, _, bytes)| bytes).sum(),
        next_cursor: Some((last_created_at, last_block_id)),
    })
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}
