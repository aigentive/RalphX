// Chat Service Types and Event Payloads
//
// Extracted from chat_service.rs to improve modularity and reduce file size.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::chat_conversation::compatible_provider_session_fields_from_provider_ref;
use crate::domain::entities::{ChatConversation, ChatMessage, ChatTimelineItem};
use crate::infrastructure::agents::claude::ToolCall;

use super::tool_result_preview::{LiveToolResultPreview, ToolArgumentPreviewPayload};

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

/// Whether a timeline block must retain its original raw JSON for renderer-specific hydration.
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

// ============================================================================
// Event Name Constants
// ============================================================================
// Unified event names for all chat-related events.
// Use these constants instead of hardcoding event strings.

/// Unified events (new API - includes context_type in payload)
pub mod events {
    /// Agent text chunk event
    pub const AGENT_CHUNK: &str = "agent:chunk";
    /// Agent reasoning chunk event
    pub const AGENT_THINKING: &str = "agent:thinking";
    /// Agent tool call event
    pub const AGENT_TOOL_CALL: &str = "agent:tool_call";
    /// Agent run started event
    pub const AGENT_RUN_STARTED: &str = "agent:run_started";
    /// Agent run completed event
    pub const AGENT_RUN_COMPLETED: &str = "agent:run_completed";
    /// Agent turn completed event (interactive mode: turn done but process still alive)
    pub const AGENT_TURN_COMPLETED: &str = "agent:turn_completed";
    /// Agent usage updated event (live mid-turn usage persisted)
    pub const AGENT_USAGE_UPDATED: &str = "agent:usage_updated";
    /// Agent message created event
    pub const AGENT_MESSAGE_CREATED: &str = "agent:message_created";
    /// Agent error event
    pub const AGENT_ERROR: &str = "agent:error";
    /// Agent queue sent event
    pub const AGENT_QUEUE_SENT: &str = "agent:queue_sent";
    /// Agent message queued event (message entered the queue, agent already running)
    pub const AGENT_MESSAGE_QUEUED: &str = "agent:message_queued";
    /// Activity stream message event (for execution bar)
    pub const AGENT_MESSAGE: &str = "agent:message";
    /// Agent task (subagent) started event
    pub const AGENT_TASK_STARTED: &str = "agent:task_started";
    /// Agent task (subagent) completed event
    pub const AGENT_TASK_COMPLETED: &str = "agent:task_completed";
    /// Agent hook event (started/completed/block)
    pub const AGENT_HOOK: &str = "agent:hook";

    /// Team artifact created event
    pub const TEAM_ARTIFACT_CREATED: &str = "team:artifact_created";
}

// ============================================================================
// Types
// ============================================================================

/// Context indicating who initiated a `send_message` call.
///
/// Controls whether a `SpawnFailed` error on an ideation context is caught-and-persisted
/// (UserInitiated) or propagated directly (DrainService).  The distinction prevents an
/// infinite drain loop: if the drain service already called `send_message`, capacity is
/// still full — persisting the prompt again and returning `Ok` would cause the drain to
/// re-claim the same session on the next tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SendCallerContext {
    /// User-initiated send (frontend / HTTP handler).
    /// On ideation capacity full → persist message as `pending_initial_prompt` and return
    /// `Ok(SendResult { queued_as_pending: true })`.
    #[default]
    UserInitiated,
    /// Drain-service-initiated send.
    /// On ideation capacity full → return `Err(SpawnFailed)` so the drain service breaks cleanly.
    DrainService,
    /// Startup/recovery-initiated send.
    /// Must not roll over, repair, or spawn a terminal Agent workspace automatically.
    StartupResumption,
}

const PENDING_PROMPT_ENVELOPE_PREFIX: &str = "ralphx-pending-prompt-v1:";

#[derive(Debug, Serialize, Deserialize)]
struct PendingPromptEnvelope {
    message: String,
    metadata: String,
}

