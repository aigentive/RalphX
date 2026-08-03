use serde::{Deserialize, Serialize};

use crate::infrastructure::agents::claude::ToolCall;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexUsage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
}

impl CodexUsage {
    fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.cached_input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.reasoning_output_tokens.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexUsagePayload {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_output_tokens: Option<u64>,
    #[serde(default)]
    pub total_token_usage: Option<CodexUsage>,
    #[serde(default)]
    pub last_token_usage: Option<CodexUsage>,
}

impl CodexUsagePayload {
    fn direct_usage(&self) -> CodexUsage {
        CodexUsage {
            input_tokens: self.input_tokens,
            cached_input_tokens: self.cached_input_tokens,
            output_tokens: self.output_tokens,
            reasoning_output_tokens: self.reasoning_output_tokens,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexUsageSource {
    TurnDelta,
    CumulativeTotal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexUsageSnapshot {
    pub usage: CodexUsage,
    pub source: CodexUsageSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexItemError {
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexErrorSource {
    Runtime,
    McpTool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexErrorMessage {
    pub message: String,
    pub source: CodexErrorSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexFileChange {
    pub path: String,
    pub kind: String,
}

/// Summary part of a rollout `response_item` reasoning payload.
///
/// Not part of the `codex exec --json` item schema — see `fixtures/README.md`. Retained because
/// that shape is what the persisted session rollout carries, so it stays a tolerated fallback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexReasoningSummary {
    #[serde(rename = "type")]
    pub summary_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexItem {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub summary: Option<Vec<CodexReasoningSummary>>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub arguments: Option<serde_json::Value>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<CodexItemError>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub aggregated_output: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub changes: Option<Vec<CodexFileChange>>,
    #[serde(default)]
    pub sender_thread_id: Option<String>,
    #[serde(default)]
    pub receiver_thread_ids: Option<Vec<String>>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub agents_states: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub item: Option<CodexItem>,
    #[serde(default)]
    pub usage: Option<CodexUsagePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexCommandExecution {
    pub id: Option<String>,
    pub command: Option<String>,
    pub status: Option<String>,
    pub aggregated_output: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexFileChangeSnapshot {
    pub id: Option<String>,
    pub phase: CodexToolCallPhase,
    pub status: Option<String>,
    pub changes: Vec<CodexFileChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexToolCallPhase {
    Started,
    Completed,
}

#[derive(Debug, Clone)]
pub struct CodexToolCallSnapshot {
    pub phase: CodexToolCallPhase,
    pub tool_call: ToolCall,
}

pub fn parse_codex_event_line(line: &str) -> Option<CodexStreamEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(error) => {
            tracing::debug!(%error, line = %trimmed, "Ignoring malformed Codex event line");
            return None;
        }
    };
    if let Some(normalized) = normalize_codex_event_msg(&value) {
        return Some(normalized);
    }

    let event: CodexStreamEvent = match serde_json::from_value(value) {
        Ok(event) => event,
        Err(error) => {
            tracing::debug!(%error, line = %trimmed, "Ignoring unparseable Codex event line");
            return None;
        }
    };
    if !matches!(
        event.event_type.as_str(),
        "thread.started"
            | "turn.started"
            | "turn.completed"
            | "turn.failed"
            | "item.started"
            | "item.updated"
            | "item.completed"
    ) {
        tracing::debug!(event_type = %event.event_type, "Ignoring unrecognized Codex event type");
    }
    Some(event)
}

/// Normalizes the `event_msg` envelope into the `item.*` shape the rest of this module speaks.
///
/// `codex exec --json` 0.146.0 does not emit `event_msg` at all (verified capture:
/// `fixtures/exec_json_reasoning_0_146_0.jsonl`); it is the persisted-rollout serialization, where
/// the envelope key is `payload`. `msg` is kept for older CLIs that used that key on stdout.
/// `agent_reasoning_delta` is deliberately absent: that tag does not exist in 0.146.0 — the real
/// internal delta tags are `reasoning_content_delta` / `reasoning_raw_content_delta`, and neither
/// reaches this transport.
fn normalize_codex_event_msg(value: &serde_json::Value) -> Option<CodexStreamEvent> {
    if value.get("type")?.as_str()? != "event_msg" {
        return None;
    }

    let envelope = value.get("msg").or_else(|| value.get("payload"))?;
    let item_type = envelope.get("type")?.as_str()?;
    if !matches!(item_type, "agent_message" | "agent_reasoning") {
        return None;
    }

    let text = envelope
        .get(if item_type == "agent_message" {
            "message"
        } else {
            "text"
        })
        .or_else(|| envelope.get("text"))
        .and_then(|value| value.as_str())?
        .to_string();
    let id = envelope
        .get("id")
        .or_else(|| value.get("id"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let thread_id = envelope
        .get("thread_id")
        .or_else(|| value.get("thread_id"))
        .and_then(|value| value.as_str())
        .map(str::to_string);

    Some(CodexStreamEvent {
        event_type: "item.completed".to_string(),
        thread_id,
        item: Some(CodexItem {
            id,
            item_type: item_type.to_string(),
            text: Some(text),
            summary: None,
            server: None,
            tool: None,
            arguments: None,
            result: None,
            error: None,
            status: None,
            aggregated_output: None,
            exit_code: None,
            command: None,
            changes: None,
            sender_thread_id: None,
            receiver_thread_ids: None,
            prompt: None,
            agents_states: None,
        }),
        usage: None,
    })
}

pub fn extract_codex_agent_message(event: &CodexStreamEvent) -> Option<String> {
    if event.event_type != "item.completed" {
        return None;
    }

    let item = event.item.as_ref()?;
    if item.item_type != "agent_message" {
        return None;
    }

    item.text.clone()
}

/// Extracts Codex reasoning text.
///
/// The live production shape is `item.completed` + `item.type == "reasoning"` + flat `text`, where
/// `text` holds the summary parts joined by `\n` (verified capture:
/// `fixtures/exec_json_reasoning_0_146_0.jsonl`). `item.started` / `item.updated` are accepted for
/// the same item type because the 0.146.0 exec schema declares them, even though reasoning was only
/// observed as `item.completed`. `agent_reasoning` covers the rollout envelope, and the `summary`
/// array is the rollout `response_item` fallback — see `fixtures/README.md`.
pub fn extract_codex_reasoning(event: &CodexStreamEvent) -> Option<String> {
    let item = event.item.as_ref()?;
    let is_reasoning = item.item_type == "agent_reasoning" && event.event_type == "item.completed"
        || item.item_type == "reasoning" && event.event_type.starts_with("item.");
    if !is_reasoning {
        return None;
    }

    item.text
        .clone()
        .filter(|text| !text.is_empty())
        .or_else(|| {
            item.summary.as_ref().and_then(|summary| {
                let text = summary
                    .iter()
                    .filter_map(|entry| entry.text.as_deref())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.is_empty()).then_some(text)
            })
        })
}

pub fn extract_codex_thread_id(event: &CodexStreamEvent) -> Option<String> {
    if event.event_type != "thread.started" {
        return None;
    }

    event.thread_id.clone()
}

pub fn extract_codex_tool_call_snapshot(event: &CodexStreamEvent) -> Option<CodexToolCallSnapshot> {
    let item = event.item.as_ref()?;

    let phase = match event.event_type.as_str() {
        "item.started" => CodexToolCallPhase::Started,
        "item.completed" => CodexToolCallPhase::Completed,
        _ => return None,
    };

    let tool_call = match item.item_type.as_str() {
        "mcp_tool_call" => {
            let name = match (item.server.as_deref(), item.tool.as_deref()) {
                (Some(server), Some(tool)) => format!("{server}::{tool}"),
                (None, Some(tool)) => tool.to_string(),
                _ => return None,
            };

            ToolCall {
                id: item.id.clone(),
                name,
                arguments: item.arguments.clone().unwrap_or_default(),
                result: item.result.clone(),
                parent_tool_use_id: None,
                diff_context: None,
                stats: None,
            }
        }
        "command_execution" => {
            let arguments = item
                .command
                .as_ref()
                .map(|command| serde_json::json!({ "command": command }))
                .unwrap_or_else(|| serde_json::json!({}));
            let result = match phase {
                CodexToolCallPhase::Started => None,
                CodexToolCallPhase::Completed => Some(serde_json::json!({
                    "text": item.aggregated_output.clone().unwrap_or_default(),
                    "exit_code": item.exit_code,
                    "status": item.status.clone(),
                })),
            };

            ToolCall {
                id: item.id.clone(),
                name: "bash".to_string(),
                arguments,
                result,
                parent_tool_use_id: None,
                diff_context: None,
                stats: None,
            }
        }
        _ => return None,
    };

    Some(CodexToolCallSnapshot { phase, tool_call })
}

pub fn extract_codex_command_execution(event: &CodexStreamEvent) -> Option<CodexCommandExecution> {
    let item = event.item.as_ref()?;
    if item.item_type != "command_execution" {
        return None;
    }

    Some(CodexCommandExecution {
        id: item.id.clone(),
        command: item.command.clone(),
        status: item.status.clone(),
        aggregated_output: item.aggregated_output.clone(),
        exit_code: item.exit_code,
    })
}

pub fn extract_codex_file_change_snapshot(
    event: &CodexStreamEvent,
) -> Option<CodexFileChangeSnapshot> {
    let item = event.item.as_ref()?;
    if item.item_type != "file_change" {
        return None;
    }

    let phase = match event.event_type.as_str() {
        "item.started" => CodexToolCallPhase::Started,
        "item.completed" => CodexToolCallPhase::Completed,
        _ => return None,
    };

    Some(CodexFileChangeSnapshot {
        id: item.id.clone(),
        phase,
        status: item.status.clone(),
        changes: item.changes.clone().unwrap_or_default(),
    })
}

pub fn extract_codex_error(event: &CodexStreamEvent) -> Option<CodexErrorMessage> {
    let item = event.item.as_ref()?;

    let (source, message) = match item.item_type.as_str() {
        "error" => (
            CodexErrorSource::Runtime,
            item.error
                .as_ref()
                .and_then(|error| error.message.clone())
                .or_else(|| item.text.clone())?,
        ),
        "mcp_tool_call" => (
            CodexErrorSource::McpTool,
            item.error
                .as_ref()
                .and_then(|error| error.message.clone())?,
        ),
        _ => return None,
    };

    Some(CodexErrorMessage { message, source })
}

pub fn extract_codex_error_message(event: &CodexStreamEvent) -> Option<String> {
    extract_codex_error(event).map(|error| error.message)
}

pub fn is_non_fatal_mcp_resource_probe_error(
    event: &CodexStreamEvent,
    error_message: &str,
) -> bool {
    let item = match event.item.as_ref() {
        Some(item) => item,
        None => return false,
    };

    if item.item_type != "mcp_tool_call" {
        return false;
    }

    let tool_name = item.tool.as_deref().unwrap_or_default();
    if !matches!(tool_name, "list_mcp_resources" | "read_mcp_resource") {
        return false;
    }

    error_message.contains("Method not found")
}

pub fn extract_codex_usage(event: &CodexStreamEvent) -> Option<CodexUsageSnapshot> {
    if event.event_type != "turn.completed" {
        return None;
    }

    let payload = event.usage.as_ref()?;
    if let Some(usage) = payload
        .last_token_usage
        .as_ref()
        .filter(|usage| !usage.is_empty())
    {
        return Some(CodexUsageSnapshot {
            usage: usage.clone(),
            source: CodexUsageSource::TurnDelta,
        });
    }

    let direct_usage = payload.direct_usage();
    if !direct_usage.is_empty() {
        return Some(CodexUsageSnapshot {
            usage: direct_usage,
            source: CodexUsageSource::CumulativeTotal,
        });
    }

    payload
        .total_token_usage
        .as_ref()
        .filter(|usage| !usage.is_empty())
        .map(|usage| CodexUsageSnapshot {
            usage: usage.clone(),
            source: CodexUsageSource::CumulativeTotal,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim `codex exec --json` stdout from codex-cli 0.146.0. Provenance and recapture command:
    /// `fixtures/README.md`.
    const LIVE_EXEC_JSON_REASONING_FIXTURE: &str =
        include_str!("fixtures/exec_json_reasoning_0_146_0.jsonl");

    fn codex_item(item_type: &str) -> CodexItem {
        CodexItem {
            id: None,
            item_type: item_type.to_string(),
            text: None,
            summary: None,
            server: None,
            tool: None,
            arguments: None,
            result: None,
            error: None,
            status: None,
            aggregated_output: None,
            exit_code: None,
            command: None,
            changes: None,
            sender_thread_id: None,
            receiver_thread_ids: None,
            prompt: None,
            agents_states: None,
        }
    }

    #[test]
    fn extract_codex_usage_ignores_non_turn_completed_events() {
        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: None,
            usage: Some(CodexUsagePayload {
                input_tokens: Some(10),
                cached_input_tokens: Some(3),
                output_tokens: Some(5),
                reasoning_output_tokens: None,
                total_token_usage: None,
                last_token_usage: None,
            }),
        };

        assert_eq!(extract_codex_usage(&event), None);
    }

    #[test]
    fn parse_event_msg_agent_message_normalizes_to_agent_message_item() {
        let event = parse_codex_event_line(
            r#"{"type":"event_msg","msg":{"type":"agent_message","phase":"final_answer","message":"Done from Codex.","thread_id":"codex-thread-1"}}"#,
        )
        .expect("event_msg agent message should parse");

        assert_eq!(event.event_type, "item.completed");
        assert_eq!(event.thread_id.as_deref(), Some("codex-thread-1"));
        assert_eq!(
            extract_codex_agent_message(&event).as_deref(),
            Some("Done from Codex.")
        );
    }

    #[test]
    fn parse_event_msg_agent_message_supports_text_and_top_level_ids() {
        let event = parse_codex_event_line(
            r#"{"type":"event_msg","id":"event-msg-1","thread_id":"codex-thread-top","msg":{"type":"agent_message","phase":"commentary","text":"Working from Codex."}}"#,
        )
        .expect("event_msg text fallback should parse");

        assert_eq!(event.thread_id.as_deref(), Some("codex-thread-top"));
        let item = event.item.as_ref().expect("normalized item");
        assert_eq!(item.id.as_deref(), Some("event-msg-1"));
        assert_eq!(
            extract_codex_agent_message(&event).as_deref(),
            Some("Working from Codex.")
        );
    }

    #[test]
    fn parse_event_msg_payload_agent_reasoning_normalizes_to_thinking() {
        let event = parse_codex_event_line(
            r#"{"timestamp":"2026-07-30T00:00:00Z","type":"event_msg","payload":{"type":"agent_reasoning","text":"**Checking git status**"}}"#,
        )
        .expect("event_msg payload reasoning should parse");

        assert_eq!(event.event_type, "item.completed");
        assert_eq!(
            extract_codex_reasoning(&event).as_deref(),
            Some("**Checking git status**")
        );
    }

    #[test]
    fn parse_event_msg_msg_agent_reasoning_normalizes_to_thinking() {
        let event = parse_codex_event_line(
            r#"{"type":"event_msg","msg":{"type":"agent_reasoning","text":"**Inspecting files**"}}"#,
        )
        .expect("event_msg msg reasoning should parse");

        assert_eq!(event.event_type, "item.completed");
        assert_eq!(
            extract_codex_reasoning(&event).as_deref(),
            Some("**Inspecting files**")
        );
    }

    #[test]
    fn live_exec_json_fixture_yields_reasoning_from_item_completed_only() {
        let events: Vec<CodexStreamEvent> = LIVE_EXEC_JSON_REASONING_FIXTURE
            .lines()
            .filter_map(parse_codex_event_line)
            .collect();

        assert_eq!(
            events.len(),
            12,
            "every captured line must parse; unparsed lines are silently dropped reasoning"
        );

        let reasoning: Vec<String> = events.iter().filter_map(extract_codex_reasoning).collect();
        assert_eq!(
            reasoning,
            vec![
                "**Verifying line counting commands**".to_string(),
                "**Confirming command verification**".to_string(),
            ]
        );
        for event in &events {
            if extract_codex_reasoning(event).is_some() {
                assert_eq!(event.event_type, "item.completed");
                let item = event
                    .item
                    .as_ref()
                    .expect("reasoning event carries an item");
                assert_eq!(item.item_type, "reasoning");
            }
        }
    }

    #[test]
    fn live_exec_json_fixture_reasoning_item_carries_flat_text_without_summary() {
        let item = LIVE_EXEC_JSON_REASONING_FIXTURE
            .lines()
            .filter_map(parse_codex_event_line)
            .filter_map(|event| event.item)
            .find(|item| item.item_type == "reasoning")
            .expect("fixture contains a reasoning item");

        assert_eq!(item.id.as_deref(), Some("item_2"));
        assert_eq!(
            item.text.as_deref(),
            Some("**Verifying line counting commands**")
        );
        assert!(
            item.summary.is_none(),
            "exec --json reasoning items have no summary array; that shape is rollout-only"
        );
    }

    #[test]
    fn live_exec_json_fixture_contains_no_event_msg_envelope() {
        assert!(
            !LIVE_EXEC_JSON_REASONING_FIXTURE.contains("event_msg"),
            "codex exec --json does not use the event_msg envelope; the msg/payload readers exist \
             for the rollout format and older CLIs only"
        );
    }

    #[test]
    fn agent_reasoning_delta_is_not_normalized_as_reasoning() {
        let event = parse_codex_event_line(
            r#"{"type":"event_msg","payload":{"type":"agent_reasoning_delta","text":"partial"}}"#,
        )
        .expect("unrecognized envelopes still parse as an ignored event");

        assert_eq!(event.event_type, "event_msg");
        assert!(event.item.is_none());
        assert_eq!(
            extract_codex_reasoning(&event),
            None,
            "agent_reasoning_delta does not exist in codex-cli 0.146.0"
        );
    }

    #[test]
    fn extract_codex_reasoning_supports_flat_item_text() {
        let event = parse_codex_event_line(
            r#"{"type":"item.completed","item":{"type":"reasoning","text":"Checking repository status"}}"#,
        )
        .expect("flat reasoning item should parse");

        assert_eq!(
            extract_codex_reasoning(&event).as_deref(),
            Some("Checking repository status")
        );
    }

    #[test]
    fn extract_codex_reasoning_supports_summary_item_text() {
        let event = parse_codex_event_line(
            r#"{"type":"item.completed","item":{"type":"reasoning","summary":[{"type":"summary_text","text":"Checking repository"},{"type":"summary_text","text":"status"}]}}"#,
        )
        .expect("summary reasoning item should parse");

        assert_eq!(
            extract_codex_reasoning(&event).as_deref(),
            Some("Checking repository\nstatus")
        );
    }

    #[test]
    fn extract_codex_usage_returns_turn_usage() {
        let event = CodexStreamEvent {
            event_type: "turn.completed".to_string(),
            thread_id: Some("thread-123".to_string()),
            item: None,
            usage: Some(CodexUsagePayload {
                input_tokens: Some(101),
                cached_input_tokens: Some(22),
                output_tokens: Some(33),
                reasoning_output_tokens: Some(11),
                total_token_usage: None,
                last_token_usage: None,
            }),
        };

        assert_eq!(
            extract_codex_usage(&event),
            Some(CodexUsageSnapshot {
                usage: CodexUsage {
                    input_tokens: Some(101),
                    cached_input_tokens: Some(22),
                    output_tokens: Some(33),
                    reasoning_output_tokens: Some(11),
                },
                source: CodexUsageSource::CumulativeTotal,
            })
        );
    }

    #[test]
    fn extract_codex_usage_prefers_last_token_usage_over_session_total() {
        let event = parse_codex_event_line(
            r#"{"type":"turn.completed","usage":{"total_token_usage":{"input_tokens":67362753,"cached_input_tokens":65914240,"output_tokens":109831},"last_token_usage":{"input_tokens":202091,"cached_input_tokens":201600,"output_tokens":673}}}"#,
        )
        .expect("Codex usage event should parse");

        assert_eq!(
            extract_codex_usage(&event),
            Some(CodexUsageSnapshot {
                usage: CodexUsage {
                    input_tokens: Some(202091),
                    cached_input_tokens: Some(201600),
                    output_tokens: Some(673),
                    reasoning_output_tokens: None,
                },
                source: CodexUsageSource::TurnDelta,
            })
        );
    }

    #[test]
    fn extract_codex_usage_returns_total_when_only_total_is_available() {
        let event = parse_codex_event_line(
            r#"{"type":"turn.completed","usage":{"total_token_usage":{"input_tokens":900,"cached_input_tokens":800,"output_tokens":70}}}"#,
        )
        .expect("Codex total-only usage event should parse");

        assert_eq!(
            extract_codex_usage(&event),
            Some(CodexUsageSnapshot {
                usage: CodexUsage {
                    input_tokens: Some(900),
                    cached_input_tokens: Some(800),
                    output_tokens: Some(70),
                    reasoning_output_tokens: None,
                },
                source: CodexUsageSource::CumulativeTotal,
            })
        );
    }

    #[test]
    fn extract_codex_usage_uses_direct_usage_when_last_snapshot_is_empty() {
        let event = parse_codex_event_line(
            r#"{"type":"turn.completed","usage":{"input_tokens":12,"cached_input_tokens":7,"output_tokens":3,"last_token_usage":{}}}"#,
        )
        .expect("Codex direct usage event should parse");

        assert_eq!(
            extract_codex_usage(&event),
            Some(CodexUsageSnapshot {
                usage: CodexUsage {
                    input_tokens: Some(12),
                    cached_input_tokens: Some(7),
                    output_tokens: Some(3),
                    reasoning_output_tokens: None,
                },
                source: CodexUsageSource::CumulativeTotal,
            })
        );
    }

    #[test]
    fn extract_codex_usage_ignores_empty_usage_payload() {
        let event =
            parse_codex_event_line(r#"{"type":"turn.completed","usage":{"last_token_usage":{}}}"#)
                .expect("Codex empty usage event should parse");

        assert_eq!(extract_codex_usage(&event), None);
    }

    #[test]
    fn extract_codex_tool_call_snapshot_normalizes_command_execution_as_bash() {
        let mut item = codex_item("command_execution");
        item.id = Some("item_0".to_string());
        item.command = Some("/bin/zsh -lc pwd".to_string());
        item.aggregated_output = Some("/workspace/project\n".to_string());
        item.exit_code = Some(0);
        item.status = Some("completed".to_string());

        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(item),
            usage: None,
        };

        let snapshot = extract_codex_tool_call_snapshot(&event).expect("bash snapshot");
        assert_eq!(snapshot.phase, CodexToolCallPhase::Completed);
        assert_eq!(snapshot.tool_call.id.as_deref(), Some("item_0"));
        assert_eq!(snapshot.tool_call.name, "bash");
        assert_eq!(
            snapshot.tool_call.arguments,
            serde_json::json!({ "command": "/bin/zsh -lc pwd" })
        );
        assert_eq!(
            snapshot.tool_call.result,
            Some(serde_json::json!({
                "text": "/workspace/project\n",
                "exit_code": 0,
                "status": "completed",
            }))
        );
    }

    #[test]
    fn extract_codex_file_change_snapshot_keeps_real_file_change_shape() {
        let mut item = codex_item("file_change");
        item.id = Some("item_1".to_string());
        item.status = Some("completed".to_string());
        item.changes = Some(vec![CodexFileChange {
            path: "/workspace/file.txt".to_string(),
            kind: "update".to_string(),
        }]);

        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(item),
            usage: None,
        };

        let snapshot = extract_codex_file_change_snapshot(&event).expect("file_change snapshot");
        assert_eq!(snapshot.phase, CodexToolCallPhase::Completed);
        assert_eq!(snapshot.id.as_deref(), Some("item_1"));
        assert_eq!(
            snapshot.changes,
            vec![CodexFileChange {
                path: "/workspace/file.txt".to_string(),
                kind: "update".to_string(),
            }]
        );
        assert_eq!(snapshot.status.as_deref(), Some("completed"));
    }

    #[test]
    fn extract_codex_command_execution_keeps_command() {
        let mut item = codex_item("command_execution");
        item.id = Some("item_7".to_string());
        item.command = Some("/bin/zsh -lc cargo test".to_string());
        item.status = Some("completed".to_string());
        item.aggregated_output = Some("ok\n".to_string());
        item.exit_code = Some(0);

        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(item),
            usage: None,
        };

        assert_eq!(
            extract_codex_command_execution(&event),
            Some(CodexCommandExecution {
                id: Some("item_7".to_string()),
                command: Some("/bin/zsh -lc cargo test".to_string()),
                status: Some("completed".to_string()),
                aggregated_output: Some("ok\n".to_string()),
                exit_code: Some(0),
            })
        );
    }

    #[test]
    fn resource_probe_method_not_found_is_non_fatal() {
        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(CodexItem {
                id: Some("tool-1".to_string()),
                item_type: "mcp_tool_call".to_string(),
                text: None,
                summary: None,
                server: Some("ralphx".to_string()),
                tool: Some("list_mcp_resources".to_string()),
                arguments: None,
                result: None,
                error: Some(CodexItemError {
                    message: Some(
                        "resources/list failed for 'ralphx': Mcp error: -32601: Method not found"
                            .to_string(),
                    ),
                }),
                status: None,
                aggregated_output: None,
                exit_code: None,
                command: None,
                changes: None,
                sender_thread_id: None,
                receiver_thread_ids: None,
                prompt: None,
                agents_states: None,
            }),
            usage: None,
        };

        assert!(is_non_fatal_mcp_resource_probe_error(
            &event,
            "resources/list failed for 'ralphx': Mcp error: -32601: Method not found",
        ));
    }

    #[test]
    fn normal_mcp_tool_error_is_not_marked_non_fatal() {
        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(CodexItem {
                id: Some("tool-2".to_string()),
                item_type: "mcp_tool_call".to_string(),
                text: None,
                summary: None,
                server: Some("ralphx".to_string()),
                tool: Some("delegate_start".to_string()),
                arguments: None,
                result: None,
                error: Some(CodexItemError {
                    message: Some("delegate_start failed".to_string()),
                }),
                status: None,
                aggregated_output: None,
                exit_code: None,
                command: None,
                changes: None,
                sender_thread_id: None,
                receiver_thread_ids: None,
                prompt: None,
                agents_states: None,
            }),
            usage: None,
        };

        assert!(!is_non_fatal_mcp_resource_probe_error(
            &event,
            "delegate_start failed",
        ));
    }

    #[test]
    fn extract_codex_error_marks_runtime_errors() {
        let mut item = codex_item("error");
        item.id = Some("runtime-error".to_string());
        item.error = Some(CodexItemError {
            message: Some("Error: rate_limit_exceeded".to_string()),
        });
        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(item),
            usage: None,
        };

        let error = extract_codex_error(&event).expect("runtime error");
        assert_eq!(error.source, CodexErrorSource::Runtime);
        assert_eq!(error.message, "Error: rate_limit_exceeded");
    }

    #[test]
    fn extract_codex_error_uses_runtime_text_fallback() {
        let mut item = codex_item("error");
        item.id = Some("runtime-text-error".to_string());
        item.text = Some("runtime failed before structured error".to_string());
        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(item),
            usage: None,
        };

        let error = extract_codex_error(&event).expect("runtime text error");
        assert_eq!(error.source, CodexErrorSource::Runtime);
        assert_eq!(error.message, "runtime failed before structured error");
        assert_eq!(
            extract_codex_error_message(&event).as_deref(),
            Some("runtime failed before structured error")
        );
    }

    #[test]
    fn extract_codex_error_marks_mcp_tool_errors_as_local_tool_errors() {
        let mut item = codex_item("mcp_tool_call");
        item.id = Some("tool-error".to_string());
        item.server = Some("ralphx".to_string());
        item.tool = Some("delegate_start".to_string());
        item.error = Some(CodexItemError {
            message: Some("delegate_start saw local rate_limit metadata".to_string()),
        });
        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(item),
            usage: None,
        };

        let error = extract_codex_error(&event).expect("mcp error");
        assert_eq!(error.source, CodexErrorSource::McpTool);
        assert_eq!(
            error.message,
            "delegate_start saw local rate_limit metadata"
        );
    }

    #[test]
    fn extract_codex_error_ignores_items_without_errors() {
        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(codex_item("command_execution")),
            usage: None,
        };
        assert_eq!(extract_codex_error(&event), None);

        let event = CodexStreamEvent {
            event_type: "item.completed".to_string(),
            thread_id: None,
            item: Some(codex_item("mcp_tool_call")),
            usage: None,
        };
        assert_eq!(extract_codex_error(&event), None);
    }
}
