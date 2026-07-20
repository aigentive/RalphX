// Migration v20260717152714: persona artifact history

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(conn, "personas", "artifact_id", "TEXT NULL")?;
    conn.execute_batch(
        r#"BEGIN IMMEDIATE;

           INSERT OR IGNORE INTO artifact_buckets (id, name, config_json, is_system)
           VALUES (
               'persona-library',
               'Persona Library',
               '{"accepted_types":["persona"],"writers":["agent","user","system"],"readers":["all"]}',
               1
           );

           INSERT OR IGNORE INTO artifacts (
               id, type, name, content_type, content_text, bucket_id, created_by,
               version, metadata_json, created_at
           )
           SELECT
               'persona-artifact-' || id,
               'persona',
               name,
               'inline',
               content,
               'persona-library',
               'backfill',
               1,
               json_object('persona_version', version, 'created_by', 'backfill'),
               created_at
           FROM personas
           WHERE artifact_id IS NULL;

           UPDATE personas
           SET artifact_id = 'persona-artifact-' || id
           WHERE artifact_id IS NULL;

           COMMIT;"#,
    )?;
    Ok(())
}
