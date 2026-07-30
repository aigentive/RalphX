// Claude CLI Stream Processor
// Shared stream message parsing and content extraction for all services
// that consume Claude CLI stream-json output.

mod parser;
mod types;

pub use types::{
    AssistantContent, AssistantMessage, ContentBlock, ContentBlockItem, ContentDelta, DiffContext,
    ParsedLine, StreamEvent, StreamMessage, StreamResult, ToolCall, ToolCallStats, UserContent,
};

use crate::domain::entities::{AgentRunUsage, UsageCapture, UsageProvenance};

// Re-export types and parser helpers only used by tests (via `use super::*`)
#[cfg(test)]
pub(crate) use parser::{parse_usage_text, value_to_text};
#[cfg(test)]
pub(crate) use types::UserMessage;

/// Accumulator for processing Claude CLI stream output
///
/// This struct handles the stateful parsing of stream-json output,
/// tracking partial tool calls and accumulated text.
#[derive(Debug, Default)]
pub struct StreamProcessor {
    /// Accumulated response text (legacy, for backward compat)
    pub response_text: String,
    /// Completed tool calls (legacy, for backward compat)
    pub tool_calls: Vec<ToolCall>,
    /// Content blocks in order (text and tool calls interleaved)
    pub content_blocks: Vec<ContentBlockItem>,
    /// Claude session ID for --resume
    pub session_id: Option<String>,
    /// Indicates the result message reported an error
    pub result_is_error: bool,
    /// Errors from the result message (if any)
    pub result_errors: Vec<String>,
    /// Optional error subtype from the result message
    pub result_subtype: Option<String>,
    /// Usage accumulated across already-finalized turns in this run.
    finalized_usage: AgentRunUsage,
    finalized_usage_provenance: Option<UsageProvenance>,
    /// Usage currently accumulating for the in-flight turn.
    current_turn_usage: AgentRunUsage,
    current_turn_usage_provenance: Option<UsageProvenance>,

    // Internal state for partial tool calls
    current_tool_name: String,
    current_tool_id: Option<String>,
    current_tool_input: String,
    // Track current text accumulation for content blocks
    current_text_block: String,
    // Track if we're in a thinking block (streaming mode)
    in_thinking_block: bool,
    // Accumulated thinking text during streaming
    current_thinking_block: String,
    // Track if any ContentBlockDelta text events were received (for dedup guard)
    had_streaming_text_deltas: bool,
}

impl StreamProcessor {
    /// Create a new stream processor
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a stream message without parent context (backward compat)
    pub fn process_message(&mut self, msg: StreamMessage) -> Vec<StreamEvent> {
        self.process_message_with_parent(msg, None, false, None)
    }

    /// Process a parsed line (message + parent_tool_use_id + is_synthetic + tool_use_result)
    pub fn process_parsed_line(&mut self, parsed: ParsedLine) -> Vec<StreamEvent> {
        self.process_message_with_parent(
            parsed.message,
            parsed.parent_tool_use_id,
            parsed.is_synthetic,
            parsed.tool_use_result,
        )
    }

    /// Parse a stream-json line, extracting parent_tool_use_id from the top-level JSON envelope
    pub fn parse_line(line: &str) -> Option<ParsedLine> {
        parser::parse_line(line)
    }

