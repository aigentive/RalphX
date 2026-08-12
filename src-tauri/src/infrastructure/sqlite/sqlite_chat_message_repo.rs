// SQLite-based ChatMessageRepository implementation for production use
// Uses DbConnection for non-blocking SQLite access via spawn_blocking

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use crate::domain::agents::ProviderSessionRef;
use crate::domain::entities::{
    AgentRunUsage, ChatConversationId, ChatMessage, ChatMessageAttribution, ChatMessageId,
    IdeationSessionId, ProjectId, TaskId, UsageCapture,
};
use crate::domain::repositories::ChatMessageRepository;
use crate::error::AppResult;
use crate::infrastructure::sqlite::DbConnection;

/// SQLite implementation of ChatMessageRepository for production use
pub struct SqliteChatMessageRepository {
    pub(crate) db: DbConnection,
}

impl SqliteChatMessageRepository {
    /// Create a new SQLite chat message repository with the given connection
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
impl ChatMessageRepository for SqliteChatMessageRepository {
    async fn create(&self, message: ChatMessage) -> AppResult<ChatMessage> {
        let id = message.id.as_str().to_string();
        let session_id = message
            .session_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let project_id = message
            .project_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let task_id = message.task_id.as_ref().map(|id| id.as_str().to_string());
        let conversation_id = message
            .conversation_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let role = message.role.to_string();
        let content = message.content.clone();
        let metadata = message.metadata.clone();
        let parent_message_id = message
            .parent_message_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let tool_calls = message.tool_calls.clone();
        let content_blocks = message.content_blocks.clone();
        let attribution_source = message.attribution_source.clone();
        let provider_harness = message.provider_harness.map(|value| value.to_string());
        let provider_session_id = message.provider_session_id.clone();
        let upstream_provider = message.upstream_provider.clone();
        let provider_profile = message.provider_profile.clone();
        let logical_model = message.logical_model.clone();
        let effective_model_id = message.effective_model_id.clone();
        let logical_effort = message.logical_effort.map(|value| value.to_string());
        let effective_effort = message.effective_effort.clone();
        let input_tokens = message.input_tokens;
        let output_tokens = message.output_tokens;
        let cache_creation_tokens = message.cache_creation_tokens;
        let cache_read_tokens = message.cache_read_tokens;
        let estimated_usd = message.estimated_usd;
        let usage_provenance = message.usage_provenance.map(|value| value.to_string());
        let raw_usage_input_tokens = message
            .raw_usage_snapshot
            .as_ref()
            .and_then(|value| value.input_tokens);
        let raw_usage_output_tokens = message
            .raw_usage_snapshot
            .as_ref()
            .and_then(|value| value.output_tokens);
        let raw_usage_cache_creation_tokens = message
            .raw_usage_snapshot
            .as_ref()
            .and_then(|value| value.cache_creation_tokens);
        let raw_usage_cache_read_tokens = message
            .raw_usage_snapshot
            .as_ref()
            .and_then(|value| value.cache_read_tokens);
        let raw_usage_estimated_usd = message
            .raw_usage_snapshot
            .as_ref()
            .and_then(|value| value.estimated_usd);
        let created_at = message.created_at.to_rfc3339();

        self.db.run(move |conn| {
            conn.execute(
                "INSERT INTO chat_messages (id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32)",
                rusqlite::params![
                    id, session_id, project_id, task_id, conversation_id,
                    role, content, metadata, parent_message_id, tool_calls, content_blocks,
                    attribution_source, provider_harness, provider_session_id,
                    upstream_provider, provider_profile, logical_model, effective_model_id,
                    logical_effort, effective_effort, input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance,
                    raw_usage_input_tokens, raw_usage_output_tokens,
                    raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens,
                    raw_usage_estimated_usd, created_at,
                ],
            )?;
            Ok(())
        }).await?;

        Ok(message)
    }

    async fn get_by_id(&self, id: &ChatMessageId) -> AppResult<Option<ChatMessage>> {
        let id_str = id.as_str().to_string();
        self.db.query_optional(move |conn| {
            conn.query_row(
                "SELECT id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at
                 FROM chat_messages WHERE id = ?1",
                [&id_str],
                ChatMessage::from_row,
            )
        }).await
    }

