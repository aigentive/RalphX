use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use tokio::sync::Mutex;

use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRunId, ChatConversationId, ChatMessageId, ChatTimelineItem, ChatTimelineItemId,
    ChatTimelineItemKind, ChatTimelineItemStatus, ChatTimelinePage, MessageRole,
};
use crate::domain::repositories::ChatTimelineRepository;
use crate::error::AppResult;
use crate::infrastructure::sqlite::DbConnection;

pub struct SqliteChatTimelineRepository {
    db: DbConnection,
}

const RALPHX_TOOL_NAME_PREFIXES: [&str; 6] = [
    "mcp__ralphx__",
    "mcp__ralphx_internal__",
    "ralphx::",
    "ralphx_internal::",
    "ralphx:",
    "ralphx_internal:",
];
const DIFF_TOOL_NAMES: [&str; 2] = ["edit", "write"];
const ASK_USER_QUESTION_TOOL_NAME: &str = "ask_user_question";
const DELEGATION_TOOL_NAMES: [&str; 4] = [
    "delegate_start",
    "delegate_wait",
    "delegate_cancel",
    "delegate_terminal",
];

impl SqliteChatTimelineRepository {
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
impl ChatTimelineRepository for SqliteChatTimelineRepository {
    async fn upsert_item(&self, mut item: ChatTimelineItem) -> AppResult<ChatTimelineItem> {
        self.db
            .run_transaction(move |conn| {
                let existing = conn
                    .query_row(
                        "SELECT sequence, created_at FROM chat_message_blocks WHERE id = ?1",
                        [item.id.as_str()],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?;

                if let Some((sequence, created_at)) = existing {
                    item.sequence = sequence;
                    item.created_at = parse_datetime(&created_at);
                } else if item.sequence <= 0 {
                    item.sequence = conn.query_row(
                        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM chat_message_blocks WHERE conversation_id = ?1",
                        [item.conversation_id.as_str()],
                        |row| row.get(0),
                    )?;
                }

                let item = upsert_item(conn, item)?;
                Ok(item)
            })
            .await
    }

    async fn get_by_id(&self, id: &ChatTimelineItemId) -> AppResult<Option<ChatTimelineItem>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("{} WHERE b.id = ?1", timeline_item_select_sql(true)),
                    [id],
                    row_to_timeline_item,
                )
            })
            .await
    }

    async fn get_page(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        before_sequence: Option<i64>,
    ) -> AppResult<ChatTimelinePage> {
        let conversation_id = conversation_id.as_str();
        let normalized_limit = limit.clamp(1, 200);
        self.db
            .run(move |conn| {
                let total_item_count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM chat_message_blocks WHERE conversation_id = ?1",
                    [&conversation_id],
                    |row| row.get(0),
                )?;

                let page_limit = i64::from(normalized_limit);
                let mut items = if let Some(before_sequence) = before_sequence {
                    let mut stmt = conn.prepare(&format!(
                        "{} WHERE b.conversation_id = ?1 AND b.sequence < ?2 ORDER BY b.sequence DESC LIMIT ?3",
                        timeline_item_select_sql(false)
                    ))?;
                    let rows =
                        stmt.query_map(
                            params![conversation_id, before_sequence, page_limit],
                            row_to_timeline_item,
                        )?;
                    rows.collect::<Result<Vec<_>, _>>()?
                } else {
                    let mut stmt = conn.prepare(&format!(
                        "{} WHERE b.conversation_id = ?1 ORDER BY b.sequence DESC LIMIT ?2",
                        timeline_item_select_sql(false)
                    ))?;
                    let rows =
                        stmt.query_map(params![conversation_id, page_limit], row_to_timeline_item)?;
                    rows.collect::<Result<Vec<_>, _>>()?
                };
                items.reverse();
                hydrate_required_tool_payloads(conn, &mut items)?;

                let oldest_loaded_sequence = items.first().map(|item| item.sequence);
                let newest_loaded_sequence = items.last().map(|item| item.sequence);
                let has_older = oldest_loaded_sequence
                    .map(|sequence| {
                        conn.query_row(
                            "SELECT EXISTS(SELECT 1 FROM chat_message_blocks WHERE conversation_id = ?1 AND sequence < ?2)",
                            params![conversation_id, sequence],
                            |row| row.get::<_, i64>(0),
                        )
                    })
                    .transpose()?
                    .unwrap_or(0)
                    != 0;

                Ok(ChatTimelinePage {
                    items,
                    limit: normalized_limit,
                    before_sequence,
                    total_item_count: total_item_count as u32,
                    has_older,
                    oldest_loaded_sequence,
                    newest_loaded_sequence,
                })
            })
            .await
    }

    async fn count_by_conversation(&self, conversation_id: &ChatConversationId) -> AppResult<u32> {
        let conversation_id = conversation_id.as_str();
        self.db
            .run(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM chat_message_blocks WHERE conversation_id = ?1",
                    [&conversation_id],
                    |row| row.get(0),
                )?;
                Ok(count as u32)
            })
            .await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatTimelineItem>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "{} WHERE b.conversation_id = ?1 ORDER BY b.sequence ASC",
                    timeline_item_select_sql(true)
                ))?;
                let mut items = stmt
                    .query_map(params![conversation_id], row_to_timeline_item)?
                    .collect::<Result<Vec<_>, _>>()?;
                hydrate_required_tool_payloads(conn, &mut items)?;
                Ok(items)
            })
            .await
    }

    async fn delete_message_items_except_block_indices(
        &self,
        message_id: &ChatMessageId,
        retained_block_indices: Vec<i64>,
    ) -> AppResult<()> {
        let message_id = message_id.as_str().to_string();
        let retained_block_indices: std::collections::HashSet<i64> =
            retained_block_indices.into_iter().collect();
        self.db
            .run(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT block_index FROM chat_message_blocks WHERE message_id = ?1")?;
                let existing_indices = stmt
                    .query_map(params![message_id.as_str()], |row| row.get::<_, i64>(0))?
                    .collect::<Result<Vec<_>, _>>()?;

                for block_index in existing_indices {
                    if !retained_block_indices.contains(&block_index) {
                        conn.execute(
                            "DELETE FROM chat_message_blocks WHERE message_id = ?1 AND block_index = ?2",
                            params![message_id.as_str(), block_index],
                        )?;
                    }
                }
                Ok(())
            })
            .await
    }

    async fn mark_message_items_finalized(&self, message_id: &ChatMessageId) -> AppResult<()> {
        let message_id = message_id.as_str().to_string();
        let finalized_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    r#"
                    UPDATE chat_message_blocks
                    SET status = 'finalized',
                        updated_at = ?2,
                        finalized_at = COALESCE(finalized_at, ?2)
                    WHERE message_id = ?1
                    "#,
                    params![message_id, finalized_at],
                )?;
                Ok(())
            })
            .await
    }
}