    /// Detect team-related events from tool result JSON.
    /// Process a stream message with optional parent_tool_use_id, is_synthetic, and tool_use_result context.
    /// `tool_use_result` is the top-level structured metadata from Claude Code stream JSON
    /// (e.g. `{"status": "teammate_spawned", ...}`) — distinct from `message.content[].content`.
    fn process_message_with_parent(
        &mut self,
        msg: StreamMessage,
        parent_tool_use_id: Option<String>,
        is_synthetic: bool,
        tool_use_result: Option<serde_json::Value>,
    ) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        match msg {
            StreamMessage::MessageDelta { usage, .. } => {
                if parent_tool_use_id.is_none() {
                    if let Some(usage) = usage.as_ref() {
                        update_turn_usage_from_value(&mut self.current_turn_usage, usage);
                        if !self.current_turn_usage.is_empty() {
                            self.current_turn_usage_provenance =
                                Some(UsageProvenance::ProviderSnapshotFallback);
                        }
                    }
                }
            }
            StreamMessage::ContentBlockStart { content_block, .. } => {
                if content_block.block_type == "tool_use" {
                    // Flush any accumulated text before starting tool call
                    if !self.current_text_block.is_empty() {
                        self.content_blocks.push(ContentBlockItem::Text {
                            text: self.current_text_block.clone(),
                        });
                        self.current_text_block.clear();
                    }

                    self.current_tool_name = content_block.name.unwrap_or_default();
                    self.current_tool_id = content_block.id;
                    self.current_tool_input.clear();

                    events.push(StreamEvent::ToolCallStarted {
                        name: self.current_tool_name.clone(),
                        id: self.current_tool_id.clone(),
                        parent_tool_use_id: parent_tool_use_id.clone(),
                    });
                } else if content_block.block_type == "thinking" {
                    // Start of a thinking block - mark state for thinking delta handling
                    self.in_thinking_block = true;
                    self.current_thinking_block.clear();
                }
            }
            StreamMessage::ContentBlockDelta { delta, .. } => {
                if delta.delta_type == "text_delta" {
                    if let Some(text) = delta.text {
                        self.had_streaming_text_deltas = true;
                        self.response_text.push_str(&text);
                        self.current_text_block.push_str(&text);
                        events.push(StreamEvent::TextChunk(text));
                    }
                } else if delta.delta_type == "thinking_delta" {
                    // Thinking block delta - accumulate and emit
                    if let Some(text) = delta.text {
                        self.current_thinking_block.push_str(&text);
                        events.push(StreamEvent::Thinking(text));
                    }
                } else if delta.delta_type == "input_json_delta" {
                    if let Some(json) = delta.partial_json {
                        self.current_tool_input.push_str(&json);
                    }
                }
            }
            StreamMessage::ContentBlockStop { .. } => {
                if !self.current_tool_name.is_empty() {
                    // Parse tool arguments
                    let args: serde_json::Value =
                        serde_json::from_str(&self.current_tool_input).unwrap_or_default();

                    // Detect Task/Agent tool_use and emit TaskStarted (streaming mode)
                    if self.current_tool_name == "Task" || self.current_tool_name == "Agent" {
                        if let Some(ref tool_id) = self.current_tool_id {
                            events.push(StreamEvent::TaskStarted {
                                tool_use_id: tool_id.clone(),
                                tool_name: self.current_tool_name.clone(),
                                description: args
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                subagent_type: args
                                    .get("subagent_type")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                model: args
                                    .get("model")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                            });
                        }
                    }

                    let tool_call = ToolCall {
                        id: self.current_tool_id.clone(),
                        name: self.current_tool_name.clone(),
                        arguments: args.clone(),
                        result: None,
                        parent_tool_use_id: parent_tool_use_id.clone(),
                        diff_context: None,
                        stats: None,
                    };

                    self.tool_calls.push(tool_call.clone());

                    // Add to content blocks in order
                    self.content_blocks.push(ContentBlockItem::ToolUse {
                        id: self.current_tool_id.clone(),
                        name: self.current_tool_name.clone(),
                        arguments: args,
                        result: None,
                        parent_tool_use_id: parent_tool_use_id.clone(),
                        diff_context: None,
                    });

                    events.push(StreamEvent::ToolCallCompleted {
                        tool_call,
                        parent_tool_use_id: parent_tool_use_id.clone(),
                    });

                    // Reset tool state
                    self.current_tool_name.clear();
                    self.current_tool_id = None;
                    self.current_tool_input.clear();
                } else if self.in_thinking_block {
                    // End of thinking block - reset state
                    // (thinking content was already emitted as chunks)
                    self.in_thinking_block = false;
                    self.current_thinking_block.clear();
                } else if !self.current_text_block.is_empty() {
                    // Seal the delta-streamed text block here rather than waiting
                    // for finish(). With --include-partial-messages the verbose
                    // assistant summary is suppressed by the dedup guard below,
                    // so this is the only path that puts streamed text into
                    // content_blocks — and TurnComplete persists from there.
                    self.content_blocks.push(ContentBlockItem::Text {
                        text: std::mem::take(&mut self.current_text_block),
                    });
                }
            }
            StreamMessage::Result {
                session_id,
                is_error,
                errors,
                subtype,
                cost_usd,
                usage,
                ..
            } => {
                // Only capture session_id from top-level (lead's own) result events.
                // Teammate result events carry a non-None parent_tool_use_id; capturing
                // their session_id would overwrite the lead's session and cause --resume
                // to open the teammate's context instead of the orchestrator's.
                if parent_tool_use_id.is_none() {
                    let estimated_usd = (cost_usd > 0.0).then_some(cost_usd);
                    if let Some(usage) = usage {
                        let terminal_usage = usage.into_agent_run_usage(estimated_usd);
                        if !terminal_usage.is_empty() {
                            self.current_turn_usage = terminal_usage;
                            self.current_turn_usage_provenance =
                                Some(UsageProvenance::ProviderTurnDelta);
                        } else if let Some(cost_usd) = estimated_usd {
                            self.current_turn_usage.estimated_usd = Some(cost_usd);
                        }
                    } else if let Some(cost_usd) = estimated_usd {
                        self.current_turn_usage.estimated_usd = Some(cost_usd);
                        if self.current_turn_usage_provenance.is_none() {
                            self.current_turn_usage_provenance =
                                Some(UsageProvenance::ProviderSnapshotFallback);
                        }
                    }
                    if let Some(ref id) = session_id {
                        self.session_id = session_id.clone();
                        events.push(StreamEvent::SessionId(id.clone()));
                    }

                    // Emit TurnComplete so the stream handler can finalize the
                    // assistant message and emit run_completed. In interactive mode
                    // (multi-turn), this signals the end of one turn while the CLI
                    // process stays alive for more. In single-turn mode, EOF follows
                    // immediately after.
                    events.push(StreamEvent::TurnComplete {
                        session_id: self.session_id.clone(),
                    });
                }
                if is_error {
                    self.result_is_error = true;
                    if !errors.is_empty() {
                        self.result_errors = errors;
                    }
                    if subtype.is_some() {
                        self.result_subtype = subtype;
                    }
                }
            }
            StreamMessage::Assistant {
                message,
                session_id,
            } => {
                if parent_tool_use_id.is_none() {
                    if let Some(usage) = message.usage.as_ref() {
                        update_turn_usage_from_value(&mut self.current_turn_usage, usage);
                        if !self.current_turn_usage.is_empty() {
                            self.current_turn_usage_provenance =
                                Some(UsageProvenance::ProviderSnapshotFallback);
                        }
                    }
                }
                // Handle --verbose mode assistant messages (full content in one message)
                for content in message.content {
                    match content {
                        AssistantContent::Text { text } => {
                            if self.response_text.is_empty() || !self.had_streaming_text_deltas {
                                // Either first message OR a new API call in stream-json mode (no deltas).
                                // In stream-json mode, multi-API-call turns produce multiple Assistant
                                // messages. Without deltas, the dedup guard must allow all of them.
                                self.response_text.push_str(&text);
                                self.content_blocks
                                    .push(ContentBlockItem::Text { text: text.clone() });
                                events.push(StreamEvent::TextChunk(text));
                            }
                            // else: content_block_delta events already streamed the text,
                            // populated response_text, and emitted TextChunk events.
                            // Skip re-emission to prevent duplicate text and double TextChunk events.
                        }
                        AssistantContent::ToolUse { id, name, input } => {
                            // Detect Task/Agent tool_use and emit TaskStarted
                            if name == "Task" || name == "Agent" {
                                events.push(StreamEvent::TaskStarted {
                                    tool_use_id: id.clone(),
                                    tool_name: name.clone(),
                                    description: input
                                        .get("description")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    subagent_type: input
                                        .get("subagent_type")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    model: input
                                        .get("model")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                });
                            }

                            let tool_call = ToolCall {
                                id: Some(id.clone()),
                                name: name.clone(),
                                arguments: input.clone(),
                                result: None,
                                parent_tool_use_id: parent_tool_use_id.clone(),
                                diff_context: None,
                                stats: None,
                            };

                            self.tool_calls.push(tool_call.clone());
                            // Add as content block
                            self.content_blocks.push(ContentBlockItem::ToolUse {
                                id: Some(id),
                                name,
                                arguments: input,
                                result: None,
                                parent_tool_use_id: parent_tool_use_id.clone(),
                                diff_context: None,
                            });
                            events.push(StreamEvent::ToolCallCompleted {
                                tool_call,
                                parent_tool_use_id: parent_tool_use_id.clone(),
                            });
                        }
                        AssistantContent::Thinking { thinking } => {
                            // Emit complete thinking block from verbose mode
                            events.push(StreamEvent::Thinking(thinking));
                        }
                        AssistantContent::Other => {}
                    }
                }

                // Only capture session_id from top-level (lead's own) assistant messages.
                // Teammate assistant messages carry a non-None parent_tool_use_id; capturing
                // their session_id would overwrite the lead's session.
                if parent_tool_use_id.is_none() {
                    if let Some(ref id) = session_id {
                        self.session_id = session_id.clone();
                        events.push(StreamEvent::SessionId(id.clone()));
                    }
                }
            }
            StreamMessage::System {
                session_id,
                subtype,
                hook_id,
                hook_name,
                hook_event,
                output,
                exit_code,
                outcome,
                ..
            } => {
                if let Some(ref id) = session_id {
                    self.session_id = session_id.clone();
                    events.push(StreamEvent::SessionId(id.clone()));
                }

                match subtype.as_deref() {
                    Some("hook_started") => {
                        if let (Some(hid), Some(hname), Some(hevent)) =
                            (hook_id, hook_name, hook_event)
                        {
                            events.push(StreamEvent::HookStarted {
                                hook_id: hid,
                                hook_name: hname,
                                hook_event: hevent,
                            });
                        }
                    }
                    Some("hook_response") => {
                        if let (Some(hid), Some(hname), Some(hevent)) =
                            (hook_id, hook_name, hook_event)
                        {
                            events.push(StreamEvent::HookCompleted {
                                hook_id: hid,
                                hook_name: hname,
                                hook_event: hevent,
                                output,
                                exit_code,
                                outcome,
                            });
                        }
                    }
                    _ => {}
                }
            }
            StreamMessage::User { message } => {
                // Handle tool results and synthetic hook blocks
                for content in message.content {
                    // Synthetic user text messages are hook block notifications
                    if is_synthetic {
                        if let UserContent::Text { text } = &content {
                            events.push(StreamEvent::HookBlock {
                                reason: text.clone(),
                            });
                            continue;
                        }
                    }

                    if let UserContent::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = content
                    {
                        // Check if this is a Task tool_result by finding the matching tool_call
                        let is_task_result = self.tool_calls.iter().any(|tc| {
                            tc.id.as_ref() == Some(&tool_use_id)
                                && (tc.name == "Task" || tc.name == "Agent")
                        });

                        // Find the tool call by ID and update its result
                        if let Some(tool_call) = self
                            .tool_calls
                            .iter_mut()
                            .find(|tc| tc.id.as_ref() == Some(&tool_use_id))
                        {
                            tool_call.result = Some(content.clone());
                        }

                        // Also update the content_blocks array
                        for block in self.content_blocks.iter_mut() {
                            if let ContentBlockItem::ToolUse { id, result, .. } = block {
                                if id.as_ref() == Some(&tool_use_id) {
                                    *result = Some(content.clone());
                                    break;
                                }
                            }
                        }

                        // Emit TaskCompleted if this is a Task tool_result
                        if is_task_result {
                            // Use top-level tool_use_result (structured metadata from Claude Code)
                            // falling back to content.tool_use_result or content itself
                            let empty = serde_json::Value::Null;
                            let metadata = tool_use_result.as_ref().unwrap_or_else(|| {
                                content.get("tool_use_result").unwrap_or(&empty)
                            });
                            let json_agent_id = metadata
                                .get("agentId")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            let json_duration =
                                metadata.get("totalDurationMs").and_then(|v| v.as_u64());
                            let json_tokens = metadata.get("totalTokens").and_then(|v| v.as_u64());
                            let json_tools =
                                metadata.get("totalToolUseCount").and_then(|v| v.as_u64());

                            let has_json_stats = json_duration.is_some()
                                || json_tokens.is_some()
                                || json_tools.is_some();

                            // Fall back to text-based <usage> tag parsing if JSON extraction found nothing
                            let (agent_id, total_duration_ms, total_tokens, total_tool_use_count) =
                                if has_json_stats {
                                    (json_agent_id, json_duration, json_tokens, json_tools)
                                } else {
                                    let text = parser::value_to_text(&content);
                                    let (text_agent, text_dur, text_tok, text_tools) =
                                        parser::parse_usage_text(&text);
                                    (json_agent_id.or(text_agent), text_dur, text_tok, text_tools)
                                };

                            // Inject stats into the matching ToolCall so they are
                            // persisted via all write paths (flush, TurnComplete, finalization).
                            if let Some(tool_call) = self
                                .tool_calls
                                .iter_mut()
                                .find(|tc| tc.id.as_ref() == Some(&tool_use_id))
                            {
                                tool_call.stats = Some(ToolCallStats {
                                    model: None,
                                    total_tokens,
                                    total_tool_uses: total_tool_use_count,
                                    duration_ms: total_duration_ms,
                                });
                            }

                            events.push(StreamEvent::TaskCompleted {
                                tool_use_id: tool_use_id.clone(),
                                agent_id,
                                total_duration_ms,
                                total_tokens,
                                total_tool_use_count,
                            });
                        }

                        // Emit event with updated tool call
                        events.push(StreamEvent::ToolResultReceived {
                            tool_use_id: tool_use_id.clone(),
                            result: content.clone(),
                            is_error,
                            parent_tool_use_id: parent_tool_use_id.clone(),
                        });

                        // Check if this is a team event result.
                        // Use top-level tool_use_result (structured JSON from Claude Code stream)
                        // which contains the actual team metadata. The content field is just text.
                    }
                }
            }
            _ => {}
        }

