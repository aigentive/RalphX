// SQLite-based ChatConversationRepository implementation
// Uses DbConnection for non-blocking SQLite access via spawn_blocking

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;

use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::chat_conversation::compatible_provider_session_fields_from_provider_ref;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AttributionBackfillStatus, AutomationId, AutomationRunId,
    ChatContextType, ChatConversation, ChatConversationId, ConversationAttributionBackfillState,
    ConversationAttributionBackfillSummary, CoordinationMode,
};
use crate::domain::repositories::{ChatConversationPage, ChatConversationRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

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

fn row_to_conversation(row: &rusqlite::Row) -> rusqlite::Result<ChatConversation> {
    let context_type_str: String = row.get("context_type")?;
    let claude_session_id: Option<String> = row.get("claude_session_id")?;
    let provider_session_id: Option<String> = row.get("provider_session_id")?;
    let provider_harness = row
        .get::<_, Option<String>>("provider_harness")?
        .and_then(|value| value.parse::<AgentHarnessKind>().ok());
    let upstream_provider: Option<String> = row.get("upstream_provider")?;
    let provider_profile: Option<String> = row.get("provider_profile")?;
    let agent_mode = row
        .get::<_, Option<String>>("agent_mode")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<AgentConversationWorkspaceMode>().ok());
    let bound_agent_name: Option<String> = row.get("bound_agent_name")?;
    let persona_id: Option<String> = row.get("persona_id")?;
    let builder_draft_id: Option<String> = row.get("builder_draft_id")?;
    let builder_result_persona_id = row
        .get::<_, Option<String>>("builder_result_persona_id")
        .ok()
        .flatten();
    let coordination_mode = row
        .get::<_, Option<String>>("coordination_mode")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<CoordinationMode>().ok())
        .unwrap_or_default();
    let automation_id = row
        .get::<_, Option<String>>("automation_id")
        .ok()
        .flatten()
        .map(AutomationId::from_string);
    let automation_run_id = row
        .get::<_, Option<String>>("automation_run_id")
        .ok()
        .flatten()
        .map(AutomationRunId::from_string);
    let last_message_at_str: Option<String> = row.get("last_message_at")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;
    let archived_at = row
        .get::<_, Option<String>>("archived_at")
        .ok()
        .flatten()
        .map(|value| parse_datetime(&value));

    let created_at = parse_datetime(&created_at_str);
    let updated_at = parse_datetime(&updated_at_str);
    let attribution_backfill_last_attempted_at = row
        .get::<_, Option<String>>("attribution_backfill_last_attempted_at")
        .ok()
        .flatten()
        .map(|value| parse_datetime(&value));
    let attribution_backfill_completed_at = row
        .get::<_, Option<String>>("attribution_backfill_completed_at")
        .ok()
        .flatten()
        .map(|value| parse_datetime(&value));

    let mut conversation = ChatConversation {
        id: ChatConversationId::from_string(row.get::<_, String>("id")?),
        context_type: context_type_str
            .parse()
            .unwrap_or(ChatContextType::Ideation),
        context_id: row.get("context_id")?,
        claude_session_id,
        provider_session_id,
        provider_harness,
        upstream_provider,
        provider_profile,
        agent_mode,
        bound_agent_name,
        persona_id,
        builder_draft_id,
        builder_result_persona_id,
        coordination_mode,
        automation_id,
        automation_run_id,
        title: row.get("title")?,
        message_count: row.get("message_count")?,
        last_message_at: last_message_at_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }),
        created_at,
        updated_at,
        archived_at,
        parent_conversation_id: row.get("parent_conversation_id")?,
        attribution_backfill_status: row
            .get::<_, Option<String>>("attribution_backfill_status")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<AttributionBackfillStatus>().ok()),
        attribution_backfill_source: row.get("attribution_backfill_source").ok().flatten(),
        attribution_backfill_source_path: row
            .get("attribution_backfill_source_path")
            .ok()
            .flatten(),
        attribution_backfill_last_attempted_at,
        attribution_backfill_completed_at,
        attribution_backfill_error_summary: row
            .get("attribution_backfill_error_summary")
            .ok()
            .flatten(),
    };
    conversation.normalize_provider_session_fields();
    Ok(conversation)
}