fn hydrate_required_tool_payloads(
    conn: &Connection,
    items: &mut [ChatTimelineItem],
) -> rusqlite::Result<()> {
    if !items.iter().any(should_hydrate_full_tool_payload) {
        return Ok(());
    }

    let mut stmt = conn.prepare(
        r#"
        SELECT input_json, result_json, raw_block_json
        FROM chat_message_block_payloads
        WHERE block_id = ?1
        "#,
    )?;

    for item in items
        .iter_mut()
        .filter(|item| should_hydrate_full_tool_payload(item))
    {
        let payload = stmt
            .query_row(params![item.id.as_str()], |row| {
                Ok((
                    row.get::<_, Option<String>>("input_json")?,
                    row.get::<_, Option<String>>("result_json")?,
                    row.get::<_, Option<String>>("raw_block_json")?,
                ))
            })
            .optional()?;

        if let Some((input_json, result_json, raw_block_json)) = payload {
            item.input_json = input_json;
            item.result_json = result_json;
            item.raw_block_json = raw_block_json;
        }
    }

    Ok(())
}

fn should_hydrate_full_tool_payload(item: &ChatTimelineItem) -> bool {
    item.kind == ChatTimelineItemKind::ToolUse
        && item
            .tool_name
            .as_deref()
            .is_some_and(retains_full_raw_tool_payload)
}