        events
    }

    /// Reset accumulated state for the next interactive turn.
    /// Preserves `session_id` (stable across turns) but clears all content,
    /// tool calls, and error state so the next turn starts fresh.
    pub fn reset_for_next_turn(&mut self) {
        // Flush any pending text block
        if !self.current_text_block.is_empty() {
            self.content_blocks.push(ContentBlockItem::Text {
                text: self.current_text_block.clone(),
            });
            self.current_text_block.clear();
        }
        self.response_text.clear();
        self.tool_calls.clear();
        self.content_blocks.clear();
        self.current_tool_name.clear();
        self.current_tool_id = None;
        self.current_tool_input.clear();
        self.in_thinking_block = false;
        self.current_thinking_block.clear();
        self.had_streaming_text_deltas = false;
        self.result_is_error = false;
        self.result_errors.clear();
        self.result_subtype = None;
        accumulate_usage(&mut self.finalized_usage, &self.current_turn_usage);
        self.finalized_usage_provenance = combine_usage_provenance(
            self.finalized_usage_provenance,
            self.current_turn_usage_provenance,
        );
        self.current_turn_usage = AgentRunUsage::default();
        self.current_turn_usage_provenance = None;
    }

    pub fn current_turn_usage(&self) -> AgentRunUsage {
        self.current_turn_usage.clone()
    }

    pub fn current_turn_capture(&self) -> Option<UsageCapture> {
        if self.current_turn_usage.is_empty() {
            return None;
        }
        Some(UsageCapture::normalized(
            self.current_turn_usage.clone(),
            self.current_turn_usage_provenance
                .unwrap_or(UsageProvenance::ProviderSnapshotFallback),
        ))
    }

    /// Get the final result after stream is complete
    pub fn finish(mut self) -> StreamResult {
        // Flush any remaining text as a content block
        if !self.current_text_block.is_empty() {
            self.content_blocks.push(ContentBlockItem::Text {
                text: self.current_text_block,
            });
        }

        StreamResult {
            response_text: self.response_text,
            tool_calls: self.tool_calls,
            content_blocks: self.content_blocks,
            session_id: self.session_id,
            usage: combined_usage(&self.finalized_usage, &self.current_turn_usage),
            usage_provenance: combine_usage_provenance(
                self.finalized_usage_provenance,
                self.current_turn_usage_provenance,
            ),
            is_error: self.result_is_error,
            errors: self.result_errors,
            error_subtype: self.result_subtype,
        }
    }
}

