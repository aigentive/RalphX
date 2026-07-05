use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{
    judge_transition_clears_verdict, Automation, AutomationId, AutomationJudgeState,
    AutomationPromptAuthor, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus,
    ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    AutomationRepository, AutomationRunPublicationMetadata, AutomationRunRepository,
    AutomationSettingsPatch,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

pub struct SqliteAutomationRepository {
    db: DbConnection,
}

impl SqliteAutomationRepository {
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

    fn row_to_automation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Automation> {
        Ok(Automation {
            id: AutomationId::from_string(row.get::<_, String>(0)?),
            project_id: ProjectId::from_string(row.get::<_, String>(1)?),
            name: row.get(2)?,
            status: AutomationStatus::parse(&row.get::<_, String>(3)?)
                .unwrap_or(AutomationStatus::Draft),
            paused_reason_code: row.get(4)?,
            paused_reason_detail: row.get(5)?,
            goal_prompt: row.get(6)?,
            setup_conversation_id: row
                .get::<_, Option<String>>(7)?
                .map(ChatConversationId::from_string),
            provider_harness: row.get(8)?,
            model_id: row.get(9)?,
            logical_effort: row.get(10)?,
            run_mode: row.get(11)?,
            base_ref_kind: row.get(12)?,
            base_ref: row.get(13)?,
            base_display_name: row.get(14)?,
            base_source_pull_request_json: row.get(15)?,
            goal_items_json: row.get(16)?,
            chain_mode: row.get(17)?,
            completion_signal: row.get(18)?,
            max_runs: row.get(19)?,
            max_consecutive_failures: row.get(20)?,
            first_run_prompt: row.get(21)?,
            setup_analysis_summary: row.get(22)?,
            created_at: parse_datetime_required(row.get(23)?),
            updated_at: parse_datetime_required(row.get(24)?),
        })
    }
}

const SELECT_AUTOMATION: &str = "SELECT
    id, project_id, name, status, paused_reason_code, paused_reason_detail,
    goal_prompt, setup_conversation_id, provider_harness, model_id, logical_effort,
    run_mode, base_ref_kind, base_ref, base_display_name, base_source_pull_request_json,
    goal_items_json, chain_mode, completion_signal, max_runs, max_consecutive_failures,
    first_run_prompt, setup_analysis_summary, created_at, updated_at
FROM automations";

#[async_trait]
impl AutomationRepository for SqliteAutomationRepository {
    async fn create(&self, automation: Automation) -> AppResult<Automation> {
        let to_insert = automation.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO automations (
                        id, project_id, name, status, paused_reason_code, paused_reason_detail,
                        goal_prompt, setup_conversation_id, provider_harness, model_id,
                        logical_effort, run_mode, base_ref_kind, base_ref, base_display_name,
                        base_source_pull_request_json, goal_items_json, chain_mode,
                        completion_signal, max_runs, max_consecutive_failures, first_run_prompt,
                        setup_analysis_summary, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                        ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25
                    )",
                    params![
                        to_insert.id.as_str(),
                        to_insert.project_id.as_str(),
                        to_insert.name,
                        to_insert.status.as_str(),
                        to_insert.paused_reason_code,
                        to_insert.paused_reason_detail,
                        to_insert.goal_prompt,
                        to_insert.setup_conversation_id.map(|id| id.as_str()),
                        to_insert.provider_harness,
                        to_insert.model_id,
                        to_insert.logical_effort,
                        to_insert.run_mode,
                        to_insert.base_ref_kind,
                        to_insert.base_ref,
                        to_insert.base_display_name,
                        to_insert.base_source_pull_request_json,
                        to_insert.goal_items_json,
                        to_insert.chain_mode,
                        to_insert.completion_signal,
                        to_insert.max_runs,
                        to_insert.max_consecutive_failures,
                        to_insert.first_run_prompt,
                        to_insert.setup_analysis_summary,
                        to_insert.created_at.to_rfc3339(),
                        to_insert.updated_at.to_rfc3339(),
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(automation)
    }

    async fn get_by_id(&self, id: &AutomationId) -> AppResult<Option<Automation>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = format!("{SELECT_AUTOMATION} WHERE id = ?1");
                conn.query_row(&sql, [id], Self::row_to_automation)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn list(&self, project_id: Option<ProjectId>) -> AppResult<Vec<Automation>> {
        self.db
            .run(move |conn| {
                let sql = if project_id.is_some() {
                    format!(
                        "{SELECT_AUTOMATION} WHERE project_id = ?1 ORDER BY created_at DESC, id DESC"
                    )
                } else {
                    format!("{SELECT_AUTOMATION} ORDER BY created_at DESC, id DESC")
                };
                let mut statement = conn.prepare(&sql)?;
                let rows = if let Some(project_id) = project_id {
                    statement.query_map([project_id.as_str()], Self::row_to_automation)?
                } else {
                    statement.query_map([], Self::row_to_automation)?
                };
                rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
            })
            .await
    }

    async fn list_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Automation>> {
        self.list(Some(project_id.clone())).await
    }

    async fn update_settings(
        &self,
        id: &AutomationId,
        patch: AutomationSettingsPatch,
    ) -> AppResult<Option<Automation>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let affected = conn.execute(
                    "UPDATE automations
                     SET name = COALESCE(?1, name),
                         max_runs = COALESCE(?2, max_runs),
                         max_consecutive_failures = COALESCE(?3, max_consecutive_failures),
                         updated_at = ?4
                     WHERE id = ?5",
                    params![
                        patch.name,
                        patch.max_runs,
                        patch.max_consecutive_failures,
                        Utc::now().to_rfc3339(),
                        id,
                    ],
                )?;
                if affected == 0 {
                    return Ok(None);
                }

                let sql = format!("{SELECT_AUTOMATION} WHERE id = ?1");
                conn.query_row(&sql, [id], Self::row_to_automation)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn compare_and_swap_status(
        &self,
        id: &AutomationId,
        from: AutomationStatus,
        to: AutomationStatus,
        paused_reason_code: Option<String>,
        paused_reason_detail: Option<String>,
    ) -> AppResult<bool> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let affected = conn.execute(
                    "UPDATE automations
                     SET status = ?1,
                         paused_reason_code = ?2,
                         paused_reason_detail = ?3,
                         updated_at = ?4
                     WHERE id = ?5 AND status = ?6",
                    params![
                        to.as_str(),
                        paused_reason_code,
                        paused_reason_detail,
                        Utc::now().to_rfc3339(),
                        id,
                        from.as_str(),
                    ],
                )?;
                Ok(affected == 1)
            })
            .await
    }

    async fn delete_terminal(&self, id: &AutomationId) -> AppResult<bool> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let affected = conn.execute(
                    "DELETE FROM automations
                     WHERE id = ?1 AND status IN ('completed', 'stopped')",
                    [id],
                )?;
                Ok(affected == 1)
            })
            .await
    }
}

