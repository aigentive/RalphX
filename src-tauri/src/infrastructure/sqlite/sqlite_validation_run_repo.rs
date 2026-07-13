use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::domain::entities::{
    ProjectId, TaskId, ValidationCacheDecision, ValidationCommandCategory, ValidationCommandResult,
    ValidationCommandSource, ValidationCommandStatus, ValidationContextType, ValidationPurpose,
    ValidationRun, ValidationRunMode, ValidationRunStatus, ValidationRunWithResults,
};
use crate::domain::repositories::ValidationRunRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

pub struct SqliteValidationRunRepository {
    db: DbConnection,
}

impl SqliteValidationRunRepository {
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

    fn format_datetime(dt: &DateTime<Utc>) -> String {
        dt.to_rfc3339()
    }

    fn parse_datetime(value: Option<String>) -> Option<DateTime<Utc>> {
        let value = value?;
        DateTime::parse_from_rfc3339(&value)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .map(|ndt| Utc.from_utc_datetime(&ndt))
            })
    }

    fn parse_datetime_required(value: String) -> DateTime<Utc> {
        Self::parse_datetime(Some(value)).unwrap_or_else(Utc::now)
    }

    fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<ValidationRun> {
        let started_at: String = row.get(15)?;
        Ok(ValidationRun {
            id: row.get(0)?,
            task_id: TaskId::from_string(row.get(1)?),
            project_id: ProjectId::from_string(row.get(2)?),
            purpose: ValidationPurpose::parse(row.get::<_, String>(3)?.as_str()),
            context_type: ValidationContextType::parse(row.get::<_, String>(4)?.as_str()),
            requested_by_agent: row.get(5)?,
            status: ValidationRunStatus::parse(row.get::<_, String>(6)?.as_str()),
            mode: ValidationRunMode::parse(row.get::<_, String>(7)?.as_str()),
            policy_enabled: row.get::<_, i64>(8)? != 0,
            head_sha: row.get(9)?,
            start_content_fingerprint: row.get(10)?,
            validated_content_fingerprint: row.get(11)?,
            promoted_commit_sha: row.get(12)?,
            base_ref: row.get(13)?,
            analysis_fingerprint: row.get(14)?,
            status_episode_entered_at: Self::parse_datetime(row.get(16)?),
            started_at: Self::parse_datetime_required(started_at),
            completed_at: Self::parse_datetime(row.get(17)?),
        })
    }

    fn row_to_command(row: &rusqlite::Row<'_>) -> rusqlite::Result<ValidationCommandResult> {
        let related_files_json: String = row.get(11)?;
        let related_files = serde_json::from_str(&related_files_json).unwrap_or_default();
        let created_at: String = row.get(26)?;
        Ok(ValidationCommandResult {
            id: row.get(0)?,
            validation_run_id: row.get(1)?,
            task_id: TaskId::from_string(row.get(2)?),
            project_id: ProjectId::from_string(row.get(3)?),
            command_source: ValidationCommandSource::parse(row.get::<_, String>(4)?.as_str()),
            command_ref: row.get(5)?,
            command: row.get(6)?,
            cwd: row.get(7)?,
            label: row.get(8)?,
            category: ValidationCommandCategory::parse(row.get::<_, String>(9)?.as_str()),
            reason: row.get(10)?,
            related_files,
            cache_key: row.get(12)?,
            cache_decision: ValidationCacheDecision::parse(row.get::<_, String>(13)?.as_str()),
            status: ValidationCommandStatus::parse(row.get::<_, String>(14)?.as_str()),
            exit_code: row.get(15)?,
            duration_ms: row.get::<_, Option<i64>>(16)?.map(|v| v as u64),
            stdout_snippet: row.get(17)?,
            stderr_snippet: row.get(18)?,
            stdout_log_path: row.get(19)?,
            stderr_log_path: row.get(20)?,
            launcher_kind: row.get(21)?,
            resolved_shell_path: row.get(22)?,
            head_sha: row.get(23)?,
            analysis_fingerprint: row.get(24)?,
            status_episode_entered_at: Self::parse_datetime(row.get(25)?),
            created_at: Self::parse_datetime_required(created_at),
        })
    }
}

