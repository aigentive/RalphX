use serde::{Deserialize, Serialize};

use crate::infrastructure::agents::claude::ToolCall;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexUsage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexItem {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub text: Option<String>,
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
    pub usage: Option<CodexUsage>,
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

    serde_json::from_str(trimmed).ok()
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

pub fn extract_codex_usage(event: &CodexStreamEvent) -> Option<CodexUsage> {
    if event.event_type != "turn.completed" {
        return None;
    }

    event.usage.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_item(item_type: &str) -> CodexItem {
        CodexItem {
            id: None,
            item_type: item_type.to_string(),
            text: None,
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
            usage: Some(CodexUsage {
                input_tokens: Some(10),
                cached_input_tokens: Some(3),
                output_tokens: Some(5),
            }),
        };

        assert_eq!(extract_codex_usage(&event), None);
    }

    #[test]
    fn extract_codex_usage_returns_turn_usage() {
        let event = CodexStreamEvent {
            event_type: "turn.completed".to_string(),
            thread_id: Some("thread-123".to_string()),
            item: None,
            usage: Some(CodexUsage {
                input_tokens: Some(101),
                cached_input_tokens: Some(22),
                output_tokens: Some(33),
            }),
        };

        assert_eq!(
            extract_codex_usage(&event),
            Some(CodexUsage {
                input_tokens: Some(101),
                cached_input_tokens: Some(22),
                output_tokens: Some(33),
            })
        );
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