pub struct SqliteAutomationRunRepository {
    db: DbConnection,
}

impl SqliteAutomationRunRepository {
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

    fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRun> {
        Ok(AutomationRun {
            id: AutomationRunId::from_string(row.get::<_, String>(0)?),
            automation_id: AutomationId::from_string(row.get::<_, String>(1)?),
            run_index: row.get(2)?,
            status: AutomationRunStatus::parse(&row.get::<_, String>(3)?)
                .unwrap_or(AutomationRunStatus::Pending),
            judge_state: AutomationJudgeState::parse(&row.get::<_, String>(4)?)
                .unwrap_or(AutomationJudgeState::None),
            judge_lease_expires_at: parse_datetime(row.get(5)?),
            conversation_id: row
                .get::<_, Option<String>>(6)?
                .map(ChatConversationId::from_string),
            run_prompt: row.get(7)?,
            prompt_author: AutomationPromptAuthor::parse(&row.get::<_, String>(8)?)
                .unwrap_or(AutomationPromptAuthor::SetupAgent),
            base_ref_kind: row.get(9)?,
            base_ref_used: row.get(10)?,
            base_from_run_id: row
                .get::<_, Option<String>>(11)?
                .map(AutomationRunId::from_string),
            branch_name: row.get(12)?,
            pr_number: row.get(13)?,
            pr_url: row.get(14)?,
            pr_title: row.get(15)?,
            pr_head_ref_name: row.get(16)?,
            pr_base_ref_name: row.get(17)?,
            pr_merged_at: parse_datetime(row.get(18)?),
            merge_commit_sha: row.get(19)?,
            diff_stats_json: row.get(20)?,
            agent_summary: row.get(21)?,
            judge_verdict_json: row.get(22)?,
            judge_model_id: row.get(23)?,
            error_code: row.get(24)?,
            error_detail: row.get(25)?,
            signal_check_failures: row.get(26)?,
            started_at: parse_datetime(row.get(27)?),
            finished_at: parse_datetime(row.get(28)?),
            created_at: parse_datetime_required(row.get(29)?),
            updated_at: parse_datetime_required(row.get(30)?),
        })
    }
}