const SELECT_RUN: &str = "SELECT
    id, task_id, project_id, purpose, context_type, requested_by_agent, status, mode,
    policy_enabled, head_sha, start_content_fingerprint, validated_content_fingerprint,
    promoted_commit_sha, base_ref, analysis_fingerprint, started_at,
    status_episode_entered_at, completed_at
FROM validation_runs";

const SELECT_COMMAND: &str = "SELECT
    id, validation_run_id, task_id, project_id, command_source, command_ref, command,
    cwd, label, category, reason, related_files_json, cache_key,
    cache_decision, status, exit_code, duration_ms, stdout_snippet, stderr_snippet,
    stdout_log_path, stderr_log_path, launcher_kind, resolved_shell_path, head_sha,
    analysis_fingerprint, status_episode_entered_at, created_at
FROM validation_command_results";

#[async_trait]
impl ValidationRunRepository for SqliteValidationRunRepository {
    async fn create_run(&self, run: &ValidationRun) -> AppResult<()> {
        let run = run.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO validation_runs (
                        id, task_id, project_id, purpose, context_type, requested_by_agent,
                        status, mode, policy_enabled, head_sha, start_content_fingerprint,
                        validated_content_fingerprint, promoted_commit_sha, base_ref,
                        analysis_fingerprint, status_episode_entered_at, started_at, completed_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                    rusqlite::params![
                        run.id,
                        run.task_id.as_str(),
                        run.project_id.as_str(),
                        run.purpose.as_str(),
                        run.context_type.as_str(),
                        run.requested_by_agent,
                        run.status.as_str(),
                        run.mode.as_str(),
                        run.policy_enabled as i64,
                        run.head_sha,
                        run.start_content_fingerprint,
                        run.validated_content_fingerprint,
                        run.promoted_commit_sha,
                        run.base_ref,
                        run.analysis_fingerprint,
                        run.status_episode_entered_at
                            .map(|dt| Self::format_datetime(&dt)),
                        Self::format_datetime(&run.started_at),
                        run.completed_at.map(|dt| Self::format_datetime(&dt)),
                    ],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn update_run_status(
        &self,
        run_id: &str,
        status: ValidationRunStatus,
        completed_at: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        let run_id = run_id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE validation_runs SET status = ?1, completed_at = ?2 WHERE id = ?3",
                    rusqlite::params![
                        status.as_str(),
                        completed_at.map(|dt| Self::format_datetime(&dt)),
                        run_id,
                    ],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn record_validated_content_fingerprint(
        &self,
        run_id: &str,
        fingerprint: Option<String>,
    ) -> AppResult<()> {
        let run_id = run_id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE validation_runs SET validated_content_fingerprint = ?1 WHERE id = ?2",
                    rusqlite::params![fingerprint, run_id],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn promote_run_to_commit(&self, run_id: &str, commit_sha: &str) -> AppResult<()> {
        let run_id = run_id.to_string();
        let commit_sha = commit_sha.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE validation_runs SET promoted_commit_sha = ?1 WHERE id = ?2",
                    rusqlite::params![commit_sha, run_id],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn mark_running_runs_error(&self, completed_at: DateTime<Utc>) -> AppResult<u64> {
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE validation_runs
                         SET status = ?1, completed_at = ?2
                         WHERE status = ?3",
                        rusqlite::params![
                            ValidationRunStatus::Error.as_str(),
                            Self::format_datetime(&completed_at),
                            ValidationRunStatus::Running.as_str(),
                        ],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(rows as u64)
            })
            .await
    }

    async fn add_command_result(&self, result: &ValidationCommandResult) -> AppResult<()> {
        let result = result.clone();
        self.db
            .run(move |conn| {
                let related_files_json =
                    serde_json::to_string(&result.related_files).map_err(|e| {
                        AppError::Database(format!("failed to serialize related files: {e}"))
                    })?;
                conn.execute(
                    "INSERT INTO validation_command_results (
                        id, validation_run_id, task_id, project_id, command_source, command_ref,
                        command, cwd, label, category, reason, related_files_json, cache_key,
                        cache_decision, status, exit_code, duration_ms, stdout_snippet,
                        stderr_snippet, stdout_log_path, stderr_log_path, launcher_kind,
                        resolved_shell_path, head_sha, analysis_fingerprint,
                        status_episode_entered_at, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                        ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
                    rusqlite::params![
                        result.id,
                        result.validation_run_id,
                        result.task_id.as_str(),
                        result.project_id.as_str(),
                        result.command_source.as_str(),
                        result.command_ref,
                        result.command,
                        result.cwd,
                        result.label,
                        result.category.as_str(),
                        result.reason,
                        related_files_json,
                        result.cache_key,
                        result.cache_decision.as_str(),
                        result.status.as_str(),
                        result.exit_code,
                        result.duration_ms.map(|v| v as i64),
                        result.stdout_snippet,
                        result.stderr_snippet,
                        result.stdout_log_path,
                        result.stderr_log_path,
                        result.launcher_kind,
                        result.resolved_shell_path,
                        result.head_sha,
                        result.analysis_fingerprint,
                        result
                            .status_episode_entered_at
                            .map(|dt| Self::format_datetime(&dt)),
                        Self::format_datetime(&result.created_at),
                    ],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn list_command_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Vec<ValidationCommandResult>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = format!("{SELECT_COMMAND} WHERE task_id = ?1 ORDER BY created_at DESC");
                let mut statement = conn
                    .prepare(&sql)
                    .map_err(|e| AppError::Database(e.to_string()))?;
                let rows = statement
                    .query_map([task_id], Self::row_to_command)
                    .map_err(|e| AppError::Database(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(e.to_string()))
            })
            .await
    }

    async fn latest_run_with_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>> {
        self.latest_run_with_results_for_task_filtered(task_id, false)
            .await
    }

    async fn latest_non_baseline_run_with_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>> {
        self.latest_run_with_results_for_task_filtered(task_id, true)
            .await
    }
}

impl SqliteValidationRunRepository {
    async fn latest_run_with_results_for_task_filtered(
        &self,
        task_id: &TaskId,
        exclude_baseline: bool,
    ) -> AppResult<Option<ValidationRunWithResults>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = if exclude_baseline {
                    format!(
                        "{SELECT_RUN} WHERE task_id = ?1 AND purpose <> ?2 ORDER BY started_at DESC LIMIT 1"
                    )
                } else {
                    format!("{SELECT_RUN} WHERE task_id = ?1 ORDER BY started_at DESC LIMIT 1")
                };
                let run = if exclude_baseline {
                    conn.query_row(
                        &sql,
                        rusqlite::params![task_id.as_str(), ValidationPurpose::Baseline.as_str()],
                        Self::row_to_run,
                    )
                } else {
                    conn.query_row(&sql, [task_id.as_str()], Self::row_to_run)
                };
                let run = match run {
                    Ok(run) => run,
                    Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                    Err(e) => return Err(AppError::Database(e.to_string())),
                };

                let sql = format!(
                    "{SELECT_COMMAND} WHERE validation_run_id = ?1 ORDER BY created_at ASC"
                );
                let mut statement = conn
                    .prepare(&sql)
                    .map_err(|e| AppError::Database(e.to_string()))?;
                let commands = statement
                    .query_map([run.id.as_str()], Self::row_to_command)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(e.to_string()))?;

                Ok(Some(ValidationRunWithResults { run, commands }))
            })
            .await
    }
}