pub(crate) fn encode_pending_initial_prompt(message: &str, metadata: Option<&str>) -> String {
    let Some(metadata) = metadata else {
        return message.to_string();
    };
    let envelope = PendingPromptEnvelope {
        message: message.to_string(),
        metadata: metadata.to_string(),
    };
    match serde_json::to_string(&envelope) {
        Ok(payload) => format!("{PENDING_PROMPT_ENVELOPE_PREFIX}{payload}"),
        Err(_) => message.to_string(),
    }
}

pub(crate) fn decode_pending_initial_prompt(payload: &str) -> (String, Option<String>) {
    let Some(encoded) = payload.strip_prefix(PENDING_PROMPT_ENVELOPE_PREFIX) else {
        return (payload.to_string(), None);
    };
    match serde_json::from_str::<PendingPromptEnvelope>(encoded) {
        Ok(envelope) => (envelope.message, Some(envelope.metadata)),
        Err(_) => (payload.to_string(), None),
    }
}

/// Result from sending a message (returns immediately while processing continues in background)
#[derive(Debug, Clone, Serialize, Default)]
pub struct SendResult {
    /// The conversation ID for this chat
    pub conversation_id: String,
    /// The agent run ID tracking this execution
    pub agent_run_id: String,
    /// Whether this is a new conversation (first message)
    pub is_new_conversation: bool,
    /// Whether the message was queued (Gate 2 blocked — agent already running)
    pub was_queued: bool,
    /// The queued message ID if was_queued is true
    pub queued_message_id: Option<String>,
    /// Whether the message was persisted as `pending_initial_prompt` because an idle
    /// ideation launch was deferred by pause or capacity. Distinct from a volatile
    /// running-agent queue entry.
    pub queued_as_pending: bool,
}

/// A conversation with its messages
#[derive(Debug, Clone)]
pub struct ChatConversationWithMessages {
    pub conversation: ChatConversation,
    pub messages: Vec<ChatMessage>,
}

// ============================================================================
// Unified Event Payloads (agent:* namespace)
// ============================================================================

/// Payload for agent:run_started event
#[derive(Debug, Clone, Serialize)]
pub struct AgentRunStartedPayload {
    pub run_id: String,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_chain_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// The resolved Claude model ID used for this run (e.g. "claude-sonnet-4-6").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model_id: Option<String>,
    /// Human-readable label for the effective model (e.g. "Sonnet 4.6").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_harness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
}

impl AgentRunStartedPayload {
    #[allow(clippy::too_many_arguments)]
    pub fn with_provider_session(
        run_id: impl Into<String>,
        conversation_id: impl Into<String>,
        context_type: impl Into<String>,
        context_id: impl Into<String>,
        run_chain_id: Option<String>,
        parent_run_id: Option<String>,
        effective_model_id: Option<String>,
        effective_model_label: Option<String>,
        harness: Option<AgentHarnessKind>,
        provider_session_id: Option<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            conversation_id: conversation_id.into(),
            context_type: context_type.into(),
            context_id: context_id.into(),
            run_chain_id,
            parent_run_id,
            agent_name: None,
            launch_role: None,
            started_at: None,
            effective_model_id,
            effective_model_label,
            provider_harness: harness.map(|value| value.to_string()),
            provider_session_id,
            service_tier: None,
        }
    }
}

/// Payload for agent:chunk event
#[derive(Debug, Clone, Serialize)]
pub struct AgentChunkPayload {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_index: Option<u64>,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    pub seq: u64,
    #[serde(default)]
    pub append_to_previous: bool,
}

/// Payload for agent:thinking event.
#[derive(Debug, Clone, Serialize)]
pub struct AgentThinkingPayload {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_index: Option<u64>,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    pub seq: u64,
    #[serde(default)]
    pub append_to_previous: bool,
}

/// Payload for agent:usage_updated event
#[derive(Debug, Clone, Serialize)]
pub struct AgentUsageUpdatedPayload {
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
}

/// Optional preview metadata for agent:tool_call payloads.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentToolCallPreviewFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview_original_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview_line_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview_omitted_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_preview_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_preview_original_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_preview_line_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_preview_omitted_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_preview: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_ref: Option<serde_json::Value>,
}