// Consumed inside PersonaService::archive_persona's run_transaction (persona spec PR-6).
#[allow(dead_code)]
pub(crate) fn clear_persona_bindings_sync(
    conn: &rusqlite::Connection,
    persona_id: &str,
) -> AppResult<u64> {
    Ok(conn.execute(
        "UPDATE chat_conversations SET persona_id = NULL, updated_at = ?1 WHERE persona_id = ?2",
        rusqlite::params![Utc::now().to_rfc3339(), persona_id],
    )? as u64)
}

pub(crate) fn update_builder_draft_binding_sync(
    conn: &rusqlite::Connection,
    conversation_id: &str,
    builder_draft_id: Option<&str>,
) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE chat_conversations SET builder_draft_id = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![builder_draft_id, Utc::now().to_rfc3339(), conversation_id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::NotFound(format!(
            "Chat conversation not found: {conversation_id}"
        )));
    }
    Ok(())
}

pub(crate) fn claim_builder_draft_binding_sync(
    conn: &rusqlite::Connection,
    conversation_id: &str,
    builder_draft_id: &str,
) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE chat_conversations
         SET builder_draft_id = ?1, updated_at = ?2
         WHERE id = ?3
           AND builder_draft_id IS NULL
           AND builder_result_persona_id IS NULL",
        rusqlite::params![builder_draft_id, Utc::now().to_rfc3339(), conversation_id],
    )?;
    if changed == 1 {
        return Ok(());
    }
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_conversations WHERE id = ?1)",
        [conversation_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(crate::error::AppError::NotFound(format!(
            "Chat conversation not found: {conversation_id}"
        )));
    }
    Err(crate::error::AppError::Conflict(format!(
        "PersonaBuilder conversation {conversation_id} already has a draft or result binding"
    )))
}

pub(crate) fn clear_builder_draft_bindings_sync(
    conn: &rusqlite::Connection,
    draft_id: &str,
) -> AppResult<u64> {
    Ok(conn.execute(
        "UPDATE chat_conversations
         SET builder_draft_id = NULL, updated_at = ?1
         WHERE builder_draft_id = ?2",
        rusqlite::params![Utc::now().to_rfc3339(), draft_id],
    )? as u64)
}

pub(crate) fn finish_builder_binding_sync(
    conn: &rusqlite::Connection,
    draft_id: &str,
    result_persona_id: &str,
) -> AppResult<u64> {
    Ok(conn.execute(
        "UPDATE chat_conversations
         SET builder_draft_id = NULL, builder_result_persona_id = ?1, updated_at = ?2
         WHERE builder_draft_id = ?3",
        rusqlite::params![result_persona_id, Utc::now().to_rfc3339(), draft_id],
    )? as u64)
}

/// SQLite implementation of ChatConversationRepository
pub struct SqliteChatConversationRepository {
    db: DbConnection,
}

