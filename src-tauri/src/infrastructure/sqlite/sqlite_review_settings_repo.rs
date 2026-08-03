use super::DbConnection;
use crate::domain::repositories::ReviewSettingsRepository;
use crate::domain::review::ReviewSettings;
use crate::error::AppError;
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SqliteReviewSettingsRepository {
    db: DbConnection,
}

impl SqliteReviewSettingsRepository {
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
impl ReviewSettingsRepository for SqliteReviewSettingsRepository {
    async fn get_settings(&self) -> Result<ReviewSettings, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT ai_review_enabled, ai_review_auto_fix, require_fix_approval,
                    require_human_review, max_fix_attempts, max_revision_cycles,
                    auto_create_followup_agent_conversation, require_workspace_review,
                    run_task_validations, autofix_workspace_review_blocking_findings,
                    workspace_review_fixer_cycle_cap
             FROM review_settings WHERE id = 1",
                    [],
                    |row| {
                        let ai_review_enabled: i64 = row.get(0)?;
                        let ai_review_auto_fix: i64 = row.get(1)?;
                        let require_fix_approval: i64 = row.get(2)?;
                        let require_human_review: i64 = row.get(3)?;
                        let max_fix_attempts: u32 = row.get(4)?;
                        let max_revision_cycles: u32 = row.get(5)?;
                        let auto_create_followup_agent_conversation: i64 = row.get(6)?;
                        let require_workspace_review: i64 = row.get(7)?;
                        let run_task_validations: i64 = row.get(8)?;
                        let autofix_workspace_review_blocking_findings: i64 = row.get(9)?;
                        let workspace_review_fixer_cycle_cap: i64 = row.get(10)?;

                        Ok(ReviewSettings {
                            ai_review_enabled: ai_review_enabled != 0,
                            ai_review_auto_fix: ai_review_auto_fix != 0,
                            require_fix_approval: require_fix_approval != 0,
                            require_human_review: require_human_review != 0,
                            require_workspace_review: require_workspace_review != 0,
                            autofix_workspace_review_blocking_findings:
                                autofix_workspace_review_blocking_findings != 0,
                            workspace_review_fixer_cycle_cap,
                            run_task_validations: run_task_validations != 0,
                            max_fix_attempts,
                            max_revision_cycles,
                            auto_create_followup_agent_conversation:
                                auto_create_followup_agent_conversation != 0,
                        })
                    },
                );

                match result {
                    Ok(settings) => Ok(settings),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ReviewSettings::default()),
                    Err(e) => Err(AppError::Database(e.to_string())),
                }
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn update_settings(
        &self,
        settings: &ReviewSettings,
    ) -> Result<ReviewSettings, Box<dyn std::error::Error>> {
        let settings = settings.clone();

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE review_settings
             SET ai_review_enabled = ?1,
                 ai_review_auto_fix = ?2,
                 require_fix_approval = ?3,
                 require_human_review = ?4,
                 max_fix_attempts = ?5,
                 max_revision_cycles = ?6,
                 auto_create_followup_agent_conversation = ?7,
                 require_workspace_review = ?8,
                 run_task_validations = ?9,
                 autofix_workspace_review_blocking_findings = ?10,
                 workspace_review_fixer_cycle_cap = ?11,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
             WHERE id = 1",
                    rusqlite::params![
                        settings.ai_review_enabled as i64,
                        settings.ai_review_auto_fix as i64,
                        settings.require_fix_approval as i64,
                        settings.require_human_review as i64,
                        settings.max_fix_attempts,
                        settings.max_revision_cycles,
                        settings.auto_create_followup_agent_conversation as i64,
                        settings.require_workspace_review as i64,
                        settings.run_task_validations as i64,
                        settings.autofix_workspace_review_blocking_findings as i64,
                        settings.workspace_review_fixer_cycle_cap,
                    ],
                )?;
                Ok(settings)
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}

#[cfg(test)]
#[path = "sqlite_review_settings_repo_tests.rs"]
mod tests;