impl AgentToolCallPreviewFields {
    pub(crate) fn from_tool_result_preview(
        preview: Option<&super::tool_result_preview::ToolResultPreviewPayload>,
    ) -> Self {
        let Some(preview) = preview else {
            return Self::default();
        };
        Self {
            result_preview_truncated: Some(true),
            result_preview_original_bytes: Some(preview.original_bytes),
            result_preview_line_count: Some(preview.line_count),
            result_preview_omitted_lines: Some(preview.omitted_lines),
            result_preview_paths: (!preview.paths.is_empty()).then(|| preview.paths.clone()),
            detail_ref: preview.detail_ref.clone(),
            ..Self::default()
        }
    }

    pub(crate) fn apply_tool_argument_preview(&mut self, preview: &ToolArgumentPreviewPayload) {
        self.arguments_preview_truncated = Some(true);
        self.arguments_preview_original_bytes = Some(preview.original_bytes);
        self.arguments_preview_line_count = Some(preview.line_count);
        self.arguments_preview_omitted_lines = Some(preview.omitted_lines);
        self.diff_preview = preview.diff_preview.clone();
        if let Some(detail_ref) = preview.detail_ref.clone() {
            self.detail_ref = Some(detail_ref);
        }
    }
}

/// Payload for agent:tool_call event
#[derive(Debug, Clone, Serialize)]
pub struct AgentToolCallPayload {
    pub tool_name: String,
    pub tool_id: Option<String>,
    pub arguments: serde_json::Value,
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(flatten)]
    pub preview: AgentToolCallPreviewFields,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_context: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    pub seq: u64,
}

impl AgentToolCallPayload {
    pub(crate) fn from_live_tool_result(
        tool_use_id: &str,
        result_preview: &LiveToolResultPreview,
        conversation_id: &str,
        context_type: &str,
        context_id: &str,
        run_id: Option<&str>,
        parent_tool_use_id: Option<String>,
        seq: u64,
    ) -> Self {
        Self {
            tool_name: format!("result:{tool_use_id}"),
            tool_id: Some(tool_use_id.to_string()),
            arguments: serde_json::Value::Null,
            result: Some(result_preview.result.clone()),
            run_id: run_id.map(str::to_string),
            preview: AgentToolCallPreviewFields::from_tool_result_preview(
                result_preview.preview.as_ref(),
            ),
            conversation_id: conversation_id.to_string(),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
            diff_context: None,
            parent_tool_use_id,
            seq,
        }
    }

    pub(crate) fn from_completed_tool_call(
        tool_call: &ToolCall,
        result_preview: Option<&LiveToolResultPreview>,
        argument_preview: Option<&ToolArgumentPreviewPayload>,
        conversation_id: &str,
        context_type: &str,
        context_id: &str,
        run_id: Option<&str>,
        diff_context: Option<serde_json::Value>,
        parent_tool_use_id: Option<String>,
        seq: u64,
    ) -> Self {
        let result = result_preview
            .map(|preview| Some(preview.result.clone()))
            .unwrap_or_else(|| tool_call.result.clone());
        let mut preview = AgentToolCallPreviewFields::from_tool_result_preview(
            result_preview.and_then(|preview| preview.preview.as_ref()),
        );
        if let Some(argument_preview) = argument_preview {
            preview.apply_tool_argument_preview(argument_preview);
        }
        let arguments = argument_preview
            .map(|preview| preview.arguments.clone())
            .unwrap_or_else(|| tool_call.arguments.clone());
        let diff_context = argument_preview
            .and_then(|preview| preview.diff_context.clone())
            .or(diff_context);

        Self {
            tool_name: tool_call.name.clone(),
            tool_id: tool_call.id.clone(),
            arguments,
            result,
            run_id: run_id.map(str::to_string),
            preview,
            conversation_id: conversation_id.to_string(),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
            diff_context,
            parent_tool_use_id,
            seq,
        }
    }
}

/// Payload for agent:message_created event
#[derive(Debug, Clone, Serialize)]
pub struct AgentMessageCreatedPayload {
    pub message_id: String,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    pub role: String,
    pub content: String,
    /// Server-side DB timestamp for the message (RFC3339). Used by frontend to avoid clock skew.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Optional JSON metadata string attached to the message (e.g. recovery_context).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    /// Canonical render-ready payload for active transcript cache handoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_ready: Option<AgentMessageRenderReadyPayload>,
}