    async fn get_by_session(&self, session_id: &IdeationSessionId) -> AppResult<Vec<ChatMessage>> {
        let session_id_str = session_id.as_str().to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at
                 FROM chat_messages WHERE session_id = ?1 ORDER BY created_at ASC, rowid ASC",
            )?;
            let messages = stmt
                .query_map([session_id_str], ChatMessage::from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(messages)
        }).await
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<ChatMessage>> {
        let project_id_str = project_id.as_str().to_string();
        self.db.run(move |conn| {
            // Get messages that belong to a project but NOT to a session (direct project chat)
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at
                 FROM chat_messages WHERE project_id = ?1 AND session_id IS NULL ORDER BY created_at ASC, rowid ASC",
            )?;
            let messages = stmt
                .query_map([project_id_str], ChatMessage::from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(messages)
        }).await
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<ChatMessage>> {
        let task_id_str = task_id.as_str().to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at
                 FROM chat_messages WHERE task_id = ?1 ORDER BY created_at ASC, rowid ASC",
            )?;
            let messages = stmt
                .query_map([task_id_str], ChatMessage::from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(messages)
        }).await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatMessage>> {
        let conv_id_str = conversation_id.as_str().to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at
                 FROM chat_messages WHERE conversation_id = ?1 ORDER BY created_at ASC, rowid ASC",
            )?;
            let messages = stmt
                .query_map([conv_id_str], ChatMessage::from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(messages)
        }).await
    }

    async fn get_recent_by_conversation_paginated(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let conv_id_str = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at
                     FROM chat_messages WHERE conversation_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2 OFFSET ?3",
                )?;
                let mut messages: Vec<ChatMessage> = stmt
                    .query_map(rusqlite::params![conv_id_str, limit, offset], |row| {
                        ChatMessage::from_row(row)
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                messages.reverse();
                Ok(messages)
            })
            .await
    }

    async fn delete_by_session(&self, session_id: &IdeationSessionId) -> AppResult<()> {
        let session_id_str = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM chat_messages WHERE session_id = ?1",
                    [session_id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete_by_project(&self, project_id: &ProjectId) -> AppResult<()> {
        let project_id_str = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM chat_messages WHERE project_id = ?1",
                    [project_id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        let task_id_str = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM chat_messages WHERE task_id = ?1",
                    [task_id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &ChatMessageId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM chat_messages WHERE id = ?1", [id_str])?;
                Ok(())
            })
            .await
    }

    async fn count_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        let session_id_str = session_id.as_str().to_string();
        self.db.run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1 AND role IN ('user', 'orchestrator')",
                [session_id_str],
                |row| row.get(0),
            )?;
            Ok(count as u32)
        }).await
    }

    async fn get_recent_by_session(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let session_id_str = session_id.as_str().to_string();
        self.db.run(move |conn| {
            // Get the most recent user/orchestrator messages, but return them in ascending order
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at
                 FROM chat_messages WHERE session_id = ?1 AND role IN ('user', 'orchestrator') ORDER BY created_at DESC, rowid DESC LIMIT ?2",
            )?;
            let mut messages: Vec<ChatMessage> = stmt
                .query_map(rusqlite::params![session_id_str, limit], |row| {
                    ChatMessage::from_row(row)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            // Reverse to return in ascending order (oldest to newest)
            messages.reverse();
            Ok(messages)
        }).await
    }

    async fn get_recent_by_session_paginated(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let session_id_str = session_id.as_str().to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, task_id, conversation_id, role, content, metadata, parent_message_id, tool_calls, content_blocks, attribution_source, provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, logical_effort, effective_effort, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, estimated_usd, usage_provenance, raw_usage_input_tokens, raw_usage_output_tokens, raw_usage_cache_creation_tokens, raw_usage_cache_read_tokens, raw_usage_estimated_usd, created_at
                 FROM chat_messages WHERE session_id = ?1 AND role IN ('user', 'orchestrator') ORDER BY created_at DESC, rowid DESC LIMIT ?2 OFFSET ?3",
            )?;
            let mut messages: Vec<ChatMessage> = stmt
                .query_map(rusqlite::params![session_id_str, limit, offset], |row| {
                    ChatMessage::from_row(row)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            // Reverse to return in ascending order (oldest to newest)
            messages.reverse();
            Ok(messages)
        }).await
    }

    async fn update_content(
        &self,
        id: &ChatMessageId,
        content: &str,
        tool_calls: Option<&str>,
        content_blocks: Option<&str>,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let content = content.to_string();
        let tool_calls = tool_calls.map(|s| s.to_string());
        let content_blocks = content_blocks.map(|s| s.to_string());
        self.db.run(move |conn| {
            conn.execute(
                "UPDATE chat_messages SET content = ?1, tool_calls = ?2, content_blocks = ?3 WHERE id = ?4",
                rusqlite::params![content, tool_calls, content_blocks, id_str],
            )?;
            Ok(())
        }).await
    }

    async fn update_provider_session_ref(
        &self,
        id: &ChatMessageId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let harness = session_ref.harness.to_string();
        let provider_session_id = session_ref.provider_session_id.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_messages
                     SET provider_harness = ?1,
                         provider_session_id = ?2
                     WHERE id = ?3",
                    rusqlite::params![harness, provider_session_id, id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_usage(&self, id: &ChatMessageId, usage: &AgentRunUsage) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let usage = usage.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_messages
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
                        id_str,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn replace_usage_capture(
        &self,
        id: &ChatMessageId,
        capture: &UsageCapture,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let capture = capture.clone();
        self.db
            .run(move |conn| {
                let raw = capture.raw_snapshot.unwrap_or_default();
                let affected = conn.execute(
                    "UPDATE chat_messages
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
                        "Chat message not found: {id}"
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn update_attribution(
        &self,
        id: &ChatMessageId,
        attribution: &ChatMessageAttribution,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let attribution = attribution.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_messages
                     SET attribution_source = COALESCE(?1, attribution_source),
                         provider_harness = COALESCE(?2, provider_harness),
                         provider_session_id = COALESCE(?3, provider_session_id),
                         upstream_provider = COALESCE(?4, upstream_provider),
                         provider_profile = COALESCE(?5, provider_profile),
                         logical_model = COALESCE(?6, logical_model),
                         effective_model_id = COALESCE(?7, effective_model_id),
                         logical_effort = COALESCE(?8, logical_effort),
                         effective_effort = COALESCE(?9, effective_effort)
                     WHERE id = ?10",
                    rusqlite::params![
                        attribution.attribution_source,
                        attribution.provider_harness.map(|value| value.to_string()),
                        attribution.provider_session_id,
                        attribution.upstream_provider,
                        attribution.provider_profile,
                        attribution.logical_model,
                        attribution.effective_model_id,
                        attribution.logical_effort.map(|value| value.to_string()),
                        attribution.effective_effort,
                        id_str,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn count_unread_assistant_messages(
        &self,
        session_id: &str,
        after_message_id: Option<&str>,
    ) -> AppResult<u32> {
        let session_id = session_id.to_string();
        let after_message_id = after_message_id.map(|s| s.to_string());
        self.db
            .run(move |conn| {
                let count: i64 = if let Some(ref cursor) = after_message_id {
                    conn.query_row(
                        "SELECT COUNT(*) FROM chat_messages \
                         WHERE session_id = ?1 \
                         AND role IN ('assistant', 'orchestrator') \
                         AND created_at > ( \
                             SELECT created_at FROM chat_messages WHERE id = ?2 \
                         )",
                        rusqlite::params![session_id, cursor],
                        |row| row.get(0),
                    )?
                } else {
                    conn.query_row(
                        "SELECT COUNT(*) FROM chat_messages \
                         WHERE session_id = ?1 \
                         AND role IN ('assistant', 'orchestrator')",
                        rusqlite::params![session_id],
                        |row| row.get(0),
                    )?
                };
                Ok(count as u32)
            })
            .await
    }

    async fn count_unread_messages(
        &self,
        session_id: &str,
        cursor_message_id: Option<&str>,
    ) -> AppResult<i64> {
        let session_id = session_id.to_string();
        let cursor_message_id = cursor_message_id.map(|s| s.to_string());
        self.db
            .run(move |conn| {
                let count: i64 = if let Some(ref cursor) = cursor_message_id {
                    conn.query_row(
                        "SELECT COUNT(*) FROM chat_messages \
                         WHERE session_id = ?1 \
                         AND role IN ('user', 'orchestrator') \
                         AND created_at > ( \
                             SELECT created_at FROM chat_messages WHERE id = ?2 \
                         )",
                        rusqlite::params![session_id, cursor],
                        |row| row.get(0),
                    )?
                } else {
                    conn.query_row(
                        "SELECT COUNT(*) FROM chat_messages \
                         WHERE session_id = ?1 \
                         AND role IN ('user', 'orchestrator')",
                        rusqlite::params![session_id],
                        |row| row.get(0),
                    )?
                };
                Ok(count)
            })
            .await
    }

    async fn get_latest_message_by_role(
        &self,
        session_id: &IdeationSessionId,
        role: &str,
    ) -> AppResult<Option<ChatMessage>> {
        let session_id_str = session_id.as_str().to_string();
        let role_str = role.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, session_id, project_id, task_id, conversation_id, role, content, \
                     metadata, parent_message_id, tool_calls, content_blocks, attribution_source, \
                     provider_harness, provider_session_id, upstream_provider, provider_profile, logical_model, effective_model_id, \
                     logical_effort, effective_effort, created_at \
                     FROM chat_messages \
                     WHERE session_id = ?1 AND role = ?2 \
                     ORDER BY created_at DESC, rowid DESC LIMIT 1",
                    rusqlite::params![session_id_str, role_str],
                    ChatMessage::from_row,
                )
            })
            .await
    }

    async fn exists_verification_result_in_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<bool> {
        let conv_id_str = conversation_id.as_str().to_string();
        let content_like_pattern = format!(
            "%{}%",
            crate::domain::verification_markers::VERIFICATION_RESULT_MARKER
        );
        let metadata_like_pattern = format!(
            "%\"{}\"%",
            crate::domain::verification_markers::VERIFICATION_RESULT_METADATA_KEY
        );
        self.db
            .run(move |conn| {
                let exists: bool = conn
                    .query_row(
                        "SELECT EXISTS(
                        SELECT 1 FROM chat_messages
                        WHERE conversation_id = ?1
                          AND (content LIKE ?2 OR metadata LIKE ?3)
                    )",
                        rusqlite::params![conv_id_str, content_like_pattern, metadata_like_pattern],
                        |row| row.get(0),
                    )
                    // Fail-safe: assume injected on any DB error to prevent double injection
                    .unwrap_or(true);
                Ok(exists)
            })
            .await
    }

    async fn get_first_user_message_by_context(
        &self,
        context_type: &str,
        context_id: &str,
    ) -> AppResult<Option<String>> {
        let context_type = context_type.to_string();
        let context_id = context_id.to_string();
        self.db
            .query_optional(move |conn| {
                let sql = match context_type.as_str() {
                    "ideation" => {
                        "SELECT content FROM chat_messages \
                         WHERE session_id = ?1 AND role = 'user' \
                         ORDER BY created_at ASC LIMIT 1"
                    }
                    "task" | "task_execution" => {
                        "SELECT content FROM chat_messages \
                         WHERE task_id = ?1 AND role = 'user' \
                         ORDER BY created_at ASC LIMIT 1"
                    }
                    "project" => {
                        "SELECT content FROM chat_messages \
                         WHERE project_id = ?1 AND session_id IS NULL AND role = 'user' \
                         ORDER BY created_at ASC LIMIT 1"
                    }
                    "standalone" => {
                        // Standalone conversations are self-keyed
                        // (context_id == conversation_id); the generic fallback
                        // below queries by session_id, which standalone messages
                        // never set — it would silently return None forever.
                        "SELECT content FROM chat_messages \
                         WHERE conversation_id = ?1 AND role = 'user' \
                         ORDER BY created_at ASC LIMIT 1"
                    }
                    _ => {
                        "SELECT content FROM chat_messages \
                         WHERE session_id = ?1 AND role = 'user' \
                         ORDER BY created_at ASC LIMIT 1"
                    }
                };
                conn.query_row(sql, rusqlite::params![context_id], |row| row.get(0))
            })
            .await
    }
}
