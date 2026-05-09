// SQLite-based AgentRunRepository implementation
// Uses DbConnection (spawn_blocking) for non-blocking rusqlite access

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;

use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
/// Parse datetime string handling both RFC3339 and SQLite's CURRENT_TIMESTAMP formats
fn parse_datetime(s: &str) -> DateTime<Utc> {
    // Try RFC3339 first (e.g., "2026-01-26T06:42:37.662598+00:00")
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }

    // Try SQLite's CURRENT_TIMESTAMP format (e.g., "2026-01-26 07:06:32")
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&ndt);
    }

    // Fallback to now
    Utc::now()
}

use crate::domain::entities::{
    AgentRun, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage, ChatContextType,
    ChatConversation, ChatConversationId, InterruptedConversation,
};

/// Map a SQLite row to an AgentRun (expects columns: id, conversation_id, status,
/// started_at, completed_at, error_message, harness, provider_session_id,
/// upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort,
/// input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
/// estimated_usd, approval_policy, sandbox_mode, run_chain_id, parent_run_id)
fn row_to_agent_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRun> {
    let status_str: String = row.get("status")?;
    let started_at_str: String = row.get("started_at")?;
    let completed_at_str: Option<String> = row.get("completed_at")?;

    Ok(AgentRun {
        id: AgentRunId::from_string(row.get::<_, String>("id")?),
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        status: status_str.parse().unwrap_or(AgentRunStatus::Failed),
        started_at: parse_datetime(&started_at_str),
        completed_at: completed_at_str.map(|s| parse_datetime(&s)),
        error_message: row.get("error_message")?,
        harness: row
            .get::<_, Option<String>>("harness")?
            .and_then(|value| value.parse::<AgentHarnessKind>().ok()),
        provider_session_id: row.get("provider_session_id")?,
        upstream_provider: row.get("upstream_provider")?,
        provider_profile: row.get("provider_profile")?,
        logical_model: row.get("logical_model")?,
        effective_model_id: row.get("effective_model_id")?,
        logical_effort: row
            .get::<_, Option<String>>("logical_effort")?
            .and_then(|value| value.parse::<LogicalEffort>().ok()),
        effective_effort: row.get("effective_effort")?,
        input_tokens: row.get("input_tokens")?,
        output_tokens: row.get("output_tokens")?,
        cache_creation_tokens: row.get("cache_creation_tokens")?,
        cache_read_tokens: row.get("cache_read_tokens")?,
        estimated_usd: row.get("estimated_usd")?,
        approval_policy: row.get("approval_policy")?,
        sandbox_mode: row.get("sandbox_mode")?,
        run_chain_id: row.get("run_chain_id")?,
        parent_run_id: row.get("parent_run_id")?,
    })
}
use crate::domain::repositories::{AgentRunRepository, ORPHANED_AGENT_RUN_ON_APP_RESTART};
use crate::error::AppResult;

use super::DbConnection;

/// SQLite implementation of AgentRunRepository
pub struct SqliteAgentRunRepository {
    db: DbConnection,
}