impl SqliteChatConversationRepository {
    /// Create a new SQLite chat conversation repository with the given connection
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
impl ChatConversationRepository for SqliteChatConversationRepository {
    async fn create(&self, conversation: ChatConversation) -> AppResult<ChatConversation> {
        if conversation.context_type == ChatContextType::Standalone
            && !conversation.is_valid_standalone_self_key()
        {
            return Err(AppError::Validation(
                "Standalone conversation context_id must equal its conversation id".to_string(),
            ));
        }
        let id = conversation.id.as_str().to_string();
        let context_type = conversation.context_type.to_string();
        let context_id = conversation.context_id.clone();
        let claude_session_id = conversation.claude_session_id.clone();
        let provider_session_id = conversation.provider_session_id.clone();
        let provider_harness = conversation.provider_harness.map(|value| value.to_string());
        let upstream_provider = conversation.upstream_provider.clone();
        let provider_profile = conversation.provider_profile.clone();
        let agent_mode = conversation.agent_mode.map(|value| value.to_string());
        let bound_agent_name = conversation.bound_agent_name.clone();
        let persona_id = conversation.persona_id.clone();
        let builder_draft_id = conversation.builder_draft_id.clone();
        let builder_result_persona_id = conversation.builder_result_persona_id.clone();
        let coordination_mode = conversation.coordination_mode.to_string();
        let automation_id = conversation
            .automation_id
            .as_ref()
            .map(|value| value.as_str().to_string());
        let automation_run_id = conversation
            .automation_run_id
            .as_ref()
            .map(|value| value.as_str().to_string());
        let title = conversation.title.clone();
        let message_count = conversation.message_count;
        let last_message_at = conversation.last_message_at.map(|dt| dt.to_rfc3339());
        let created_at = conversation.created_at.to_rfc3339();
        let updated_at = conversation.updated_at.to_rfc3339();
        let archived_at = conversation.archived_at.map(|dt| dt.to_rfc3339());
        let parent_conversation_id = conversation.parent_conversation_id.clone();
        let attribution_backfill_status = conversation
            .attribution_backfill_status
            .map(|value| value.to_string());
        let attribution_backfill_source = conversation.attribution_backfill_source.clone();
        let attribution_backfill_source_path =
            conversation.attribution_backfill_source_path.clone();
        let attribution_backfill_last_attempted_at = conversation
            .attribution_backfill_last_attempted_at
            .map(|value| value.to_rfc3339());
        let attribution_backfill_completed_at = conversation
            .attribution_backfill_completed_at
            .map(|value| value.to_rfc3339());
        let attribution_backfill_error_summary =
            conversation.attribution_backfill_error_summary.clone();

        self.db.run(move |conn| {
            conn.execute(
                "INSERT INTO chat_conversations (
                    id, context_type, context_id, claude_session_id, provider_session_id,
                    provider_harness, upstream_provider, provider_profile, agent_mode,
                    bound_agent_name, persona_id, builder_draft_id,
                    builder_result_persona_id, coordination_mode,
                    automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                    updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                    attribution_backfill_source, attribution_backfill_source_path,
                    attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                    attribution_backfill_error_summary
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29)",
                rusqlite::params![
                    id, context_type, context_id, claude_session_id, provider_session_id,
                    provider_harness, upstream_provider, provider_profile, agent_mode,
                    bound_agent_name, persona_id, builder_draft_id,
                    builder_result_persona_id, coordination_mode,
                    automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                    updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                    attribution_backfill_source, attribution_backfill_source_path,
                    attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                    attribution_backfill_error_summary,
                ],
            )?;
            Ok(())
        }).await?;

        Ok(conversation)
    }

    async fn get_by_id(&self, id: &ChatConversationId) -> AppResult<Option<ChatConversation>> {
        let id_str = id.as_str().to_string();
        self.db.query_optional(move |conn| {
            conn.query_row(
                "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                        provider_harness, upstream_provider, provider_profile, agent_mode, bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                        updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                        attribution_backfill_source, attribution_backfill_source_path,
                        attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                        attribution_backfill_error_summary
                 FROM chat_conversations WHERE id = ?1",
                [&id_str],
                row_to_conversation,
            )
        }).await
    }

