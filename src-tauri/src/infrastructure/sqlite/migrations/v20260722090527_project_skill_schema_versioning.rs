// Migration v20260722090527: project skill schema versioning

use rusqlite::Connection;
use serde_json::Value;

use super::v20260521222911_agent_plan_mode::{
    foreign_keys_enabled, legacy_alter_table_enabled, rewrite_table_check_constraint,
};
use crate::domain::entities::{
    project_skill_authorship_from_provenance, project_skill_content_hash,
    project_skill_evidence_hash_from_raw, project_skill_pipeline_role_from_provenance,
};
use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    let foreign_keys_was_enabled = foreign_keys_enabled(conn)?;
    let legacy_alter_table_was_enabled = legacy_alter_table_enabled(conn)?;

    conn.execute_batch("PRAGMA foreign_keys = OFF; PRAGMA legacy_alter_table = ON;")
        .map_err(db_error)?;
    if let Err(error) = conn.execute_batch("BEGIN IMMEDIATE") {
        let primary = AppError::Database(format!("BEGIN IMMEDIATE failed: {error}"));
        return restore_pragmas(
            conn,
            foreign_keys_was_enabled,
            legacy_alter_table_was_enabled,
            Err(primary),
        );
    }

    let transaction_result = match migrate_in_transaction(conn) {
        Ok(()) => conn
            .execute_batch("COMMIT")
            .map_err(|error| AppError::Database(format!("COMMIT failed: {error}"))),
        Err(primary) => {
            let rollback = conn.execute_batch("ROLLBACK");
            Err(match rollback {
                Ok(()) => primary,
                Err(error) => {
                    AppError::Database(format!("{primary}; additionally ROLLBACK failed: {error}"))
                }
            })
        }
    };

    restore_pragmas(
        conn,
        foreign_keys_was_enabled,
        legacy_alter_table_was_enabled,
        transaction_result,
    )
}

fn migrate_in_transaction(conn: &Connection) -> AppResult<()> {
    rewrite_table_check_constraint(
        conn,
        "project_skills",
        "'stale'",
        &[(
            "'staged', 'approved', 'rejected', 'archived', 'retired'",
            "'staged', 'approved', 'rejected', 'stale', 'archived', 'retired'",
        )],
        "Stale project skill lifecycle",
    )?;

    add_column_if_missing(
        conn,
        "project_skills",
        "content_hash",
        "ALTER TABLE project_skills ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        conn,
        "project_skills",
        "evidence_hash",
        "ALTER TABLE project_skills ADD COLUMN evidence_hash TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        conn,
        "project_skills",
        "created_by",
        "ALTER TABLE project_skills ADD COLUMN created_by TEXT NOT NULL DEFAULT 'agent' CHECK (created_by IN ('user', 'agent', 'imported'))",
    )?;
    add_column_if_missing(
        conn,
        "project_skills",
        "pipeline_role",
        "ALTER TABLE project_skills ADD COLUMN pipeline_role TEXT CHECK (pipeline_role IS NULL OR length(trim(pipeline_role)) > 0)",
    )?;

    backfill_project_skill_metadata(conn)?;
    create_version_table(conn)?;
    extend_settings(conn)?;
    ensure_foreign_keys_valid(conn)?;
    Ok(())
}

