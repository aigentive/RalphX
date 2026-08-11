// Stream processor type definitions
// All data types, enums, and structs used by the stream processor

use serde::{Deserialize, Serialize};

use crate::domain::entities::{AgentRunUsage, UsageProvenance};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeResultUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default, alias = "cache_creation_input_tokens")]
    pub cache_creation_tokens: Option<u64>,
    #[serde(
        default,
        alias = "cache_read_input_tokens",
        alias = "cached_input_tokens"
    )]
    pub cache_read_tokens: Option<u64>,
}

impl ClaudeResultUsage {
    pub fn into_agent_run_usage(self, estimated_usd: Option<f64>) -> AgentRunUsage {
        AgentRunUsage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_tokens: self.cache_creation_tokens,
            cache_read_tokens: self.cache_read_tokens,
            estimated_usd,
        }
    }
}

// ============================================================================
// Stream Message Types (from Claude CLI stream-json output)
// ============================================================================

/// Parsed stream-json message from Claude CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamMessage {
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: Option<i32>,
        content_block: ContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: Option<i32>,
        delta: ContentDelta,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: Option<i32> },
    #[serde(rename = "message_start")]
    MessageStart { message: Option<serde_json::Value> },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: Option<serde_json::Value>,
        usage: Option<serde_json::Value>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    /// Assistant message with full content (from --verbose mode)
    #[serde(rename = "assistant")]
    Assistant {
        message: AssistantMessage,
        session_id: Option<String>,
    },
    /// Result event containing session_id for --resume support
    #[serde(rename = "result")]
    Result {
        result: Option<String>,
        session_id: Option<String>,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        errors: Vec<String>,
        #[serde(default)]
        subtype: Option<String>,
        #[serde(default)]
        cost_usd: f64,
        #[serde(default)]
        usage: Option<ClaudeResultUsage>,
    },
    /// System event (e.g., init messages, hook events)
    #[serde(rename = "system")]
    System {
        message: Option<String>,
        session_id: Option<String>,
        #[serde(default)]
        subtype: Option<String>,
        #[serde(default)]
        hook_id: Option<String>,
        #[serde(default)]
        hook_name: Option<String>,
        #[serde(default)]
        hook_event: Option<String>,
        #[serde(default)]
        output: Option<String>,
        #[serde(default)]
        exit_code: Option<i32>,
        #[serde(default)]
        outcome: Option<String>,
        #[serde(default)]
        estimated_tokens: Option<u64>,
        #[serde(default)]
        estimated_tokens_delta: Option<u64>,
    },
    /// User message (contains tool results when using MCP)
    #[serde(rename = "user")]
    User { message: UserMessage },
    #[serde(other)]
    Other,
}

/// User message structure (contains tool results from MCP tool execution)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub content: Vec<UserContent>,
}

/// Content block in user message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UserContent {
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

/// Assistant message structure from Claude CLI verbose output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<AssistantContent>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<serde_json::Value>,
}

/// Content block in assistant message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AssistantContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub id: Option<String>,
    pub text: Option<String>,
    pub name: Option<String>,
    pub input: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDelta {
    #[serde(rename = "type")]
    pub delta_type: String,
    #[serde(alias = "thinking")]
    pub text: Option<String>,
    pub partial_json: Option<String>,
}

// ============================================================================
// Tool Call Type
// ============================================================================

/// Diff context captured at ToolCallCompleted for Edit/Write tool calls.
/// Stores old file content so frontend can compute proper diffs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffContext {
    /// Previous file content when it could be captured.
    pub old_content: Option<String>,
    /// Whether the file existed when the baseline was captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_file_exists: Option<bool>,
    /// Resolved file path for reference
    pub file_path: String,
}

/// Stats captured from a completed Task/Agent tool call.
/// Stored as a sibling field of `result` in `ToolCall` — serialized via all write paths.
/// Uses camelCase because the frontend TypeScript expects camelCase field names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallStats {
    pub model: Option<String>,
    pub total_tokens: Option<u64>,
    pub total_tool_uses: Option<u64>,
    pub duration_ms: Option<u64>,
}

