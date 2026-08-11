// Migration v20260716170840: persona project scope

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(conn, "personas", "project_id", "TEXT NULL")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_personas_project_id ON personas(project_id);
         DROP INDEX IF EXISTS idx_personas_slug_live;
         CREATE UNIQUE INDEX IF NOT EXISTS personas_active_slug_scoped
             ON personas(slug, IFNULL(project_id, ''))
             WHERE status = 'active';",
    )?;
    Ok(())
}