    async fn get_by_builder_draft_id(
        &self,
        builder_draft_id: &str,
    ) -> AppResult<Option<ChatConversation>> {
        let builder_draft_id = builder_draft_id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                            provider_harness, upstream_provider, provider_profile, agent_mode,
                            bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id,
                            automation_run_id, title, message_count, last_message_at, created_at,
                            updated_at, archived_at, parent_conversation_id,
                            attribution_backfill_status, attribution_backfill_source,
                            attribution_backfill_source_path, attribution_backfill_last_attempted_at,
                            attribution_backfill_completed_at, attribution_backfill_error_summary
                     FROM chat_conversations
                     WHERE builder_draft_id = ?1 AND archived_at IS NULL
                     ORDER BY created_at DESC, id DESC LIMIT 1",
                    [builder_draft_id],
                    row_to_conversation,
                )
            })
            .await
    }

    async fn get_by_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<Vec<ChatConversation>> {
        let context_type_str = context_type.to_string();
        let context_id_str = context_id.to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                        provider_harness, upstream_provider, provider_profile, agent_mode, bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                        updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                        attribution_backfill_source, attribution_backfill_source_path,
                        attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                        attribution_backfill_error_summary
                 FROM chat_conversations WHERE context_type = ?1 AND context_id = ?2 AND archived_at IS NULL ORDER BY created_at DESC",
            )?;
            let conversations = stmt
                .query_map([context_type_str, context_id_str], row_to_conversation)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(conversations)
        }).await
    }

    async fn list_by_automation_id(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Vec<ChatConversation>> {
        let automation_id_str = automation_id.as_str().to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                        provider_harness, upstream_provider, provider_profile, agent_mode, bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                        updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                        attribution_backfill_source, attribution_backfill_source_path,
                        attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                        attribution_backfill_error_summary
                 FROM chat_conversations WHERE automation_id = ?1 ORDER BY created_at DESC",
            )?;
            let conversations = stmt
                .query_map([automation_id_str], row_to_conversation)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(conversations)
        }).await
    }

    async fn get_by_context_filtered(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        include_archived: bool,
    ) -> AppResult<Vec<ChatConversation>> {
        let context_type_str = context_type.to_string();
        let context_id_str = context_id.to_string();
        self.db.run(move |conn| {
            let archived_filter = if include_archived {
                ""
            } else {
                " AND archived_at IS NULL"
            };
            let sql = format!(
                "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                        provider_harness, upstream_provider, provider_profile, agent_mode, bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                        updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                        attribution_backfill_source, attribution_backfill_source_path,
                        attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                        attribution_backfill_error_summary
                 FROM chat_conversations WHERE context_type = ?1 AND context_id = ?2{} ORDER BY created_at DESC",
                archived_filter
            );
            let mut stmt = conn.prepare(&sql)?;
            let conversations = stmt
                .query_map([context_type_str, context_id_str], row_to_conversation)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(conversations)
        }).await
    }

    async fn get_by_context_page_filtered(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        include_archived: bool,
        archived_only: bool,
        offset: u32,
        limit: u32,
        search: Option<&str>,
    ) -> AppResult<ChatConversationPage> {
        let context_type_str = context_type.to_string();
        let context_id_str = context_id.to_string();
        let normalized_search = search
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_lowercase());

        self.db
            .run(move |conn| {
                let archived_filter = if archived_only {
                    " AND archived_at IS NOT NULL"
                } else if include_archived {
                    ""
                } else {
                    " AND archived_at IS NULL"
                };
                let search_filter = if normalized_search.is_some() {
                    " AND LOWER(COALESCE(title, 'Untitled agent')) LIKE ?3"
                } else {
                    ""
                };

                let count_sql = format!(
                    "SELECT COUNT(*)
                     FROM chat_conversations
                     WHERE context_type = ?1 AND context_id = ?2{}{}",
                    archived_filter, search_filter
                );
                let list_sql = format!(
                    "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                            provider_harness, upstream_provider, provider_profile, agent_mode, bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                            updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                            attribution_backfill_source, attribution_backfill_source_path,
                            attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                            attribution_backfill_error_summary
                     FROM chat_conversations
                     WHERE context_type = ?1 AND context_id = ?2{}{}
                     ORDER BY created_at DESC
                     LIMIT ?{} OFFSET ?{}",
                    archived_filter,
                    search_filter,
                    if normalized_search.is_some() { 4 } else { 3 },
                    if normalized_search.is_some() { 5 } else { 4 }
                );

                let total_count = if let Some(search_term) = normalized_search.as_deref() {
                    let search_pattern = format!("%{search_term}%");
                    conn.query_row(
                        &count_sql,
                        rusqlite::params![&context_type_str, &context_id_str, &search_pattern],
                        |row| row.get::<_, i64>(0),
                    )?
                } else {
                    conn.query_row(
                        &count_sql,
                        rusqlite::params![&context_type_str, &context_id_str],
                        |row| row.get::<_, i64>(0),
                    )?
                };

                let mut stmt = conn.prepare(&list_sql)?;
                let conversations = if let Some(search_term) = normalized_search.as_deref() {
                    let search_pattern = format!("%{search_term}%");
                    stmt.query_map(
                        rusqlite::params![
                            &context_type_str,
                            &context_id_str,
                            &search_pattern,
                            i64::from(limit),
                            i64::from(offset)
                        ],
                        row_to_conversation,
                    )?
                    .collect::<Result<Vec<_>, _>>()?
                } else {
                    stmt.query_map(
                        rusqlite::params![
                            &context_type_str,
                            &context_id_str,
                            i64::from(limit),
                            i64::from(offset)
                        ],
                        row_to_conversation,
                    )?
                    .collect::<Result<Vec<_>, _>>()?
                };

                Ok(ChatConversationPage {
                    conversations,
                    total_count,
                    offset,
                    limit,
                })
            })
            .await
    }

    async fn get_active_for_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<Option<ChatConversation>> {
        let context_type_str = context_type.to_string();
        let context_id_str = context_id.to_string();
        self.db.query_optional(move |conn| {
            conn.query_row(
                "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                        provider_harness, upstream_provider, provider_profile, agent_mode, bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                        updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                        attribution_backfill_source, attribution_backfill_source_path,
                        attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                        attribution_backfill_error_summary
                 FROM chat_conversations WHERE context_type = ?1 AND context_id = ?2 AND archived_at IS NULL ORDER BY created_at DESC LIMIT 1",
                [context_type_str, context_id_str],
                row_to_conversation,
            )
        }).await
    }

    async fn list_recent_resumable_by_context_type(
        &self,
        context_type: ChatContextType,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>> {
        let context_type_str = context_type.to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                        provider_harness, upstream_provider, provider_profile, agent_mode, bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                        updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                        attribution_backfill_source, attribution_backfill_source_path,
                        attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                        attribution_backfill_error_summary
                 FROM chat_conversations
                 WHERE context_type = ?1
                   AND archived_at IS NULL
                   AND (provider_session_id IS NOT NULL OR claude_session_id IS NOT NULL)
                 ORDER BY COALESCE(last_message_at, updated_at, created_at) DESC
                 LIMIT ?2",
            )?;
            let conversations = stmt
                .query_map(
                    rusqlite::params![context_type_str, i64::from(limit)],
                    row_to_conversation,
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(conversations)
        }).await
    }

    async fn list_by_context_type(
        &self,
        context_type: ChatContextType,
        include_archived: bool,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>> {
        let context_type_str = context_type.to_string();
        self.db.run(move |conn| {
            let sql = if include_archived {
                "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                        provider_harness, upstream_provider, provider_profile, agent_mode, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                        updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                        attribution_backfill_source, attribution_backfill_source_path,
                        attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                        attribution_backfill_error_summary
                 FROM chat_conversations
                 WHERE context_type = ?1
                 ORDER BY COALESCE(last_message_at, updated_at, created_at) DESC
                 LIMIT ?2"
            } else {
                "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                        provider_harness, upstream_provider, provider_profile, agent_mode, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                        updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                        attribution_backfill_source, attribution_backfill_source_path,
                        attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                        attribution_backfill_error_summary
                 FROM chat_conversations
                 WHERE context_type = ?1
                   AND archived_at IS NULL
                 ORDER BY COALESCE(last_message_at, updated_at, created_at) DESC
                 LIMIT ?2"
            };
            let mut stmt = conn.prepare(sql)?;
            let conversations = stmt
                .query_map(
                    rusqlite::params![context_type_str, i64::from(limit)],
                    row_to_conversation,
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(conversations)
        }).await
    }

    async fn update_provider_session_ref(
        &self,
        id: &ChatConversationId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let (claude_session_id, provider_session_id, provider_harness) =
            compatible_provider_session_fields_from_provider_ref(
                Some(session_ref.harness),
                Some(session_ref.provider_session_id.clone()),
            );
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                 SET claude_session_id = ?1,
                     provider_session_id = ?2,
                     provider_harness = ?3,
                     updated_at = ?4
                 WHERE id = ?5",
                    rusqlite::params![
                        claude_session_id,
                        provider_session_id,
                        provider_harness.map(|value| value.to_string()),
                        Utc::now().to_rfc3339(),
                        id_str
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn clear_provider_session_ref(&self, id: &ChatConversationId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                 SET claude_session_id = NULL,
                     provider_session_id = NULL,
                     provider_harness = NULL,
                     updated_at = ?1
                 WHERE id = ?2",
                    rusqlite::params![Utc::now().to_rfc3339(), id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_provider_origin(
        &self,
        id: &ChatConversationId,
        upstream_provider: Option<&str>,
        provider_profile: Option<&str>,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let upstream_provider = upstream_provider.map(str::to_string);
        let provider_profile = provider_profile.map(str::to_string);
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                 SET upstream_provider = ?1,
                     provider_profile = ?2,
                     updated_at = ?3
                 WHERE id = ?4",
                    rusqlite::params![
                        upstream_provider,
                        provider_profile,
                        Utc::now().to_rfc3339(),
                        id_str
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_agent_mode(
        &self,
        id: &ChatConversationId,
        mode: Option<AgentConversationWorkspaceMode>,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let mode = mode.map(|value| value.to_string());
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                 SET agent_mode = ?1,
                     updated_at = ?2
                 WHERE id = ?3",
                    rusqlite::params![mode, Utc::now().to_rfc3339(), id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_bound_agent_name(
        &self,
        id: &ChatConversationId,
        bound_agent_name: Option<&str>,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let bound_agent_name = bound_agent_name.map(str::to_string);
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE chat_conversations
                     SET bound_agent_name = ?2, updated_at = ?3
                     WHERE id = ?1",
                    rusqlite::params![id, bound_agent_name, updated_at],
                )?;
                if changed == 0 {
                    return Err(crate::error::AppError::NotFound(
                        "Chat conversation not found".to_string(),
                    ));
                }
                Ok(())
            })
            .await
    }

    async fn update_persona_binding(
        &self,
        id: &ChatConversationId,
        persona_id: Option<&str>,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let persona_id = persona_id.map(str::to_string);
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                     SET persona_id = ?1, updated_at = ?2
                     WHERE id = ?3",
                    rusqlite::params![persona_id, Utc::now().to_rfc3339(), id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_builder_draft_binding(
        &self,
        id: &ChatConversationId,
        builder_draft_id: Option<&str>,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let builder_draft_id = builder_draft_id.map(str::to_string);
        self.db
            .run(move |conn| {
                update_builder_draft_binding_sync(conn, &id_str, builder_draft_id.as_deref())
            })
            .await
    }

    async fn update_coordination_mode(
        &self,
        id: &ChatConversationId,
        mode: CoordinationMode,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let mode = mode.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                 SET coordination_mode = ?1,
                     updated_at = ?2
                 WHERE id = ?3",
                    rusqlite::params![mode, Utc::now().to_rfc3339(), id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_role_default_bindings(
        &self,
        id: &ChatConversationId,
        mode: CoordinationMode,
        persona_id: Option<&str>,
        clear_provider_session: bool,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let mode = mode.to_string();
        let persona_id = persona_id.map(str::to_string);
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                     SET coordination_mode = ?1,
                         persona_id = ?2,
                         claude_session_id = CASE WHEN ?3 THEN NULL ELSE claude_session_id END,
                         provider_session_id = CASE WHEN ?3 THEN NULL ELSE provider_session_id END,
                         provider_harness = CASE WHEN ?3 THEN NULL ELSE provider_harness END,
                         updated_at = ?4
                     WHERE id = ?5",
                    rusqlite::params![
                        mode,
                        persona_id,
                        clear_provider_session,
                        Utc::now().to_rfc3339(),
                        id_str
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_agent_mode_and_role_default_bindings(
        &self,
        id: &ChatConversationId,
        agent_mode: AgentConversationWorkspaceMode,
        coordination_mode: CoordinationMode,
        persona_id: Option<&str>,
        clear_provider_session: bool,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let agent_mode = agent_mode.to_string();
        let coordination_mode = coordination_mode.to_string();
        let persona_id = persona_id.map(str::to_string);
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE chat_conversations
                     SET agent_mode = ?2,
                         coordination_mode = ?3,
                         persona_id = ?4,
                         claude_session_id = CASE WHEN ?5 THEN NULL ELSE claude_session_id END,
                         provider_session_id = CASE WHEN ?5 THEN NULL ELSE provider_session_id END,
                         provider_harness = CASE WHEN ?5 THEN NULL ELSE provider_harness END,
                         updated_at = ?6
                     WHERE id = ?1",
                    rusqlite::params![
                        id,
                        agent_mode,
                        coordination_mode,
                        persona_id,
                        clear_provider_session,
                        Utc::now().to_rfc3339(),
                    ],
                )?;
                if changed == 0 {
                    return Err(crate::error::AppError::NotFound(
                        "Chat conversation not found".to_string(),
                    ));
                }
                Ok(())
            })
            .await
    }

    async fn update_title(&self, id: &ChatConversationId, title: &str) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let title = title.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![title, Utc::now().to_rfc3339(), id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn archive(&self, id: &ChatConversationId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE chat_conversations
                 SET archived_at = ?1, updated_at = ?1
                 WHERE id = ?2 AND archived_at IS NULL",
                    rusqlite::params![now, id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn restore(&self, id: &ChatConversationId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                 SET archived_at = NULL, updated_at = ?1
                 WHERE id = ?2",
                    rusqlite::params![Utc::now().to_rfc3339(), id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_message_stats(
        &self,
        id: &ChatConversationId,
        message_count: i64,
        last_message_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let last_message_at_str = last_message_at.to_rfc3339();
        self.db.run(move |conn| {
            conn.execute(
                "UPDATE chat_conversations SET message_count = ?1, last_message_at = ?2, updated_at = ?3 WHERE id = ?4",
                rusqlite::params![message_count, last_message_at_str, Utc::now().to_rfc3339(), id_str],
            )?;
            Ok(())
        }).await
    }

    async fn list_needing_attribution_backfill(
        &self,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, context_type, context_id, claude_session_id, provider_session_id,
                            provider_harness, upstream_provider, provider_profile, agent_mode, bound_agent_name, persona_id, builder_draft_id, builder_result_persona_id, coordination_mode, automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                            updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                            attribution_backfill_source, attribution_backfill_source_path,
                            attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                            attribution_backfill_error_summary
                     FROM chat_conversations
                     WHERE claude_session_id IS NOT NULL
                       AND (
                           attribution_backfill_status IS NULL
                           OR attribution_backfill_status = 'pending'
                       )
                     ORDER BY COALESCE(attribution_backfill_last_attempted_at, created_at) ASC,
                              created_at ASC
                     LIMIT ?1",
                )?;
                let conversations = stmt
                    .query_map([limit], row_to_conversation)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(conversations)
            })
            .await
    }

    async fn reset_running_attribution_backfill_to_pending(&self) -> AppResult<u64> {
        self.db
            .run(move |conn| {
                let updated = conn.execute(
                    "UPDATE chat_conversations
                     SET attribution_backfill_status = 'pending',
                         attribution_backfill_completed_at = NULL,
                         updated_at = ?1
                     WHERE claude_session_id IS NOT NULL
                       AND attribution_backfill_status = 'running'",
                    rusqlite::params![Utc::now().to_rfc3339()],
                )?;
                Ok(updated as u64)
            })
            .await
    }

    async fn update_attribution_backfill_state(
        &self,
        id: &ChatConversationId,
        state: ConversationAttributionBackfillState,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                     SET attribution_backfill_status = ?1,
                         attribution_backfill_source = ?2,
                         attribution_backfill_source_path = ?3,
                         attribution_backfill_last_attempted_at = ?4,
                         attribution_backfill_completed_at = ?5,
                         attribution_backfill_error_summary = ?6,
                         updated_at = ?7
                     WHERE id = ?8",
                    rusqlite::params![
                        state.status.map(|value| value.to_string()),
                        state.source,
                        state.source_path,
                        state.last_attempted_at.map(|value| value.to_rfc3339()),
                        state.completed_at.map(|value| value.to_rfc3339()),
                        state.error_summary,
                        Utc::now().to_rfc3339(),
                        id_str,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_attribution_backfill_summary(
        &self,
    ) -> AppResult<ConversationAttributionBackfillSummary> {
        self.db
            .run(move |conn| {
                Ok(conn.query_row(
                    "SELECT
                        COUNT(*) AS eligible_conversation_count,
                        COALESCE(SUM(CASE
                            WHEN attribution_backfill_status IS NULL OR attribution_backfill_status = 'pending'
                            THEN 1 ELSE 0 END), 0) AS pending_count,
                        COALESCE(SUM(CASE WHEN attribution_backfill_status = 'running' THEN 1 ELSE 0 END), 0) AS running_count,
                        COALESCE(SUM(CASE WHEN attribution_backfill_status = 'completed' THEN 1 ELSE 0 END), 0) AS completed_count,
                        COALESCE(SUM(CASE WHEN attribution_backfill_status = 'partial' THEN 1 ELSE 0 END), 0) AS partial_count,
                        COALESCE(SUM(CASE WHEN attribution_backfill_status = 'session_not_found' THEN 1 ELSE 0 END), 0) AS session_not_found_count,
                        COALESCE(SUM(CASE WHEN attribution_backfill_status = 'parse_failed' THEN 1 ELSE 0 END), 0) AS parse_failed_count
                     FROM chat_conversations
                     WHERE claude_session_id IS NOT NULL",
                    [],
                    |row| {
                        Ok(ConversationAttributionBackfillSummary {
                            eligible_conversation_count: row.get("eligible_conversation_count")?,
                            pending_count: row.get("pending_count")?,
                            running_count: row.get("running_count")?,
                            completed_count: row.get("completed_count")?,
                            partial_count: row.get("partial_count")?,
                            session_not_found_count: row.get("session_not_found_count")?,
                            parse_failed_count: row.get("parse_failed_count")?,
                        })
                    },
                )?)
            })
            .await
    }

    async fn delete(&self, id: &ChatConversationId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM chat_conversations WHERE id = ?1", [id_str])?;
                Ok(())
            })
            .await
    }

    async fn delete_by_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<()> {
        let context_type_str = context_type.to_string();
        let context_id_str = context_id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM chat_conversations WHERE context_type = ?1 AND context_id = ?2",
                    [context_type_str, context_id_str],
                )?;
                Ok(())
            })
            .await
    }
}
