// Migration v20260701152000: normalize global Workspace Review runtime settings scope.

use rusqlite::Connection;

use super::helpers::table_exists;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    if !table_exists(conn, "workspace_review_runtime_settings") {
        return Ok(());
    }

    conn.execute_batch(
        "DELETE FROM workspace_review_runtime_settings
         WHERE scope_type = 'global'
           AND id NOT IN (
             SELECT keep_id
             FROM (
               SELECT MAX(id) AS keep_id
               FROM workspace_review_runtime_settings
               WHERE scope_type = 'global'
               GROUP BY provider
             )
           );
         UPDATE workspace_review_runtime_settings
         SET scope_id = ''
         WHERE scope_type = 'global';
         CREATE UNIQUE INDEX IF NOT EXISTS idx_workspace_review_runtime_settings_global_provider
             ON workspace_review_runtime_settings(provider)
             WHERE scope_type = 'global';",
    )?;

    Ok(())
}