/// Render-ready chat payload attached to `agent:message_created` after the
/// backend has already persisted canonical timeline rows for the message.
#[derive(Debug, Clone, Serialize)]
pub struct AgentMessageRenderReadyPayload {
    pub message: AgentRenderReadyMessagePayload,
    pub timeline_items: Vec<AgentRenderReadyTimelineItemPayload>,
}

impl AgentMessageRenderReadyPayload {
    pub fn from_message_and_timeline_items(
        message: &ChatMessage,
        timeline_items: Vec<ChatTimelineItem>,
    ) -> Option<Self> {
        if timeline_items.is_empty() {
            return None;
        }

        Some(Self {
            message: AgentRenderReadyMessagePayload::from(message),
            timeline_items: timeline_items
                .into_iter()
                .map(AgentRenderReadyTimelineItemPayload::from)
                .collect(),
        })
    }
}

/// Message shape matching the normal chat API response fields used by frontend cache hydration.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRenderReadyMessagePayload {
    pub id: String,
    pub conversation_id: Option<String>,
    pub role: String,
    pub content: String,
    pub metadata: Option<String>,
    pub tool_calls: Option<Value>,
    pub content_blocks: Option<Value>,
    pub attribution_source: Option<String>,
    pub provider_harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<String>,
    pub effective_effort: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub estimated_usd: Option<f64>,
    pub created_at: String,
}

impl From<&ChatMessage> for AgentRenderReadyMessagePayload {
    fn from(message: &ChatMessage) -> Self {
        Self {
            id: message.id.as_str().to_string(),
            conversation_id: message
                .conversation_id
                .as_ref()
                .map(|id| id.as_str().to_string()),
            role: message.role.to_string(),
            content: message.content.clone(),
            metadata: message.metadata.clone(),
            tool_calls: parse_json_payload(message.tool_calls.as_deref()),
            content_blocks: parse_json_payload(message.content_blocks.as_deref()),
            attribution_source: message.attribution_source.clone(),
            provider_harness: message.provider_harness.map(|value| value.to_string()),
            provider_session_id: message.provider_session_id.clone(),
            upstream_provider: message.upstream_provider.clone(),
            provider_profile: message.provider_profile.clone(),
            logical_model: message.logical_model.clone(),
            effective_model_id: message.effective_model_id.clone(),
            logical_effort: message.logical_effort.map(|value| value.to_string()),
            effective_effort: message.effective_effort.clone(),
            input_tokens: message.input_tokens,
            output_tokens: message.output_tokens,
            cache_creation_tokens: message.cache_creation_tokens,
            cache_read_tokens: message.cache_read_tokens,
            estimated_usd: message.estimated_usd,
            created_at: message.created_at.to_rfc3339(),
        }
    }
}

/// Timeline item shape matching the normal timeline API response fields used by frontend cache hydration.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRenderReadyTimelineItemPayload {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub run_id: Option<String>,
    pub sequence: i64,
    pub block_index: i64,
    pub role: String,
    pub kind: String,
    pub status: String,
    pub content: String,
    pub content_blocks: Value,
    pub tool_call: Option<Value>,
    pub metadata: Option<String>,
    pub provider_harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finalized_at: Option<String>,
}

impl From<ChatTimelineItem> for AgentRenderReadyTimelineItemPayload {
    fn from(item: ChatTimelineItem) -> Self {
        let message_id = item.message_id.as_ref().map(|id| id.as_str().to_string());
        let conversation_id = item.conversation_id.as_str();
        let content = item.text.clone().unwrap_or_default();
        let content_block =
            render_ready_timeline_content_block(&item, &conversation_id, message_id.as_deref());
        let content_blocks = Value::Array(vec![content_block.clone()]);
        let tool_call = if item.kind.to_string() == "tool_use" {
            Some(content_block)
        } else {
            None
        };

        Self {
            id: item.id.to_string(),
            conversation_id,
            message_id,
            run_id: item.run_id.map(|id| id.as_str()),
            sequence: item.sequence,
            block_index: item.block_index,
            role: item.role.to_string(),
            kind: item.kind.to_string(),
            status: item.status.to_string(),
            content,
            content_blocks,
            tool_call,
            metadata: item.metadata,
            provider_harness: item.provider_harness.map(|value| value.to_string()),
            provider_session_id: item.provider_session_id,
            created_at: item.created_at.to_rfc3339(),
            updated_at: item.updated_at.to_rfc3339(),
            finalized_at: item.finalized_at.map(|value| value.to_rfc3339()),
        }
    }
}

