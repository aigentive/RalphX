// Migration helpers for safe schema modifications
//
// These helpers ensure migrations are idempotent and safe to re-run.

use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Check if a column exists in a table
pub fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let rows = match stmt.query_map([], |row| row.get::<_, String>(1)) {
        Ok(r) => r,
        Err(_) => return false,
    };
    for row in rows.flatten() {
        if row == column {
            return true;
        }
    }
    false
}

/// Check if a table exists
pub fn table_exists(conn: &Connection, table: &str) -> bool {
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )
        .unwrap_or(0);
    count > 0
}

/// Check if an index exists
pub fn index_exists(conn: &Connection, index: &str) -> bool {
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
            [index],
            |row| row.get(0),
        )
        .unwrap_or(0);
    count > 0
}

/// Add column if it doesn't exist (SQLite doesn't support IF NOT EXISTS for columns)
pub fn add_column_if_not_exists(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> AppResult<()> {
    if !column_exists(conn, table, column) {
        let sql = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, definition);
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(e.to_string()))?;
    }
    Ok(())
}

/// Create index if it doesn't exist
pub fn create_index_if_not_exists(
    conn: &Connection,
    index_name: &str,
    table: &str,
    columns: &str,
) -> AppResult<()> {
    let sql = format!(
        "CREATE INDEX IF NOT EXISTS {} ON {}({})",
        index_name, table, columns
    );
    conn.execute(&sql, [])
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

/// Violation counts keyed by `(child table, parent table, foreign key id)`.
///
/// Keying on the constraint rather than the offending rowid keeps the baseline
/// comparison stable across the table rewrites this migration performs, which
/// renumber rowids.
pub(super) type ForeignKeyViolationCounts = BTreeMap<(String, String, i64), i64>;

pub(super) fn foreign_key_violation_counts(
    conn: &Connection,
) -> AppResult<ForeignKeyViolationCounts> {
    let mut statement = conn
        .prepare(
            "SELECT \"table\", \"parent\", \"fkid\", COUNT(*)
             FROM pragma_foreign_key_check
             GROUP BY \"table\", \"parent\", \"fkid\"",
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                (
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ),
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| AppError::Database(error.to_string()))?;

    let mut counts = ForeignKeyViolationCounts::new();
    for row in rows {
        let (key, count) = row.map_err(|error| AppError::Database(error.to_string()))?;
        counts.insert(key, count);
    }
    Ok(counts)
}

/// Reports only violations this migration added, so orphan rows that already
/// existed cannot be attributed to it.
pub(super) fn introduced_violations(
    baseline: &ForeignKeyViolationCounts,
    after: &ForeignKeyViolationCounts,
) -> Vec<(String, String, i64)> {
    after
        .iter()
        .filter(|(key, count)| **count > baseline.get(*key).copied().unwrap_or(0))
        .map(|((table, parent, fkid), count)| {
            (
                table.clone(),
                parent.clone(),
                count
                    - baseline
                        .get(&(table.clone(), parent.clone(), *fkid))
                        .copied()
                        .unwrap_or(0),
            )
        })
        .collect()
}