#[doc(hidden)]
pub(crate) fn retains_full_raw_tool_payload(tool_name: &str) -> bool {
    let normalized = normalize_ralphx_tool_name(tool_name);
    let leaf_name = normalized.rsplit("::").next().unwrap_or(&normalized);
    DIFF_TOOL_NAMES.contains(&leaf_name)
        || normalized == ASK_USER_QUESTION_TOOL_NAME
        || DELEGATION_TOOL_NAMES.contains(&normalized.as_str())
}

fn normalize_ralphx_tool_name(tool_name: &str) -> String {
    let normalized = tool_name.trim().to_ascii_lowercase();
    RALPHX_TOOL_NAME_PREFIXES
        .iter()
        .find_map(|prefix| normalized.strip_prefix(prefix).map(str::to_string))
        .unwrap_or(normalized)
}

fn upsert_item(conn: &Connection, item: ChatTimelineItem) -> AppResult<ChatTimelineItem> {
    let mut item = item;
    if item.kind != ChatTimelineItemKind::ToolUse
        || !item
            .tool_name
            .as_deref()
            .is_some_and(retains_full_raw_tool_payload)
    {
        item.raw_block_json = None;
    }
    let id = item.id.as_str().to_string();
    let conversation_id = item.conversation_id.as_str();
    let message_id = item.message_id.as_ref().map(|id| id.as_str().to_string());
    let run_id = item.run_id.map(|id| id.as_str());
    let role = item.role.to_string();
    let kind = item.kind.to_string();
    let status = item.status.to_string();
    let provider_harness = item.provider_harness.map(|kind| kind.to_string());
    let created_at = item.created_at.to_rfc3339();
    let updated_at = item.updated_at.to_rfc3339();
    let finalized_at = item.finalized_at.map(|value| value.to_rfc3339());

    conn.execute(
        r#"
        INSERT INTO chat_message_blocks (
            id, conversation_id, message_id, run_id, sequence, block_index, role, kind, status,
            text, tool_call_id, tool_name, tool_status, tool_input_preview, tool_result_preview,
            metadata, provider_harness, provider_session_id, created_at, updated_at, finalized_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
        ON CONFLICT(id) DO UPDATE SET
            message_id = excluded.message_id,
            run_id = excluded.run_id,
            block_index = excluded.block_index,
            role = excluded.role,
            kind = excluded.kind,
            status = excluded.status,
            text = excluded.text,
            tool_call_id = excluded.tool_call_id,
            tool_name = excluded.tool_name,
            tool_status = excluded.tool_status,
            tool_input_preview = excluded.tool_input_preview,
            tool_result_preview = excluded.tool_result_preview,
            metadata = excluded.metadata,
            provider_harness = excluded.provider_harness,
            provider_session_id = excluded.provider_session_id,
            updated_at = excluded.updated_at,
            finalized_at = excluded.finalized_at
        "#,
        params![
            id,
            conversation_id,
            message_id,
            run_id,
            item.sequence,
            item.block_index,
            role,
            kind,
            status,
            item.text,
            item.tool_call_id,
            item.tool_name,
            item.tool_status,
            item.tool_input_preview,
            item.tool_result_preview,
            item.metadata,
            provider_harness,
            item.provider_session_id,
            created_at,
            updated_at,
            finalized_at,
        ],
    )?;

    if item.input_json.is_some() || item.result_json.is_some() || item.raw_block_json.is_some() {
        conn.execute(
            r#"
            INSERT INTO chat_message_block_payloads (
                block_id, input_json, result_json, raw_block_json, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(block_id) DO UPDATE SET
                input_json = excluded.input_json,
                result_json = excluded.result_json,
                raw_block_json = excluded.raw_block_json,
                updated_at = excluded.updated_at
            "#,
            params![
                item.id.as_str(),
                item.input_json,
                item.result_json,
                item.raw_block_json,
                updated_at,
            ],
        )?;
    } else {
        conn.execute(
            "DELETE FROM chat_message_block_payloads WHERE block_id = ?1",
            [item.id.as_str()],
        )?;
    }

    Ok(item)
}

fn timeline_item_select_sql(include_payload: bool) -> &'static str {
    if include_payload {
        r#"
        SELECT
            b.id, b.conversation_id, b.message_id, b.run_id, b.sequence, b.block_index, b.role,
            b.kind, b.status, b.text, b.tool_call_id, b.tool_name, b.tool_status,
            b.tool_input_preview, b.tool_result_preview, p.input_json, p.result_json,
            p.raw_block_json, b.metadata, b.provider_harness, b.provider_session_id, b.created_at,
            b.updated_at, b.finalized_at
        FROM chat_message_blocks b
        LEFT JOIN chat_message_block_payloads p ON p.block_id = b.id
        "#
    } else {
        r#"
        SELECT
            b.id, b.conversation_id, b.message_id, b.run_id, b.sequence, b.block_index, b.role,
            b.kind, b.status, b.text, b.tool_call_id, b.tool_name, b.tool_status,
            b.tool_input_preview, b.tool_result_preview, NULL AS input_json, NULL AS result_json,
            NULL AS raw_block_json, b.metadata, b.provider_harness, b.provider_session_id,
            b.created_at, b.updated_at, b.finalized_at
        FROM chat_message_blocks b
        "#
    }
}

fn row_to_timeline_item(row: &Row<'_>) -> rusqlite::Result<ChatTimelineItem> {
    let id: String = row.get("id")?;
    let conversation_id: String = row.get("conversation_id")?;
    let message_id: Option<String> = row.get("message_id")?;
    let run_id: Option<String> = row.get("run_id")?;
    let role: String = row.get("role")?;
    let kind: String = row.get("kind")?;
    let status: String = row.get("status")?;
    let provider_harness = row
        .get::<_, Option<String>>("provider_harness")?
        .and_then(|value| value.parse::<AgentHarnessKind>().ok());
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let finalized_at: Option<String> = row.get("finalized_at")?;

    Ok(ChatTimelineItem {
        id: ChatTimelineItemId::from_string(id),
        conversation_id: ChatConversationId::from_string(conversation_id),
        message_id: message_id.map(ChatMessageId::from_string),
        run_id: run_id.map(AgentRunId::from_string),
        sequence: row.get("sequence")?,
        block_index: row.get("block_index")?,
        role: MessageRole::from_str(&role).unwrap_or(MessageRole::Orchestrator),
        kind: ChatTimelineItemKind::from_str(&kind).unwrap_or(ChatTimelineItemKind::Text),
        status: ChatTimelineItemStatus::from_str(&status)
            .unwrap_or(ChatTimelineItemStatus::Finalized),
        text: row.get("text")?,
        tool_call_id: row.get("tool_call_id")?,
        tool_name: row.get("tool_name")?,
        tool_status: row.get("tool_status")?,
        tool_input_preview: row.get("tool_input_preview")?,
        tool_result_preview: row.get("tool_result_preview")?,
        input_json: row.get("input_json")?,
        result_json: row.get("result_json")?,
        raw_block_json: row.get("raw_block_json")?,
        metadata: row.get("metadata")?,
        provider_harness,
        provider_session_id: row.get("provider_session_id")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
        finalized_at: finalized_at.map(|value| parse_datetime(&value)),
    })
}

fn parse_datetime(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