fn parse_json_payload(raw: Option<&str>) -> Option<Value> {
    raw.and_then(|value| serde_json::from_str(value).ok())
}

fn render_ready_timeline_content_block(
    item: &ChatTimelineItem,
    conversation_id: &str,
    message_id: Option<&str>,
) -> Value {
    if item.kind.to_string() == "text" {
        return serde_json::json!({
            "type": "text",
            "text": item.text.clone().unwrap_or_default(),
        });
    }

    let arguments = item
        .input_json
        .as_deref()
        .or(item.tool_input_preview.as_deref())
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let result = item
        .result_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .or_else(|| item.tool_result_preview.clone().map(Value::String));
    let mut block = serde_json::json!({
        "type": "tool_use",
        "id": item.tool_call_id.clone().unwrap_or_else(|| item.id.to_string()),
        "name": item.tool_name.clone().unwrap_or_else(|| "unknown".to_string()),
        "arguments": arguments,
        "result": result,
        "detail_ref": {
            "conversation_id": conversation_id,
            "message_id": message_id.unwrap_or(item.id.as_str()),
            "tool_call_id": item.tool_call_id.clone(),
            "content_block_index": item.block_index,
            "timeline_item_id": item.id.to_string(),
        }
    });

    if let Some(raw) = item
        .raw_block_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
    {
        if let Some(diff_context) = raw.get("diff_context").cloned() {
            block["diff_context"] = diff_context;
        }
    }

    block
}

/// Payload for agent:run_completed event
#[derive(Debug, Clone, Serialize)]
pub struct AgentRunCompletedPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    pub claude_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_harness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_chain_id: Option<String>,
}

impl AgentRunCompletedPayload {
    pub fn with_provider_session(
        conversation_id: impl Into<String>,
        context_type: impl Into<String>,
        context_id: impl Into<String>,
        harness: Option<AgentHarnessKind>,
        provider_session_id: Option<String>,
        run_chain_id: Option<String>,
    ) -> Self {
        Self::with_provider_session_and_run_id(
            None,
            conversation_id,
            context_type,
            context_id,
            harness,
            provider_session_id,
            run_chain_id,
        )
    }

    pub fn with_provider_session_and_run_id(
        run_id: Option<String>,
        conversation_id: impl Into<String>,
        context_type: impl Into<String>,
        context_id: impl Into<String>,
        harness: Option<AgentHarnessKind>,
        provider_session_id: Option<String>,
        run_chain_id: Option<String>,
    ) -> Self {
        let (claude_session_id, provider_session_id, provider_harness) =
            compatible_provider_session_fields_from_provider_ref(harness, provider_session_id);

        Self {
            run_id,
            conversation_id: conversation_id.into(),
            context_type: context_type.into(),
            context_id: context_id.into(),
            claude_session_id,
            provider_harness: provider_harness.map(|value| value.to_string()),
            provider_session_id,
            run_chain_id,
        }
    }
}

/// Payload for agent:error event
#[derive(Debug, Clone, Serialize)]
pub struct AgentErrorPayload {
    pub conversation_id: Option<String>,
    pub context_type: String,
    pub context_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_run_id: Option<String>,
    pub error: String,
    pub stderr: Option<String>,
}

/// Payload for agent:task_started event
#[derive(Debug, Clone, Serialize)]
pub struct AgentTaskStartedPayload {
    pub tool_use_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Tool name that triggered this: "Task" or "Agent"
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teammate_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_agent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_harness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_provenance: Option<String>,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    pub seq: u64,
}