fn combine_usage_provenance(
    finalized: Option<UsageProvenance>,
    current: Option<UsageProvenance>,
) -> Option<UsageProvenance> {
    match (finalized, current) {
        (None, next) | (next, None) => next,
        (Some(UsageProvenance::ProviderSnapshotFallback), _)
        | (_, Some(UsageProvenance::ProviderSnapshotFallback)) => {
            Some(UsageProvenance::ProviderSnapshotFallback)
        }
        _ => Some(UsageProvenance::ProviderTurnDelta),
    }
}

fn usage_value_as_u64(usage: &serde_json::Value, key: &str) -> Option<u64> {
    usage.get(key).and_then(|value| value.as_u64())
}

fn update_turn_usage_from_value(target: &mut AgentRunUsage, usage: &serde_json::Value) {
    update_max_usage_field(
        &mut target.input_tokens,
        usage_value_as_u64(usage, "input_tokens"),
    );
    update_max_usage_field(
        &mut target.output_tokens,
        usage_value_as_u64(usage, "output_tokens"),
    );
    update_max_usage_field(
        &mut target.cache_creation_tokens,
        usage_value_as_u64(usage, "cache_creation_input_tokens")
            .or_else(|| usage_value_as_u64(usage, "cache_creation_tokens")),
    );
    update_max_usage_field(
        &mut target.cache_read_tokens,
        usage_value_as_u64(usage, "cache_read_input_tokens")
            .or_else(|| usage_value_as_u64(usage, "cache_read_tokens"))
            .or_else(|| usage_value_as_u64(usage, "cached_input_tokens")),
    );
}

