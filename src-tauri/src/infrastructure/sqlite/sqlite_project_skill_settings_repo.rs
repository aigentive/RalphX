use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{types::Type, Connection, OptionalExtension};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::{ProjectId, ProjectSkillSettings, ProjectSkillSettingsPatch};
use crate::domain::repositories::ProjectSkillSettingsRepository;
use crate::error::AppResult;

pub struct SqliteProjectSkillSettingsRepository {
    db: DbConnection,
}

impl SqliteProjectSkillSettingsRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

fn settings_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSkillSettings> {
    Ok(ProjectSkillSettings {
        project_id: ProjectId::from_string(row.get::<_, String>(0)?),
        enabled: boolean_from_row(row, 1, "enabled")?,
        auto_inject: boolean_from_row(row, 2, "auto_inject")?,
        auto_distill: boolean_from_row(row, 3, "auto_distill")?,
        injection_max_skills: row.get(4)?,
        injection_max_chars: row.get(5)?,
        injection_guidance_max_chars: row.get(6)?,
        report_min_outcomes: row.get(7)?,
        verification_corpus_gate: row.get(8)?,
        export_enabled: boolean_from_row(row, 9, "export_enabled")?,
    })
}

fn boolean_from_row(
    row: &rusqlite::Row<'_>,
    index: usize,
    column: &'static str,
) -> rusqlite::Result<bool> {
    match row.get::<_, i64>(index)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid project skill settings {column}: {value}"),
            )),
        )),
    }
}

const SETTINGS_COLUMNS: &str =
    "project_id, enabled, auto_inject, auto_distill, injection_max_skills,
     injection_max_chars, injection_guidance_max_chars, report_min_outcomes,
     verification_corpus_gate, export_enabled";

fn save_settings(conn: &Connection, settings: &ProjectSkillSettings) -> AppResult<()> {
    conn.execute(
        "INSERT INTO project_skill_settings (
            project_id, enabled, auto_inject, auto_distill, injection_max_skills,
            injection_max_chars, injection_guidance_max_chars, report_min_outcomes,
            verification_corpus_gate, export_enabled, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
            CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
         )
         ON CONFLICT(project_id)
         DO UPDATE SET
            enabled = excluded.enabled,
            auto_inject = excluded.auto_inject,
            auto_distill = excluded.auto_distill,
            injection_max_skills = excluded.injection_max_skills,
            injection_max_chars = excluded.injection_max_chars,
            injection_guidance_max_chars = excluded.injection_guidance_max_chars,
            report_min_outcomes = excluded.report_min_outcomes,
            verification_corpus_gate = excluded.verification_corpus_gate,
            export_enabled = excluded.export_enabled,
            updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![
            settings.project_id.as_str(),
            i64::from(settings.enabled),
            i64::from(settings.auto_inject),
            i64::from(settings.auto_distill),
            settings.injection_max_skills,
            settings.injection_max_chars,
            settings.injection_guidance_max_chars,
            settings.report_min_outcomes,
            settings.verification_corpus_gate,
            i64::from(settings.export_enabled),
        ],
    )?;
    Ok(())
}

#[async_trait]
impl ProjectSkillSettingsRepository for SqliteProjectSkillSettingsRepository {
    async fn get_for_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Option<ProjectSkillSettings>> {
        let project_id = project_id.as_str().to_string();
        let settings = self
            .db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT {SETTINGS_COLUMNS} FROM project_skill_settings WHERE project_id = ?1"
                    ),
                    [project_id],
                    settings_from_row,
                )
            })
            .await?;
        if let Some(settings) = &settings {
            settings.validate()?;
        }
        Ok(settings)
    }

    async fn upsert(&self, settings: ProjectSkillSettings) -> AppResult<ProjectSkillSettings> {
        settings.validate()?;
        let saved = settings.clone();
        let saved_for_write = saved.clone();
        self.db
            .run(move |conn| {
                save_settings(conn, &saved_for_write)?;
                Ok(())
            })
            .await?;
        Ok(saved)
    }

    async fn patch(
        &self,
        project_id: &ProjectId,
        patch: ProjectSkillSettingsPatch,
    ) -> AppResult<ProjectSkillSettings> {
        patch.validate()?;
        let project_id = project_id.clone();
        self.db
            .run_transaction(move |conn| {
                let mut settings = conn
                    .query_row(
                        &format!(
                            "SELECT {SETTINGS_COLUMNS} FROM project_skill_settings WHERE project_id = ?1"
                        ),
                        [project_id.as_str()],
                        settings_from_row,
                    )
                    .optional()?
                    .unwrap_or_else(|| ProjectSkillSettings::default_for_project(project_id));
                settings.validate()?;
                patch.apply_to(&mut settings);
                settings.validate()?;
                save_settings(conn, &settings)?;
                Ok(settings)
            })
            .await
    }
}