/// Payload for agent:task_completed event
#[derive(Debug, Clone, Serialize)]
pub struct AgentTaskCompletedPayload {
    pub tool_use_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tool_use_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teammate_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_agent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_harness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_provenance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    pub seq: u64,
}

/// Payload for agent:queue_sent event
#[derive(Debug, Clone, Serialize)]
pub struct AgentQueueSentPayload {
    pub message_id: String,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
}

/// Payload for agent:message_queued event (message entered the queue at Gate 2)
#[derive(Debug, Clone, Serialize)]
pub struct AgentMessageQueuedPayload {
    pub message_id: String,
    pub content: String,
    pub context_type: String,
    pub context_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
}

/// Payload for agent:conversation_created event
#[derive(Debug, Clone, Serialize)]
pub struct AgentConversationCreatedPayload {
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
}

/// Payload for agent:hook event (discriminated by `hook_type`)
#[derive(Debug, Clone, Serialize)]
pub struct AgentHookPayload {
    /// Discriminator: "started", "completed", or "block"
    #[serde(rename = "type")]
    pub hook_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    pub timestamp: i64,
}

// ============================================================================
// Team Event Payloads
// ============================================================================

/// Payload for team:artifact_created event
#[derive(Debug, Clone, Serialize)]
pub struct TeamArtifactCreatedPayload {
    pub artifact_id: String,
    pub session_id: String,
    pub artifact_type: String,
    pub title: String,
}

// ============================================================================
// Error type
// ============================================================================

/// Marker prefix for `ChatServiceError::MessageDeliveredNotPersisted`. The frontend
/// matches on it to keep the sent turn visible instead of reporting a failed send.
/// ❌ Don't reword without updating `frontend/src/lib/sendDeliveryErrors.ts`.
pub const MESSAGE_DELIVERED_NOT_PERSISTED_PREFIX: &str = "[Message delivered but not saved:";

#[derive(Debug, Clone)]
pub enum ChatServiceError {
    InvalidInput(String),
    AgentNotAvailable(String),
    SpawnFailed(String),
    /// The caller required a fresh runtime turn, but current conversation or launch capacity
    /// makes that impossible right now. This is intentionally distinct from a spawn failure:
    /// orchestrators can defer it without spending delivery retry budget.
    ImmediateStartRejected(String),
    SpawnValidation {
        harness: crate::domain::agents::AgentHarnessKind,
        model: String,
        reason: String,
    },
    CommunicationFailed(String),
    ParseError(String),
    ContextNotFound(String),
    ConversationNotFound(String),
    RepositoryError(String),
    AgentRunFailed(String),
    PersonaUnavailable(String),
    /// The live interactive process accepted this turn, but persisting it failed.
    /// The agent IS answering: the UI must not report the turn as never sent.
    MessageDeliveredNotPersisted(String),
}

impl std::fmt::Display for ChatServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            Self::AgentNotAvailable(msg) => write!(f, "Agent not available: {}", msg),
            Self::SpawnFailed(msg) => write!(f, "Failed to spawn agent: {}", msg),
            Self::ImmediateStartRejected(msg) => write!(f, "Immediate start rejected: {msg}"),
            Self::SpawnValidation {
                harness,
                model,
                reason,
            } => write!(
                f,
                "Invalid agent runtime (harness={harness}, model={model}): {reason}"
            ),
            Self::CommunicationFailed(msg) => write!(f, "Communication failed: {}", msg),
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::ContextNotFound(msg) => write!(f, "Context not found: {}", msg),
            Self::ConversationNotFound(msg) => write!(f, "Conversation not found: {}", msg),
            Self::RepositoryError(msg) => write!(f, "Repository error: {}", msg),
            Self::AgentRunFailed(msg) => write!(f, "Agent run failed: {}", msg),
            Self::PersonaUnavailable(message) => write!(f, "{message}"),
            Self::MessageDeliveredNotPersisted(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ChatServiceError {}

#[cfg(test)]
#[path = "chat_service_types_tests.rs"]
mod tests;