/// Tool call extracted from the stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: Option<String>,
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_context: Option<DiffContext>,
    /// Stats for Task/Agent tool calls — populated at TaskCompleted time.
    /// Field is absent (not null) for old rows and non-Task tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<ToolCallStats>,
}

/// Content block item - preserves order of text and tool calls
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlockItem {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// Provider-reported reasoning-token total when a harness exposes it.
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_tokens: Option<u64>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: Option<String>,
        name: String,
        arguments: serde_json::Value,
        result: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_tool_use_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff_context: Option<serde_json::Value>,
    },
}

// ============================================================================
// Stream Events (what the processor emits)
// ============================================================================

/// Events emitted during stream processing
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text chunk received
    TextChunk(String),
    /// Thinking block from Claude's extended reasoning.
    /// `block_index` is authoritative, so consumers do not infer it from
    /// mutable processor state.
    Thinking { text: String, block_index: u64 },
    /// A thinking block was sealed with its final duration.
    ThinkingSettled {
        block_index: u64,
        duration_ms: Option<u64>,
    },
    /// Estimated tokens consumed while Claude is thinking.
    ThinkingProgress {
        estimated_tokens: u64,
        estimated_tokens_delta: Option<u64>,
    },
    /// Tool call started (name and id available)
    ToolCallStarted {
        name: String,
        id: Option<String>,
        parent_tool_use_id: Option<String>,
    },
    /// Tool call completed (arguments parsed)
    ToolCallCompleted {
        tool_call: ToolCall,
        parent_tool_use_id: Option<String>,
    },
    /// Tool result received (from user message with tool_result)
    ToolResultReceived {
        tool_use_id: String,
        result: serde_json::Value,
        is_error: bool,
        parent_tool_use_id: Option<String>,
    },
    /// Session ID received (from Result or Assistant message)
    SessionId(String),
    /// Task subagent started (detected from Task/Agent tool_use)
    TaskStarted {
        tool_use_id: String,
        /// Tool name that triggered this: "Task" or "Agent"
        tool_name: String,
        description: Option<String>,
        subagent_type: Option<String>,
        model: Option<String>,
    },
    /// Task subagent completed (detected from Task tool_result)
    TaskCompleted {
        tool_use_id: String,
        agent_id: Option<String>,
        total_duration_ms: Option<u64>,
        total_tokens: Option<u64>,
        total_tool_use_count: Option<u64>,
    },
    /// Hook started (from system message with subtype "hook_started")
    HookStarted {
        hook_id: String,
        hook_name: String,
        hook_event: String,
    },
    /// Hook completed (from system message with subtype "hook_response")
    HookCompleted {
        hook_id: String,
        hook_name: String,
        hook_event: String,
        output: Option<String>,
        exit_code: Option<i32>,
        outcome: Option<String>,
    },
    /// Hook block (from synthetic user message with text content)
    HookBlock { reason: String },
    /// Turn completed — the lead's result event signals the end of one
    /// agentic turn in interactive (multi-turn) mode. The CLI process stays
    /// alive for subsequent turns.
    TurnComplete { session_id: Option<String> },
}

// ============================================================================
// Parsed Line and Stream Result
// ============================================================================

/// Parsed line with optional parent_tool_use_id and is_synthetic extracted from top-level JSON
pub struct ParsedLine {
    pub message: StreamMessage,
    pub parent_tool_use_id: Option<String>,
    pub is_synthetic: bool,
    /// Top-level `tool_use_result` from Claude Code stream JSON.
    /// Contains structured metadata (e.g. `{"status": "teammate_spawned", ...}`)
    /// that is NOT inside the `message.content[].content` field.
    pub tool_use_result: Option<serde_json::Value>,
}

/// Final result from processing a stream
#[derive(Debug, Clone)]
pub struct StreamResult {
    pub response_text: String,
    pub tool_calls: Vec<ToolCall>,
    /// Content blocks in order (text and tool calls interleaved)
    pub content_blocks: Vec<ContentBlockItem>,
    pub session_id: Option<String>,
    pub usage: AgentRunUsage,
    pub usage_provenance: Option<UsageProvenance>,
    pub is_error: bool,
    pub errors: Vec<String>,
    pub error_subtype: Option<String>,
}