impl SqliteAgentRunRepository {
    /// Create a new SQLite agent run repository with the given connection
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    /// Create from an Arc-wrapped mutex connection (for sharing)
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl AgentRunRepository for SqliteAgentRunRepository {
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_runs (
                        id, conversation_id, status, started_at, completed_at, error_message,
                        harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                        logical_effort, effective_effort, input_tokens, output_tokens,
                        cache_creation_tokens, cache_read_tokens, estimated_usd,
                        approval_policy, sandbox_mode, run_chain_id, parent_run_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
                    rusqlite::params![
                        run.id.as_str(),
                        run.conversation_id.as_str(),
                        run.status.to_string(),
                        run.started_at.to_rfc3339(),
                        run.completed_at.map(|dt| dt.to_rfc3339()),
                        run.error_message,
                        run.harness.map(|value| value.to_string()),
                        run.provider_session_id,
                        run.upstream_provider,
                        run.provider_profile,
                        run.logical_model,
                        run.effective_model_id,
                        run.logical_effort.map(|value| value.to_string()),
                        run.effective_effort,
                        run.input_tokens,
                        run.output_tokens,
                        run.cache_creation_tokens,
                        run.cache_read_tokens,
                        run.estimated_usd,
                        run.approval_policy,
                        run.sandbox_mode,
                        run.run_chain_id,
                        run.parent_run_id,
                    ],
                )?;
                Ok(run)
            })
            .await
    }

    async fn get_by_id(&self, id: &AgentRunId) -> AppResult<Option<AgentRun>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, status, started_at, completed_at, error_message,
                            harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                            logical_effort, effective_effort, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            approval_policy, sandbox_mode, run_chain_id, parent_run_id
                     FROM agent_runs WHERE id = ?1",
                    [&id],
                    |row| row_to_agent_run(row),
                )
            })
            .await
    }

    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, status, started_at, completed_at, error_message,
                            harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                            logical_effort, effective_effort, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            approval_policy, sandbox_mode, run_chain_id, parent_run_id
                     FROM agent_runs WHERE conversation_id = ?1 ORDER BY started_at DESC LIMIT 1",
                    [&conversation_id],
                    |row| row_to_agent_run(row),
                )
            })
            .await
    }

    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, status, started_at, completed_at, error_message,
                            harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                            logical_effort, effective_effort, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            approval_policy, sandbox_mode, run_chain_id, parent_run_id
                     FROM agent_runs WHERE conversation_id = ?1 AND status = 'running' ORDER BY started_at DESC LIMIT 1",
                    [&conversation_id],
                    |row| row_to_agent_run(row),
                )
            })
            .await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, conversation_id, status, started_at, completed_at, error_message,
                            harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                            logical_effort, effective_effort, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            approval_policy, sandbox_mode, run_chain_id, parent_run_id
                     FROM agent_runs WHERE conversation_id = ?1 ORDER BY started_at DESC",
                )?;
                let runs = stmt
                    .query_map([&conversation_id], |row| row_to_agent_run(row))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(runs)
            })
            .await
    }

    async fn update_status(&self, id: &AgentRunId, status: AgentRunStatus) -> AppResult<()> {
        let id = id.as_str().to_string();
        let status_str = status.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_runs SET status = ?1 WHERE id = ?2",
                    rusqlite::params![status_str, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_usage(&self, id: &AgentRunId, usage: &AgentRunUsage) -> AppResult<()> {
        let id = id.as_str().to_string();
        let usage = usage.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_runs
                     SET input_tokens = COALESCE(?1, input_tokens),
                         output_tokens = COALESCE(?2, output_tokens),
                         cache_creation_tokens = COALESCE(?3, cache_creation_tokens),
                         cache_read_tokens = COALESCE(?4, cache_read_tokens),
                         estimated_usd = COALESCE(?5, estimated_usd)
                     WHERE id = ?6",
                    rusqlite::params![
                        usage.input_tokens,
                        usage.output_tokens,
                        usage.cache_creation_tokens,
                        usage.cache_read_tokens,
                        usage.estimated_usd,
                        id,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_attribution(
        &self,
        id: &AgentRunId,
        attribution: &AgentRunAttribution,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let attribution = attribution.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_runs
                     SET harness = COALESCE(?1, harness),
                         provider_session_id = COALESCE(?2, provider_session_id),
                         upstream_provider = COALESCE(?3, upstream_provider),
                         provider_profile = COALESCE(?4, provider_profile),
                         logical_model = COALESCE(?5, logical_model),
                         effective_model_id = COALESCE(?6, effective_model_id),
                         logical_effort = COALESCE(?7, logical_effort),
                         effective_effort = COALESCE(?8, effective_effort)
                     WHERE id = ?9",
                    rusqlite::params![
                        attribution.harness.map(|value| value.to_string()),
                        attribution.provider_session_id,
                        attribution.upstream_provider,
                        attribution.provider_profile,
                        attribution.logical_model,
                        attribution.effective_model_id,
                        attribution.logical_effort.map(|value| value.to_string()),
                        attribution.effective_effort,
                        id,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn complete(&self, id: &AgentRunId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_runs SET status = 'completed', completed_at = ?1, error_message = NULL WHERE id = ?2",
                    rusqlite::params![Utc::now().to_rfc3339(), id],
                )?;
                Ok(())
            })
            .await
    }

    async fn fail(&self, id: &AgentRunId, error_message: &str) -> AppResult<()> {
        let id = id.as_str().to_string();
        let error_message = error_message.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_runs SET status = 'failed', completed_at = ?1, error_message = ?2 WHERE id = ?3",
                    rusqlite::params![Utc::now().to_rfc3339(), error_message, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn cancel(&self, id: &AgentRunId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_runs SET status = 'cancelled', completed_at = ?1, error_message = NULL WHERE id = ?2",
                    rusqlite::params![Utc::now().to_rfc3339(), id],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &AgentRunId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM agent_runs WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
    }

    async fn delete_by_conversation(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM agent_runs WHERE conversation_id = ?1",
                    [conversation_id],
                )?;
                Ok(())
            })
            .await
    }

    async fn count_by_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentRunStatus,
    ) -> AppResult<u32> {
        let conversation_id = conversation_id.as_str().to_string();
        let status_str = status.to_string();
        self.db
            .run(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM agent_runs WHERE conversation_id = ?1 AND status = ?2",
                    [conversation_id.as_str(), status_str.as_str()],
                    |row| row.get(0),
                )?;
                Ok(count as u32)
            })
            .await
    }

    async fn cancel_all_running(&self) -> AppResult<u32> {
        self.db
            .run(move |conn| {
                let changes = conn.execute(
                    "UPDATE agent_runs SET status = 'cancelled', completed_at = ?1, error_message = ?2 WHERE status = 'running'",
                    rusqlite::params![Utc::now().to_rfc3339(), ORPHANED_AGENT_RUN_ON_APP_RESTART],
                )?;
                Ok(changes as u32)
            })
            .await
    }

    async fn cancel_running_started_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u32> {
        let cutoff = cutoff.to_rfc3339();
        self.db
            .run(move |conn| {
                let changes = conn.execute(
                    "UPDATE agent_runs SET status = 'cancelled', completed_at = ?1, error_message = ?2 WHERE status = 'running' AND started_at < ?3",
                    rusqlite::params![
                        Utc::now().to_rfc3339(),
                        ORPHANED_AGENT_RUN_ON_APP_RESTART,
                        cutoff
                    ],
                )?;
                Ok(changes as u32)
            })
            .await
    }

    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT
                        c.id as conv_id,
                        c.context_type,
                        c.context_id,
                        c.claude_session_id,
                        c.provider_session_id as conv_provider_session_id,
                        c.provider_harness as conv_provider_harness,
                        c.upstream_provider as conv_upstream_provider,
                        c.provider_profile as conv_provider_profile,
                        c.title,
                        c.message_count,
                        c.last_message_at,
                        c.created_at as conv_created_at,
                        c.updated_at as conv_updated_at,
                        ar.id as run_id,
                        ar.conversation_id,
                        ar.status,
                        ar.started_at,
                        ar.completed_at,
                        ar.error_message,
                        ar.harness as run_harness,
                        ar.provider_session_id as run_provider_session_id,
                        ar.upstream_provider as run_upstream_provider,
                        ar.provider_profile as run_provider_profile,
                        ar.logical_model,
                        ar.effective_model_id,
                        ar.logical_effort,
                        ar.effective_effort,
                        ar.input_tokens,
                        ar.output_tokens,
                        ar.cache_creation_tokens,
                        ar.cache_read_tokens,
                        ar.estimated_usd,
                        ar.approval_policy,
                        ar.sandbox_mode,
                        ar.run_chain_id,
                        ar.parent_run_id
                    FROM chat_conversations c
                    INNER JOIN agent_runs ar ON c.id = ar.conversation_id
                    WHERE (
                        c.provider_session_id IS NOT NULL
                        OR c.claude_session_id IS NOT NULL
                    )
                      AND ar.status = 'cancelled'
                      AND ar.error_message = ?1
                      AND ar.id = (
                        SELECT ar2.id FROM agent_runs ar2
                        WHERE ar2.conversation_id = c.id
                        ORDER BY ar2.started_at DESC LIMIT 1
                      )
                    ORDER BY ar.started_at DESC",
                )?;

                let results = stmt
                    .query_map([ORPHANED_AGENT_RUN_ON_APP_RESTART], |row| {
                        let context_type_str: String = row.get("context_type")?;
                        let claude_session_id: Option<String> = row.get("claude_session_id")?;
                        let provider_session_id: Option<String> =
                            row.get("conv_provider_session_id")?;
                        let provider_harness = row
                            .get::<_, Option<String>>("conv_provider_harness")?
                            .and_then(|value| value.parse::<AgentHarnessKind>().ok());
                        let upstream_provider: Option<String> =
                            row.get("conv_upstream_provider")?;
                        let provider_profile: Option<String> =
                            row.get("conv_provider_profile")?;
                        let conv_created_at_str: String = row.get("conv_created_at")?;
                        let conv_updated_at_str: String = row.get("conv_updated_at")?;
                        let last_message_at_str: Option<String> = row.get("last_message_at")?;
                        let mut conversation = ChatConversation {
                            id: ChatConversationId::from_string(row.get::<_, String>("conv_id")?),
                            context_type: context_type_str.parse().unwrap_or(ChatContextType::Project),
                            context_id: row.get("context_id")?,
                            claude_session_id,
                            provider_session_id,
                            provider_harness,
                            upstream_provider,
                            provider_profile,
                            agent_mode: None,
                            title: row.get("title")?,
                            message_count: row.get("message_count")?,
                            last_message_at: last_message_at_str.map(|s| parse_datetime(&s)),
                            created_at: parse_datetime(&conv_created_at_str),
                            updated_at: parse_datetime(&conv_updated_at_str),
                            archived_at: None,
                            parent_conversation_id: None,
                            attribution_backfill_status: None,
                            attribution_backfill_source: None,
                            attribution_backfill_source_path: None,
                            attribution_backfill_last_attempted_at: None,
                            attribution_backfill_completed_at: None,
                            attribution_backfill_error_summary: None,
                        };
                        conversation.normalize_provider_session_fields();

                        let status_str: String = row.get("status")?;
                        let started_at_str: String = row.get("started_at")?;
                        let completed_at_str: Option<String> = row.get("completed_at")?;

                        let last_run = AgentRun {
                            id: AgentRunId::from_string(row.get::<_, String>("run_id")?),
                            conversation_id: ChatConversationId::from_string(
                                row.get::<_, String>("conversation_id")?,
                            ),
                            status: status_str.parse().unwrap_or(AgentRunStatus::Cancelled),
                            started_at: parse_datetime(&started_at_str),
                            completed_at: completed_at_str.map(|s| parse_datetime(&s)),
                            error_message: row.get("error_message")?,
                            harness: row
                                .get::<_, Option<String>>("run_harness")?
                                .and_then(|value| value.parse::<AgentHarnessKind>().ok()),
                            provider_session_id: row.get("run_provider_session_id")?,
                            upstream_provider: row.get("run_upstream_provider")?,
                            provider_profile: row.get("run_provider_profile")?,
                            logical_model: row.get("logical_model")?,
                            effective_model_id: row.get("effective_model_id")?,
                            logical_effort: row
                                .get::<_, Option<String>>("logical_effort")?
                                .and_then(|value| value.parse::<LogicalEffort>().ok()),
                            effective_effort: row.get("effective_effort")?,
                            input_tokens: row.get("input_tokens")?,
                            output_tokens: row.get("output_tokens")?,
                            cache_creation_tokens: row.get("cache_creation_tokens")?,
                            cache_read_tokens: row.get("cache_read_tokens")?,
                            estimated_usd: row.get("estimated_usd")?,
                            approval_policy: row.get("approval_policy")?,
                            sandbox_mode: row.get("sandbox_mode")?,
                            run_chain_id: row.get("run_chain_id")?,
                            parent_run_id: row.get("parent_run_id")?,
                        };

                        Ok(InterruptedConversation {
                            conversation,
                            last_run,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(results)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_agent_run_repo_tests.rs"]
mod tests;