const SELECT_RUN: &str = "SELECT
    id, automation_id, run_index, status, judge_state, judge_lease_expires_at,
    conversation_id, run_prompt, prompt_author, base_ref_kind, base_ref_used,
    base_from_run_id, branch_name, pr_number, pr_url, pr_title, pr_head_ref_name,
    pr_base_ref_name, pr_merged_at, merge_commit_sha, diff_stats_json, agent_summary,
    judge_verdict_json, judge_model_id, error_code, error_detail, signal_check_failures,
    started_at, finished_at, created_at, updated_at
FROM automation_runs";
const AUTOMATION_RUN_SINGLE_OPEN_CONSTRAINT: &str =
    "UNIQUE constraint failed: automation_runs.automation_id";

#[async_trait]
impl AutomationRunRepository for SqliteAutomationRunRepository {
    async fn create_run(&self, run: AutomationRun) -> AppResult<AutomationRun> {
        let to_insert = run.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO automation_runs (
                        id, automation_id, run_index, status, judge_state, judge_lease_expires_at,
                        conversation_id, run_prompt, prompt_author, base_ref_kind, base_ref_used,
                        base_from_run_id, branch_name, pr_number, pr_url, pr_title,
                        pr_head_ref_name, pr_base_ref_name, pr_merged_at, merge_commit_sha,
                        diff_stats_json, agent_summary, judge_verdict_json, judge_model_id,
                        error_code, error_detail, signal_check_failures, started_at, finished_at,
                        created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                        ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                        ?25, ?26, ?27, ?28, ?29, ?30, ?31
                    )",
                    params![
                        to_insert.id.as_str(),
                        to_insert.automation_id.as_str(),
                        to_insert.run_index,
                        to_insert.status.as_str(),
                        to_insert.judge_state.as_str(),
                        to_insert.judge_lease_expires_at.map(|dt| dt.to_rfc3339()),
                        to_insert.conversation_id.map(|id| id.as_str()),
                        to_insert.run_prompt,
                        to_insert.prompt_author.as_str(),
                        to_insert.base_ref_kind,
                        to_insert.base_ref_used,
                        to_insert.base_from_run_id.map(|id| id.as_str().to_string()),
                        to_insert.branch_name,
                        to_insert.pr_number,
                        to_insert.pr_url,
                        to_insert.pr_title,
                        to_insert.pr_head_ref_name,
                        to_insert.pr_base_ref_name,
                        to_insert.pr_merged_at.map(|dt| dt.to_rfc3339()),
                        to_insert.merge_commit_sha,
                        to_insert.diff_stats_json,
                        to_insert.agent_summary,
                        to_insert.judge_verdict_json,
                        to_insert.judge_model_id,
                        to_insert.error_code,
                        to_insert.error_detail,
                        to_insert.signal_check_failures,
                        to_insert.started_at.map(|dt| dt.to_rfc3339()),
                        to_insert.finished_at.map(|dt| dt.to_rfc3339()),
                        to_insert.created_at.to_rfc3339(),
                        to_insert.updated_at.to_rfc3339(),
                    ],
                )
                .map_err(|error| match error {
                    rusqlite::Error::SqliteFailure(sqlite_error, Some(message))
                        if sqlite_error.code == ErrorCode::ConstraintViolation
                            && message == AUTOMATION_RUN_SINGLE_OPEN_CONSTRAINT =>
                    {
                        AppError::Conflict("automation already has an open run".to_string())
                    }
                    error => AppError::from(error),
                })?;
                Ok(())
            })
            .await?;
        Ok(run)
    }

    async fn get_by_id(&self, id: &AutomationRunId) -> AppResult<Option<AutomationRun>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = format!("{SELECT_RUN} WHERE id = ?1");
                conn.query_row(&sql, [id], Self::row_to_run)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn list_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Vec<AutomationRun>> {
        let automation_id = automation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = format!("{SELECT_RUN} WHERE automation_id = ?1 ORDER BY run_index ASC");
                let mut statement = conn.prepare(&sql)?;
                let rows = statement.query_map([automation_id], Self::row_to_run)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
            })
            .await
    }

    async fn latest_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Option<AutomationRun>> {
        let automation_id = automation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = format!(
                    "{SELECT_RUN} WHERE automation_id = ?1 ORDER BY run_index DESC LIMIT 1"
                );
                conn.query_row(&sql, [automation_id], Self::row_to_run)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn compare_and_swap_status(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();
                let terminal = matches!(
                    to,
                    AutomationRunStatus::Merged
                        | AutomationRunStatus::PrClosed
                        | AutomationRunStatus::AgentFailed
                        | AutomationRunStatus::Cancelled
                );
                let affected = conn.execute(
                    "UPDATE automation_runs
                     SET status = ?1,
                         error_code = ?2,
                         error_detail = ?3,
                         finished_at = CASE
                             WHEN ?4 = 1 THEN COALESCE(finished_at, ?5)
                             ELSE finished_at
                         END,
                         updated_at = ?5
                     WHERE id = ?6 AND status = ?7",
                    params![
                        to.as_str(),
                        error_code,
                        error_detail,
                        if terminal { 1_i64 } else { 0_i64 },
                        now,
                        id,
                        from.as_str(),
                    ],
                )?;
                Ok(affected == 1)
            })
            .await
    }

    async fn update_start_metadata(
        &self,
        id: &AutomationRunId,
        conversation_id: &ChatConversationId,
        branch_name: Option<String>,
    ) -> AppResult<Option<AutomationRun>> {
        let id = id.as_str().to_string();
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();
                let affected = conn.execute(
                    "UPDATE automation_runs
                     SET conversation_id = ?2,
                         branch_name = ?3,
                         started_at = COALESCE(started_at, ?4),
                         updated_at = ?4
                     WHERE id = ?1 AND status = 'provisioning'",
                    params![id, conversation_id, branch_name, now],
                )?;
                if affected == 0 {
                    return Ok(None);
                }

                let sql = format!("{SELECT_RUN} WHERE id = ?1");
                conn.query_row(&sql, [id], Self::row_to_run)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn update_publication_metadata(
        &self,
        id: &AutomationRunId,
        metadata: AutomationRunPublicationMetadata,
    ) -> AppResult<Option<AutomationRun>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();
                let affected = conn.execute(
                    "UPDATE automation_runs
                     SET pr_number = ?2,
                         pr_url = ?3,
                         pr_title = ?4,
                         pr_head_ref_name = ?5,
                         pr_base_ref_name = ?6,
                         updated_at = ?7
                     WHERE id = ?1 AND status IN ('running', 'published')",
                    params![
                        id,
                        metadata.pr_number,
                        metadata.pr_url,
                        metadata.pr_title,
                        metadata.pr_head_ref_name,
                        metadata.pr_base_ref_name,
                        now,
                    ],
                )?;
                if affected == 0 {
                    return Ok(None);
                }

                let sql = format!("{SELECT_RUN} WHERE id = ?1");
                conn.query_row(&sql, [id], Self::row_to_run)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn update_merge_metadata(
        &self,
        id: &AutomationRunId,
        merge_commit_sha: Option<String>,
        pr_merged_at: Option<DateTime<Utc>>,
    ) -> AppResult<Option<AutomationRun>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();
                let affected = conn.execute(
                    "UPDATE automation_runs
                     SET pr_merged_at = ?2,
                         merge_commit_sha = ?3,
                         signal_check_failures = 0,
                         updated_at = ?4
                     WHERE id = ?1 AND status = 'published'",
                    params![
                        id,
                        pr_merged_at.map(|dt| dt.to_rfc3339()),
                        merge_commit_sha,
                        now,
                    ],
                )?;
                if affected == 0 {
                    return Ok(None);
                }

                let sql = format!("{SELECT_RUN} WHERE id = ?1");
                conn.query_row(&sql, [id], Self::row_to_run)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn increment_signal_check_failures(
        &self,
        id: &AutomationRunId,
    ) -> AppResult<Option<AutomationRun>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let affected = conn.execute(
                    "UPDATE automation_runs
                     SET signal_check_failures = signal_check_failures + 1,
                         updated_at = ?2
                     WHERE id = ?1 AND status = 'published'",
                    params![id, Utc::now().to_rfc3339()],
                )?;
                if affected == 0 {
                    return Ok(None);
                }

                let sql = format!("{SELECT_RUN} WHERE id = ?1");
                conn.query_row(&sql, [id], Self::row_to_run)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn reset_signal_check_failures(
        &self,
        id: &AutomationRunId,
    ) -> AppResult<Option<AutomationRun>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let affected = conn.execute(
                    "UPDATE automation_runs
                     SET signal_check_failures = 0,
                         updated_at = ?2
                     WHERE id = ?1 AND status = 'published'",
                    params![id, Utc::now().to_rfc3339()],
                )?;
                if affected == 0 {
                    return Ok(None);
                }

                let sql = format!("{SELECT_RUN} WHERE id = ?1");
                conn.query_row(&sql, [id], Self::row_to_run)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
    }

    async fn compare_and_swap_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationJudgeState,
        to: AutomationJudgeState,
        judge_verdict_json: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        let id = id.as_str().to_string();
        let clear_judge_verdict = i64::from(judge_transition_clears_verdict(
            to,
            judge_verdict_json.as_deref(),
        ));
        self.db
            .run(move |conn| {
                let affected = conn.execute(
                    "UPDATE automation_runs
                     SET judge_state = ?1,
                         judge_verdict_json = CASE
                             WHEN ?2 = 1 THEN NULL
                             ELSE COALESCE(?3, judge_verdict_json)
                         END,
                         error_detail = ?4,
                         updated_at = ?5
                     WHERE id = ?6 AND judge_state = ?7",
                    params![
                        to.as_str(),
                        clear_judge_verdict,
                        judge_verdict_json,
                        error_detail,
                        Utc::now().to_rfc3339(),
                        id,
                        from.as_str(),
                    ],
                )?;
                Ok(affected == 1)
            })
            .await
    }

    async fn delete_for_automation(&self, automation_id: &AutomationId) -> AppResult<usize> {
        let automation_id = automation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM automation_runs WHERE automation_id = ?1",
                    [automation_id],
                )
                .map_err(AppError::from)
            })
            .await
    }
}

fn parse_datetime(value: Option<String>) -> Option<DateTime<Utc>> {
    let value = value?;
    DateTime::parse_from_rfc3339(&value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|datetime| Utc.from_utc_datetime(&datetime))
        })
}

fn parse_datetime_required(value: String) -> DateTime<Utc> {
    parse_datetime(Some(value)).unwrap_or_else(Utc::now)
}