fn backfill_project_skill_metadata(conn: &Connection) -> AppResult<()> {
    let rows = {
        let mut statement = conn
            .prepare(
                "SELECT id, title, bucket, stage, body_markdown, provenance_json
                 FROM project_skills
                 WHERE content_hash = '' OR evidence_hash = ''",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        rows
    };

    for (id, title, bucket, stage, body, provenance_raw) in rows {
        let content_hash = project_skill_content_hash(&title, &bucket, &stage, &body);
        let evidence_hash = project_skill_evidence_hash_from_raw(&provenance_raw)?;
        let parsed = serde_json::from_str::<Value>(&provenance_raw).map_err(|error| {
            AppError::Database(format!(
                "invalid project_skills provenance_json during B1 backfill: {error}"
            ))
        })?;
        let created_by = project_skill_authorship_from_provenance(&parsed);
        let pipeline_role = project_skill_pipeline_role_from_provenance(&parsed);
        conn.execute(
            "UPDATE project_skills
             SET content_hash = ?2,
                 evidence_hash = ?3,
                 created_by = ?4,
                 pipeline_role = ?5
             WHERE id = ?1 AND (content_hash = '' OR evidence_hash = '')",
            rusqlite::params![
                id,
                content_hash,
                evidence_hash,
                created_by.to_string(),
                pipeline_role
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

fn create_version_table(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_skill_versions (
            project_skill_id TEXT NOT NULL REFERENCES project_skills(id) ON DELETE CASCADE,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            version INTEGER NOT NULL CHECK (version > 0),
            title TEXT NOT NULL,
            bucket TEXT NOT NULL,
            stage TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN (
                'staged', 'approved', 'rejected', 'stale', 'archived', 'retired'
            )),
            pinned INTEGER NOT NULL CHECK (pinned IN (0, 1)),
            archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
            scope_paths_json TEXT NOT NULL,
            compact_guidance TEXT NOT NULL,
            body_markdown TEXT NOT NULL,
            predicted_effect TEXT,
            provenance_json TEXT NOT NULL,
            companion_of_skill_id TEXT,
            content_hash TEXT NOT NULL,
            evidence_hash TEXT NOT NULL,
            created_by TEXT NOT NULL CHECK (created_by IN ('user', 'agent', 'imported')),
            pipeline_role TEXT CHECK (pipeline_role IS NULL OR length(trim(pipeline_role)) > 0),
            skill_created_at TEXT NOT NULL,
            skill_updated_at TEXT NOT NULL,
            snapshot_created_at TEXT NOT NULL,
            PRIMARY KEY (project_skill_id, version)
        );
        CREATE INDEX IF NOT EXISTS idx_project_skill_versions_project_skill
            ON project_skill_versions(project_skill_id, version DESC);",
    )
    .map_err(db_error)
}

fn extend_settings(conn: &Connection) -> AppResult<()> {
    for (column, sql) in [
        (
            "enabled",
            "ALTER TABLE project_skill_settings ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))",
        ),
        (
            "auto_inject",
            "ALTER TABLE project_skill_settings ADD COLUMN auto_inject INTEGER NOT NULL DEFAULT 1 CHECK (auto_inject IN (0, 1))",
        ),
        (
            "auto_distill",
            "ALTER TABLE project_skill_settings ADD COLUMN auto_distill INTEGER NOT NULL DEFAULT 1 CHECK (auto_distill IN (0, 1))",
        ),
        (
            "injection_max_skills",
            "ALTER TABLE project_skill_settings ADD COLUMN injection_max_skills INTEGER NOT NULL DEFAULT 4 CHECK (injection_max_skills > 0)",
        ),
        (
            "injection_max_chars",
            "ALTER TABLE project_skill_settings ADD COLUMN injection_max_chars INTEGER NOT NULL DEFAULT 6000 CHECK (injection_max_chars > 0)",
        ),
        (
            "injection_guidance_max_chars",
            "ALTER TABLE project_skill_settings ADD COLUMN injection_guidance_max_chars INTEGER NOT NULL DEFAULT 400 CHECK (injection_guidance_max_chars > 0)",
        ),
        (
            "report_min_outcomes",
            "ALTER TABLE project_skill_settings ADD COLUMN report_min_outcomes INTEGER NOT NULL DEFAULT 5 CHECK (report_min_outcomes > 0)",
        ),
        (
            "verification_corpus_gate",
            "ALTER TABLE project_skill_settings ADD COLUMN verification_corpus_gate INTEGER NOT NULL DEFAULT 0 CHECK (verification_corpus_gate >= 0)",
        ),
    ] {
        add_column_if_missing(conn, "project_skill_settings", column, sql)?;
    }
    Ok(())
}

fn add_column_if_missing(conn: &Connection, table: &str, column: &str, sql: &str) -> AppResult<()> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }
    conn.execute_batch(sql).map_err(db_error)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> AppResult<bool> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(db_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    Ok(columns.iter().any(|value| value == column))
}

fn ensure_foreign_keys_valid(conn: &Connection) -> AppResult<()> {
    let mut statement = conn.prepare("PRAGMA foreign_key_check").map_err(db_error)?;
    let mut rows = statement.query([]).map_err(db_error)?;
    if let Some(row) = rows.next().map_err(db_error)? {
        let table: String = row.get(0).map_err(db_error)?;
        return Err(AppError::Database(format!(
            "foreign key check failed for table {table}"
        )));
    }
    Ok(())
}

fn restore_pragmas(
    conn: &Connection,
    foreign_keys_was_enabled: bool,
    legacy_alter_table_was_enabled: bool,
    result: AppResult<()>,
) -> AppResult<()> {
    let restore = conn.execute_batch(&format!(
        "PRAGMA legacy_alter_table = {}; PRAGMA foreign_keys = {};",
        if legacy_alter_table_was_enabled {
            "ON"
        } else {
            "OFF"
        },
        if foreign_keys_was_enabled {
            "ON"
        } else {
            "OFF"
        },
    ));
    match (result, restore) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), Err(error)) => Err(AppError::Database(format!(
            "failed to restore migration PRAGMAs: {error}"
        ))),
        (Err(primary), Err(error)) => Err(AppError::Database(format!(
            "{primary}; additionally failed to restore migration PRAGMAs: {error}"
        ))),
    }
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}