fn update_max_usage_field(target: &mut Option<u64>, next: Option<u64>) {
    if let Some(value) = next {
        if value > target.unwrap_or(0) {
            *target = Some(value);
        }
    }
}

fn add_optional_u64(lhs: Option<u64>, rhs: Option<u64>) -> Option<u64> {
    match (lhs, rhs) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn add_optional_f64(lhs: Option<f64>, rhs: Option<f64>) -> Option<f64> {
    match (lhs, rhs) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn accumulate_usage(total: &mut AgentRunUsage, turn: &AgentRunUsage) {
    total.input_tokens = add_optional_u64(total.input_tokens, turn.input_tokens);
    total.output_tokens = add_optional_u64(total.output_tokens, turn.output_tokens);
    total.cache_creation_tokens =
        add_optional_u64(total.cache_creation_tokens, turn.cache_creation_tokens);
    total.cache_read_tokens = add_optional_u64(total.cache_read_tokens, turn.cache_read_tokens);
    total.estimated_usd = add_optional_f64(total.estimated_usd, turn.estimated_usd);
}

fn combined_usage(finalized: &AgentRunUsage, current_turn: &AgentRunUsage) -> AgentRunUsage {
    AgentRunUsage {
        input_tokens: add_optional_u64(finalized.input_tokens, current_turn.input_tokens),
        output_tokens: add_optional_u64(finalized.output_tokens, current_turn.output_tokens),
        cache_creation_tokens: add_optional_u64(
            finalized.cache_creation_tokens,
            current_turn.cache_creation_tokens,
        ),
        cache_read_tokens: add_optional_u64(
            finalized.cache_read_tokens,
            current_turn.cache_read_tokens,
        ),
        estimated_usd: add_optional_f64(finalized.estimated_usd, current_turn.estimated_usd),
    }
}

#[cfg(test)]
mod tests;
