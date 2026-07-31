// SQLite-based AgentRunRepository implementation
// Uses DbConnection (spawn_blocking) for non-blocking rusqlite access

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params_from_iter, Connection};

use crate::domain::agents::{AgentHarnessKind, LogicalEffort};

fn parse_usage_provenance(value: Option<String>) -> rusqlite::Result<Option<UsageProvenance>> {
    value
        .map(|value| {
            value.parse::<UsageProvenance>().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                )
            })
        })
        .transpose()
}
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

use crate::domain::entities::agent_run::PersonaRunAttribution;
use crate::domain::entities::{
    AgentRun, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage, AutomationId,
    AutomationRunId, ChatContextType, ChatConversation, ChatConversationId, CoordinationMode,
    InterruptedConversation, ProviderUsageSnapshot, UsageCapture, UsageProvenance,
};

/// Map a SQLite row to an AgentRun (expects columns: id, conversation_id, status,
/// started_at, completed_at, error_message, harness, provider_session_id,
/// upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort,
/// service_tier, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
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
        service_tier: row.get("service_tier")?,
        input_tokens: row.get("input_tokens")?,
        output_tokens: row.get("output_tokens")?,
        cache_creation_tokens: row.get("cache_creation_tokens")?,
        cache_read_tokens: row.get("cache_read_tokens")?,
        estimated_usd: row.get("estimated_usd")?,
        usage_provenance: parse_usage_provenance(row.get("usage_provenance")?)?,
        raw_usage_snapshot: {
            let snapshot = ProviderUsageSnapshot {
                input_tokens: row.get("raw_usage_input_tokens")?,
                output_tokens: row.get("raw_usage_output_tokens")?,
                cache_creation_tokens: row.get("raw_usage_cache_creation_tokens")?,
                cache_read_tokens: row.get("raw_usage_cache_read_tokens")?,
                estimated_usd: row.get("raw_usage_estimated_usd")?,
            };
            (!snapshot.as_usage().is_empty()).then_some(snapshot)
        },
        approval_policy: row.get("approval_policy")?,
        sandbox_mode: row.get("sandbox_mode")?,
        agent_name: row.get("agent_name")?,
        launch_role: row.get("launch_role")?,
        runtime_source: row.get("runtime_source")?,
        run_chain_id: row.get("run_chain_id")?,
        parent_run_id: row.get("parent_run_id")?,
        action_kind: row
            .get::<_, Option<String>>("action_kind")?
            .and_then(|value| value.parse().ok()),
        action_context_id: row.get("action_context_id")?,
        action_target_id: row.get("action_target_id")?,
        persona_id: row.get("persona_id")?,
        persona_slug: row.get("persona_slug")?,
        persona_version: row.get("persona_version")?,
        persona_content_hash: row.get("persona_content_hash")?,
        persona_injected: row
            .get::<_, Option<i64>>("persona_injected")?
            .map(|value| value != 0),
        persona_skipped_reason: row.get("persona_skipped_reason")?,
    })
}
use crate::domain::repositories::{
    AgentRunRepository, ORPHANED_AGENT_RUN_ON_APP_RESTART, PRUNED_STALE_AGENT_RUN,
};
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
                        logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                        cache_creation_tokens, cache_read_tokens, estimated_usd,
                        usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                        raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens,
                        raw_usage_estimated_usd,
                        approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                        action_kind, action_context_id, action_target_id,
                        persona_id, persona_slug, persona_version, persona_content_hash,
                        persona_injected, persona_skipped_reason
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41, ?42)",
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
                        run.service_tier,
                        run.input_tokens,
                        run.output_tokens,
                        run.cache_creation_tokens,
                        run.cache_read_tokens,
                        run.estimated_usd,
                        run.usage_provenance.map(|value| value.to_string()),
                        run.raw_usage_snapshot.as_ref().and_then(|value| value.input_tokens),
                        run.raw_usage_snapshot.as_ref().and_then(|value| value.output_tokens),
                        run.raw_usage_snapshot.as_ref().and_then(|value| value.cache_creation_tokens),
                        run.raw_usage_snapshot.as_ref().and_then(|value| value.cache_read_tokens),
                        run.raw_usage_snapshot.as_ref().and_then(|value| value.estimated_usd),
                        run.approval_policy,
                        run.sandbox_mode,
                        run.agent_name,
                        run.launch_role,
                        run.runtime_source,
                        run.run_chain_id,
                        run.parent_run_id,
                        run.action_kind.map(|value| value.to_string()),
                        run.action_context_id,
                        run.action_target_id,
                        run.persona_id,
                        run.persona_slug,
                        run.persona_version,
                        run.persona_content_hash,
                        run.persona_injected,
                        run.persona_skipped_reason,
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
                            logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                            raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd,
                            approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                            action_kind, action_context_id, action_target_id,
                            persona_id, persona_slug, persona_version, persona_content_hash,
                            persona_injected, persona_skipped_reason
                     FROM agent_runs WHERE id = ?1",
                    [&id],
                    |row| row_to_agent_run(row),
                )
            })
            .await
    }

    async fn get_by_ids(&self, ids: &[AgentRunId]) -> AppResult<Vec<AgentRun>> {
        let ids: Vec<String> = ids.iter().map(|id| id.as_str().to_string()).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        self.db
            .run(move |conn| {
                let placeholders = std::iter::repeat_n("?", ids.len())
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql = format!(
                    "SELECT id, conversation_id, status, started_at, completed_at, error_message,
                            harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                            logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                            raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd,
                            approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                            action_kind, action_context_id, action_target_id,
                            persona_id, persona_slug, persona_version, persona_content_hash,
                            persona_injected, persona_skipped_reason
                     FROM agent_runs WHERE id IN ({placeholders})"
                );
                let mut stmt = conn.prepare(&sql)?;
                let runs = stmt
                    .query_map(params_from_iter(ids.iter()), |row| row_to_agent_run(row))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(runs)
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
                            logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                            raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd,
                            approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                            action_kind, action_context_id, action_target_id,
                            persona_id, persona_slug, persona_version, persona_content_hash,
                            persona_injected, persona_skipped_reason
                     FROM agent_runs WHERE conversation_id = ?1 ORDER BY started_at DESC LIMIT 1",
                    [&conversation_id],
                    |row| row_to_agent_run(row),
                )
            })
            .await
    }

    async fn get_latest_completed_for_provider_session(
        &self,
        conversation_id: &ChatConversationId,
        harness: AgentHarnessKind,
        provider_session_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        let conversation_id = conversation_id.as_str().to_string();
        let harness = harness.to_string();
        let provider_session_id = provider_session_id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, status, started_at, completed_at, error_message,
                            harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                            logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                            raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd,
                            approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                            action_kind, action_context_id, action_target_id,
                            persona_id, persona_slug, persona_version, persona_content_hash,
                            persona_injected, persona_skipped_reason
                     FROM agent_runs
                     WHERE conversation_id = ?1
                       AND harness = ?2
                       AND provider_session_id = ?3
                       AND status = 'completed'
                     ORDER BY started_at DESC LIMIT 1",
                    rusqlite::params![conversation_id, harness, provider_session_id],
                    row_to_agent_run,
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
                            logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                            raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd,
                            approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                            action_kind, action_context_id, action_target_id,
                            persona_id, persona_slug, persona_version, persona_content_hash,
                            persona_injected, persona_skipped_reason
                     FROM agent_runs WHERE conversation_id = ?1 AND status = 'running' ORDER BY started_at DESC LIMIT 1",
                    [&conversation_id],
                    |row| row_to_agent_run(row),
                )
            })
            .await
    }

    async fn get_latest_action(
        &self,
        conversation_id: &crate::domain::entities::ChatConversationId,
        action_kind: crate::domain::entities::AgentRunActionKind,
        action_context_id: &str,
        action_target_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        let conversation_id = conversation_id.as_str().to_string();
        let action_kind = action_kind.to_string();
        let action_context_id = action_context_id.to_string();
        let action_target_id = action_target_id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, status, started_at, completed_at, error_message,
                            harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                            logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                            raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd,
                            approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                            action_kind, action_context_id, action_target_id,
                            persona_id, persona_slug, persona_version, persona_content_hash,
                            persona_injected, persona_skipped_reason
                     FROM agent_runs
                     WHERE conversation_id = ?1 AND action_kind = ?2
                       AND action_context_id = ?3 AND action_target_id = ?4
                     ORDER BY started_at DESC LIMIT 1",
                    rusqlite::params![
                        conversation_id,
                        action_kind,
                        action_context_id,
                        action_target_id
                    ],
                    row_to_agent_run,
                )
            })
            .await
    }

    async fn get_active_action(
        &self,
        conversation_id: &crate::domain::entities::ChatConversationId,
        action_kind: crate::domain::entities::AgentRunActionKind,
        action_context_id: &str,
        action_target_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        let conversation_id = conversation_id.as_str().to_string();
        let action_kind = action_kind.to_string();
        let action_context_id = action_context_id.to_string();
        let action_target_id = action_target_id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, status, started_at, completed_at, error_message,
                            harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id,
                            logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                            raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd,
                            approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                            action_kind, action_context_id, action_target_id,
                            persona_id, persona_slug, persona_version, persona_content_hash,
                            persona_injected, persona_skipped_reason
                     FROM agent_runs
                     WHERE conversation_id = ?1 AND status = 'running'
                       AND action_kind = ?2 AND action_context_id = ?3 AND action_target_id = ?4
                     ORDER BY started_at DESC LIMIT 1",
                    rusqlite::params![
                        conversation_id,
                        action_kind,
                        action_context_id,
                        action_target_id
                    ],
                    row_to_agent_run,
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
                            logical_effort, effective_effort, service_tier, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, estimated_usd,
                            usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens,
                            raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd,
                            approval_policy, sandbox_mode, agent_name, launch_role, runtime_source, run_chain_id, parent_run_id,
                            action_kind, action_context_id, action_target_id,
                            persona_id, persona_slug, persona_version, persona_content_hash,
                            persona_injected, persona_skipped_reason
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
                    "UPDATE agent_runs
                     SET status = ?1,
                         completed_at = CASE WHEN ?1 = 'running' THEN NULL ELSE completed_at END,
                         error_message = CASE WHEN ?1 = 'running' THEN NULL ELSE error_message END
                     WHERE id = ?2",
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

    async fn replace_usage_capture(
        &self,
        id: &AgentRunId,
        capture: &UsageCapture,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let capture = capture.clone();
        self.db
            .run(move |conn| {
                let raw = capture.raw_snapshot.unwrap_or_default();
                let affected = conn.execute(
                    "UPDATE agent_runs
                     SET input_tokens = ?1,
                         output_tokens = ?2,
                         cache_creation_tokens = ?3,
                         cache_read_tokens = ?4,
                         estimated_usd = ?5,
                         usage_provenance = ?6,
                         raw_usage_input_tokens = ?7,
                         raw_usage_output_tokens = ?8,
                         raw_usage_cache_creation_tokens = ?9,
                         raw_usage_cache_read_tokens = ?10,
                         raw_usage_estimated_usd = ?11
                     WHERE id = ?12",
                    rusqlite::params![
                        capture.normalized.input_tokens,
                        capture.normalized.output_tokens,
                        capture.normalized.cache_creation_tokens,
                        capture.normalized.cache_read_tokens,
                        capture.normalized.estimated_usd,
                        capture.provenance.to_string(),
                        raw.input_tokens,
                        raw.output_tokens,
                        raw.cache_creation_tokens,
                        raw.cache_read_tokens,
                        raw.estimated_usd,
                        id,
                    ],
                )?;
                if affected == 0 {
                    return Err(crate::error::AppError::NotFound(format!(
                        "Agent run not found: {id}"
                    )));
                }
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
                         effective_effort = COALESCE(?8, effective_effort),
                         service_tier = COALESCE(?9, service_tier)
                     WHERE id = ?10",
                    rusqlite::params![
                        attribution.harness.map(|value| value.to_string()),
                        attribution.provider_session_id,
                        attribution.upstream_provider,
                        attribution.provider_profile,
                        attribution.logical_model,
                        attribution.effective_model_id,
                        attribution.logical_effort.map(|value| value.to_string()),
                        attribution.effective_effort,
                        attribution.service_tier,
                        id,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn set_persona_attribution(
        &self,
        id: &AgentRunId,
        attribution: PersonaRunAttribution,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_runs
                     SET persona_id = ?1,
                         persona_slug = ?2,
                         persona_version = ?3,
                         persona_content_hash = ?4,
                         persona_injected = ?5,
                         persona_skipped_reason = ?6
                     WHERE id = ?7",
                    rusqlite::params![
                        attribution.persona_id,
                        attribution.persona_slug,
                        attribution.persona_version,
                        attribution.persona_content_hash,
                        attribution.injected,
                        attribution.skipped_reason,
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

    async fn complete_if_running(&self, id: &AgentRunId) -> AppResult<bool> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE agent_runs
                     SET status = 'completed', completed_at = ?1, error_message = NULL
                     WHERE id = ?2 AND status = 'running'",
                    rusqlite::params![Utc::now().to_rfc3339(), id],
                )?;
                Ok(changed == 1)
            })
            .await
    }

    async fn complete_if_prune_cancelled(&self, id: &AgentRunId) -> AppResult<bool> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE agent_runs
                     SET status = 'completed', completed_at = ?1, error_message = NULL
                     WHERE id = ?2 AND status = 'cancelled' AND error_message = ?3",
                    rusqlite::params![Utc::now().to_rfc3339(), id, PRUNED_STALE_AGENT_RUN],
                )?;
                Ok(changed == 1)
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

    async fn cancel_with_reason(&self, id: &AgentRunId, reason: &str) -> AppResult<()> {
        let id = id.as_str().to_string();
        let reason = reason.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_runs SET status = 'cancelled', completed_at = ?1, error_message = ?2 WHERE id = ?3 AND status = 'running'",
                    rusqlite::params![Utc::now().to_rfc3339(), reason, id],
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
                let completed_at = Utc::now().to_rfc3339();
                let params = rusqlite::params![
                    completed_at,
                    ORPHANED_AGENT_RUN_ON_APP_RESTART,
                    cutoff
                ];
                let changes = conn.execute(
                    "UPDATE agent_runs SET status = 'cancelled', completed_at = ?1, error_message = ?2 WHERE status = 'running' AND started_at < ?3",
                    params,
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
                        c.bound_agent_name as conv_bound_agent_name,
                        c.persona_id as conv_persona_id,
                        c.builder_draft_id as conv_builder_draft_id,
                        c.builder_result_persona_id as conv_builder_result_persona_id,
                        c.coordination_mode as conv_coordination_mode,
                        c.automation_id,
                        c.automation_run_id,
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
                        ar.service_tier,
                        ar.input_tokens,
                        ar.output_tokens,
                        ar.cache_creation_tokens,
                        ar.cache_read_tokens,
                        ar.estimated_usd,
                        ar.usage_provenance,
                        ar.raw_usage_input_tokens,
                        ar.raw_usage_output_tokens,
                        ar.raw_usage_cache_creation_tokens,
                        ar.raw_usage_cache_read_tokens,
                        ar.raw_usage_estimated_usd,
                        ar.approval_policy,
                        ar.sandbox_mode,
                        ar.agent_name,
                        ar.launch_role,
                        ar.runtime_source,
                        ar.run_chain_id,
                        ar.parent_run_id,
                        ar.action_kind,
                        ar.action_context_id,
                        ar.action_target_id,
                        ar.persona_id,
                        ar.persona_slug,
                        ar.persona_version,
                        ar.persona_content_hash,
                        ar.persona_injected,
                        ar.persona_skipped_reason
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
                        let provider_profile: Option<String> = row.get("conv_provider_profile")?;
                        let bound_agent_name: Option<String> = row.get("conv_bound_agent_name")?;
                        let persona_id: Option<String> = row.get("conv_persona_id")?;
                        let builder_draft_id: Option<String> = row.get("conv_builder_draft_id")?;
                        let builder_result_persona_id: Option<String> =
                            row.get("conv_builder_result_persona_id")?;
                        let coordination_mode = row
                            .get::<_, Option<String>>("conv_coordination_mode")
                            .ok()
                            .flatten()
                            .and_then(|value| value.parse::<CoordinationMode>().ok())
                            .unwrap_or_default();
                        let conv_created_at_str: String = row.get("conv_created_at")?;
                        let conv_updated_at_str: String = row.get("conv_updated_at")?;
                        let last_message_at_str: Option<String> = row.get("last_message_at")?;
                        let mut conversation = ChatConversation {
                            id: ChatConversationId::from_string(row.get::<_, String>("conv_id")?),
                            context_type: context_type_str
                                .parse()
                                .unwrap_or(ChatContextType::Project),
                            context_id: row.get("context_id")?,
                            claude_session_id,
                            provider_session_id,
                            provider_harness,
                            upstream_provider,
                            provider_profile,
                            agent_mode: None,
                            bound_agent_name,
                            persona_id,
                            builder_draft_id,
                            builder_result_persona_id,
                            coordination_mode,
                            automation_id: row
                                .get::<_, Option<String>>("automation_id")?
                                .map(AutomationId::from_string),
                            automation_run_id: row
                                .get::<_, Option<String>>("automation_run_id")?
                                .map(AutomationRunId::from_string),
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
                            service_tier: row.get("service_tier")?,
                            input_tokens: row.get("input_tokens")?,
                            output_tokens: row.get("output_tokens")?,
                            cache_creation_tokens: row.get("cache_creation_tokens")?,
                            cache_read_tokens: row.get("cache_read_tokens")?,
                            estimated_usd: row.get("estimated_usd")?,
                            usage_provenance: parse_usage_provenance(row.get("usage_provenance")?)?,
                            raw_usage_snapshot: {
                                let snapshot = ProviderUsageSnapshot {
                                    input_tokens: row.get("raw_usage_input_tokens")?,
                                    output_tokens: row.get("raw_usage_output_tokens")?,
                                    cache_creation_tokens: row
                                        .get("raw_usage_cache_creation_tokens")?,
                                    cache_read_tokens: row.get("raw_usage_cache_read_tokens")?,
                                    estimated_usd: row.get("raw_usage_estimated_usd")?,
                                };
                                (!snapshot.as_usage().is_empty()).then_some(snapshot)
                            },
                            approval_policy: row.get("approval_policy")?,
                            sandbox_mode: row.get("sandbox_mode")?,
                            agent_name: row.get("agent_name")?,
                            launch_role: row.get("launch_role")?,
                            runtime_source: row.get("runtime_source")?,
                            run_chain_id: row.get("run_chain_id")?,
                            parent_run_id: row.get("parent_run_id")?,
                            action_kind: row
                                .get::<_, Option<String>>("action_kind")?
                                .and_then(|value| value.parse().ok()),
                            action_context_id: row.get("action_context_id")?,
                            action_target_id: row.get("action_target_id")?,
                            persona_id: row.get("persona_id")?,
                            persona_slug: row.get("persona_slug")?,
                            persona_version: row.get("persona_version")?,
                            persona_content_hash: row.get("persona_content_hash")?,
                            persona_injected: row
                                .get::<_, Option<i64>>("persona_injected")?
                                .map(|value| value != 0),
                            persona_skipped_reason: row.get("persona_skipped_reason")?,
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
