use super::DbConnection;
use crate::domain::ideation::{
    ExternalIdeationOverrides, IdeationPlanMode, IdeationSettings, TasksFeatureAction,
    TasksFeatureState,
};
use crate::domain::repositories::IdeationSettingsRepository;
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SqliteIdeationSettingsRepository {
    db: DbConnection,
}

impl SqliteIdeationSettingsRepository {
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

fn parse_ideation_settings_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IdeationSettings> {
    let plan_mode_str: String = row.get(0)?;
    let require_plan_approval: i64 = row.get(1)?;
    let suggest_plans_for_complex: i64 = row.get(2)?;
    let auto_link_proposals: i64 = row.get(3)?;
    let require_verification_for_accept: i64 = row.get(4)?;
    let require_verification_for_proposals: i64 = row.get(5)?;
    let require_accept_for_finalize: Option<i64> = row.get(6)?;
    let ext_require_verification_for_accept: Option<i64> = row.get(7)?;
    let ext_require_verification_for_proposals: Option<i64> = row.get(8)?;
    let ext_require_accept_for_finalize: Option<i64> = row.get(9)?;
    let auto_verify_plans: i64 = row.get(10)?;
    let ext_auto_verify_plans: Option<i64> = row.get(11)?;
    let _legacy_tasks_enabled: i64 = row.get(12)?;
    let tasks_feature_state_raw: String = row.get(13)?;
    let tasks_feature_state: TasksFeatureState =
        tasks_feature_state_raw.parse().map_err(|error: String| {
            rusqlite::Error::FromSqlConversionFailure(
                13,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        })?;
    let auto_verify_draft_plans: i64 = row.get(14)?;

    let plan_mode = match plan_mode_str.as_str() {
        "required" => IdeationPlanMode::Required,
        "optional" => IdeationPlanMode::Optional,
        "parallel" => IdeationPlanMode::Parallel,
        _ => IdeationPlanMode::default(),
    };

    Ok(IdeationSettings {
        tasks_enabled: tasks_feature_state.tasks_enabled(),
        tasks_feature_state,
        plan_mode,
        require_plan_approval: require_plan_approval != 0,
        suggest_plans_for_complex: suggest_plans_for_complex != 0,
        auto_link_proposals: auto_link_proposals != 0,
        auto_verify_plans: auto_verify_plans != 0,
        auto_verify_draft_plans: auto_verify_draft_plans != 0,
        require_verification_for_accept: require_verification_for_accept != 0,
        require_verification_for_proposals: require_verification_for_proposals != 0,
        require_accept_for_finalize: require_accept_for_finalize.map(|v| v != 0).unwrap_or(false),
        external_overrides: ExternalIdeationOverrides {
            auto_verify_plans: ext_auto_verify_plans.map(|v| v != 0),
            require_verification_for_accept: ext_require_verification_for_accept.map(|v| v != 0),
            require_verification_for_proposals: ext_require_verification_for_proposals
                .map(|v| v != 0),
            require_accept_for_finalize: ext_require_accept_for_finalize.map(|v| v != 0),
        },
    })
}

/// Synchronous helper for reading settings inside a transaction closure.
#[doc(hidden)]
pub fn get_settings_sync(conn: &Connection) -> AppResult<IdeationSettings> {
    let result = conn.query_row(
        "SELECT plan_mode, require_plan_approval, suggest_plans_for_complex, auto_link_proposals,
                require_verification_for_accept, require_verification_for_proposals,
                require_accept_for_finalize,
                ext_require_verification_for_accept,
                ext_require_verification_for_proposals,
                ext_require_accept_for_finalize,
                auto_verify_plans,
                ext_auto_verify_plans,
                tasks_enabled,
                tasks_feature_state,
                auto_verify_draft_plans
         FROM ideation_settings WHERE id = 1
         LIMIT 1",
        [],
        parse_ideation_settings_row,
    );

    match result {
        Ok(settings) => Ok(settings),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(IdeationSettings::default()),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

pub(crate) fn authorize_tasks_session_sync(
    conn: &Connection,
    _session_id: Option<&str>,
    action: TasksFeatureAction,
) -> AppResult<()> {
    if action == TasksFeatureAction::Quiesce {
        return Ok(());
    }
    let settings = get_settings_sync(conn)?;
    if settings.tasks_feature_state == TasksFeatureState::Enabled {
        return Ok(());
    }
    Err(tasks_disabled_error(&format!(
        "{:?} is blocked while Tasks are {}",
        action,
        settings.tasks_feature_state.as_str()
    )))
}

fn tasks_disabled_error(detail: &str) -> AppError {
    AppError::FeatureDisabled(format!(
        "ralphx:tasks_disabled: Tasks are disabled in Planning & Verification settings ({detail})"
    ))
}

#[async_trait]
impl IdeationSettingsRepository for SqliteIdeationSettingsRepository {
    async fn get_settings(&self) -> Result<IdeationSettings, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| get_settings_sync(conn))
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn update_settings(
        &self,
        settings: &IdeationSettings,
    ) -> Result<IdeationSettings, Box<dyn std::error::Error>> {
        let settings = settings.clone();

        self.db
            .run(move |conn| {
                let plan_mode_str = match settings.plan_mode {
                    IdeationPlanMode::Required => "required",
                    IdeationPlanMode::Optional => "optional",
                    IdeationPlanMode::Parallel => "parallel",
                };

                conn.execute(
                    "UPDATE ideation_settings
             SET plan_mode = ?1,
                 require_plan_approval = ?2,
                 suggest_plans_for_complex = ?3,
                 auto_link_proposals = ?4,
                 require_verification_for_accept = ?5,
                 require_verification_for_proposals = ?6,
                 require_accept_for_finalize = ?7,
                 ext_require_verification_for_accept = ?8,
                 ext_require_verification_for_proposals = ?9,
                 ext_require_accept_for_finalize = ?10,
                 auto_verify_plans = ?11,
                 ext_auto_verify_plans = ?12,
                 auto_verify_draft_plans = ?13,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
             WHERE id = 1",
                    rusqlite::params![
                        plan_mode_str,
                        settings.require_plan_approval as i64,
                        settings.suggest_plans_for_complex as i64,
                        settings.auto_link_proposals as i64,
                        settings.require_verification_for_accept as i64,
                        settings.require_verification_for_proposals as i64,
                        settings.require_accept_for_finalize as i64,
                        settings
                            .external_overrides
                            .require_verification_for_accept
                            .map(|v| v as i64),
                        settings
                            .external_overrides
                            .require_verification_for_proposals
                            .map(|v| v as i64),
                        settings
                            .external_overrides
                            .require_accept_for_finalize
                            .map(|v| v as i64),
                        settings.auto_verify_plans as i64,
                        settings
                            .external_overrides
                            .auto_verify_plans
                            .map(|v| v as i64),
                        settings.auto_verify_draft_plans as i64,
                    ],
                )?;

                get_settings_sync(conn)
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn compare_and_set_tasks_feature_state(
        &self,
        expected: TasksFeatureState,
        next: TasksFeatureState,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE ideation_settings
                     SET tasks_feature_state = ?1,
                         tasks_enabled = ?2,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE id = 1 AND tasks_feature_state = ?3",
                    rusqlite::params![
                        next.as_str(),
                        next.tasks_enabled() as i64,
                        expected.as_str(),
                    ],
                )?;
                Ok(changed == 1)
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}

#[cfg(test)]
#[path = "sqlite_ideation_settings_repo_tests.rs"]
mod tests;
