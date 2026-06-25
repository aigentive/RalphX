use crate::error::AppResult;
use rusqlite::Connection;

/// Migration v65: Deduplicate and canonicalize working_directory values,
/// then add a UNIQUE partial index to prevent future duplicates.
///
/// # Steps
/// 1. Read all projects with a non-null working_directory
/// 2. For each row, attempt to canonicalize the path via `std::fs::canonicalize()`
///    - If canonicalization succeeds and the path differs, update the row
///    - If canonicalization fails (directory no longer exists), leave the row unchanged and warn
/// 3. Remove duplicate rows (keep the one with the lowest rowid for each working_directory)
/// 4. Add a UNIQUE partial index on `working_directory WHERE working_directory IS NOT NULL`
pub fn migrate(conn: &Connection) -> AppResult<()> {
    // Step 1: Load all projects with a working_directory
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT rowid, working_directory FROM projects WHERE working_directory IS NOT NULL",
            )
            .map_err(|e| crate::error::AppError::Database(e.to_string()))?;

        let result = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| crate::error::AppError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        result
    };

    // Step 2: Canonicalize and update non-canonical paths
    for (rowid, path) in &rows {
        match std::fs::canonicalize(path) {
            Ok(canonical) => {
                let canonical_str = canonical.to_string_lossy().into_owned();
                if canonical_str != *path {
                    conn.execute(
                        "UPDATE projects SET working_directory = ?1 WHERE rowid = ?2",
                        rusqlite::params![canonical_str, rowid],
                    )
                    .map_err(|e| crate::error::AppError::Database(e.to_string()))?;
                }
            }
            Err(e) => {
                tracing::warn!(
                    rowid = rowid,
                    path = %path,
                    error = %e,
                    "v65 migration: failed to canonicalize working_directory, leaving unchanged"
                );
            }
        }
    }

    // Step 3: Deduplicate — for each working_directory value, keep the lowest rowid
    // Delete all other rows with the same working_directory
    conn.execute_batch(
        "DELETE FROM projects
         WHERE working_directory IS NOT NULL
           AND rowid NOT IN (
               SELECT MIN(rowid)
               FROM projects
               WHERE working_directory IS NOT NULL
               GROUP BY working_directory
           )",
    )
    .map_err(|e| crate::error::AppError::Database(e.to_string()))?;

    // Step 4: Add UNIQUE partial index (NULL values are excluded — each project
    // without a working_directory is independent)
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_working_dir
         ON projects(working_directory)
         WHERE working_directory IS NOT NULL",
    )
    .map_err(|e| crate::error::AppError::Database(e.to_string()))?;

    Ok(())
}
