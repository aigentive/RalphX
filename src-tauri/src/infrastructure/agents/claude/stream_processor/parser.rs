// Stream parsing and detection functions
// Stateless parsing helpers for extracting data from Claude CLI stream-json output

use super::types::{ParsedLine, StreamMessage};

/// Parse a `<usage>...</usage>` block from text to extract task completion stats.
///
/// The Claude CLI Task tool result format is:
/// ```text
/// [subagent text output]
/// agentId: abc1234 (for resuming...)
/// <usage>total_tokens: 12345
/// tool_uses: 8
/// duration_ms: 45000</usage>
/// ```
pub(crate) fn parse_usage_text(
    text: &str,
) -> (Option<String>, Option<u64>, Option<u64>, Option<u64>) {
    let agent_id = text.find("agentId:").and_then(|start| {
        let after = &text[start + "agentId:".len()..];
        let trimmed = after.trim_start();
        // agentId is a hex string, take chars until non-hex
        let end = trimmed
            .find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or(trimmed.len());
        if end > 0 {
            Some(trimmed[..end].to_string())
        } else {
            None
        }
    });

    let (duration_ms, total_tokens, tool_use_count) =
        if let Some(usage_start) = text.find("<usage>") {
            let usage_end = text.find("</usage>").unwrap_or(text.len());
            let usage_block = &text[usage_start + "<usage>".len()..usage_end];

            let duration = extract_stat(usage_block, "duration_ms:");
            let tokens = extract_stat(usage_block, "total_tokens:");
            let tools = extract_stat(usage_block, "tool_uses:");

            (duration, tokens, tools)
        } else {
            (None, None, None)
        };

    (agent_id, duration_ms, total_tokens, tool_use_count)
}

/// Extract a numeric stat value from a line like "key: 12345"
pub(crate) fn extract_stat(block: &str, key: &str) -> Option<u64> {
    block.find(key).and_then(|start| {
        let after = &block[start + key.len()..];
        let trimmed = after.trim_start();
        let end = trimmed
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(trimmed.len());
        if end > 0 {
            trimmed[..end].parse::<u64>().ok()
        } else {
            None
        }
    })
}

/// Convert a serde_json::Value to a flat text string for usage tag parsing.
/// Handles: plain strings, arrays of content blocks (with "text" type), and JSON objects.
pub(crate) fn value_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    item.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

/// Parse a stream-json line, extracting parent_tool_use_id from the top-level JSON envelope
pub(crate) fn parse_line(line: &str) -> Option<ParsedLine> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return None;
    }

    let candidate = if let Some(rest) = trimmed.strip_prefix("data:") {
        rest.trim()
    } else {
        trimmed
    };

    // First parse raw JSON so we can extract metadata and support envelope drift.
    let raw_value = serde_json::from_str::<serde_json::Value>(candidate).ok()?;
    let parent_tool_use_id = raw_value
        .get("parent_tool_use_id")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string());
    let parent_tool_use_id = parent_tool_use_id.or_else(|| {
        raw_value
            .get("message")
            .and_then(|m| m.get("parent_tool_use_id"))
            .and_then(|p| p.as_str())
            .map(|s| s.to_string())
    });
    let is_synthetic = raw_value
        .get("isSynthetic")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // Extract top-level tool_use_result before raw_value is consumed.
    // Claude Code puts structured metadata here (e.g. {"status": "teammate_spawned", ...})
    // which is separate from message.content[].content (the text result).
    let tool_use_result = raw_value
        .get("tool_use_result")
        .filter(|v| v.is_object())
        .cloned();

    // Parse either direct event objects ({type: ...}) or wrapped envelopes
    // ({message: {type: ...}}, {data: {type: ...}}, {event: {type: ...}}).
    let message_value =
        if raw_value.get("type").and_then(|value| value.as_str()) == Some("stream_event") {
            raw_value
                .get("event")
                .filter(|value| value.is_object())?
                .clone()
        } else if raw_value.get("type").is_some() {
            raw_value
        } else if let Some(inner) = raw_value.get("message").filter(|v| v.is_object()) {
            inner.clone()
        } else if let Some(inner) = raw_value.get("data").filter(|v| v.is_object()) {
            inner.clone()
        } else if let Some(inner) = raw_value.get("event").filter(|v| v.is_object()) {
            inner.clone()
        } else {
            return None;
        };

    let message: StreamMessage = serde_json::from_value(message_value).ok()?;
    Some(ParsedLine {
        message,
        parent_tool_use_id,
        is_synthetic,
        tool_use_result,
    })
}
