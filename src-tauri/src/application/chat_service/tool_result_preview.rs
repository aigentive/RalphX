use serde_json::{Map as JsonMap, Value as JsonValue};

const TOOL_RESULT_PREVIEW_MAX_LINES: usize = 10;
const TOOL_RESULT_PREVIEW_MAX_CHARS: usize = 4_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolResultPreview {
    pub text: String,
    pub original_bytes: usize,
    pub line_count: usize,
    pub omitted_lines: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolResultPreviewPayload {
    pub result: JsonValue,
    pub original_bytes: usize,
    pub line_count: usize,
    pub omitted_lines: usize,
    pub detail_ref: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LiveToolResultPreview {
    pub result: JsonValue,
    pub preview: Option<ToolResultPreviewPayload>,
}

impl LiveToolResultPreview {
    pub(crate) fn is_previewed(&self) -> bool {
        self.preview.is_some()
    }
}

fn tool_result_preview_text(value: &JsonValue) -> String {
    match value {
        JsonValue::String(text) => text.clone(),
        JsonValue::Array(items) => {
            let text_items: Vec<&str> = items
                .iter()
                .filter_map(|item| item.get("text").and_then(JsonValue::as_str))
                .collect();
            if !text_items.is_empty() {
                text_items.join("\n")
            } else {
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            }
        }
        JsonValue::Object(object) => {
            if let Some(text) = object.get("text").and_then(JsonValue::as_str) {
                return text.to_string();
            }
            if let Some(text) = object.get("content").and_then(JsonValue::as_str) {
                return text.to_string();
            }
            if let Some(text) = object.get("output").and_then(JsonValue::as_str) {
                return text.to_string();
            }
            if let Some(text) = object.get("aggregated_output").and_then(JsonValue::as_str) {
                return text.to_string();
            }
            if let Some(text) = object.get("aggregatedOutput").and_then(JsonValue::as_str) {
                return text.to_string();
            }
            if let Some(JsonValue::Array(items)) = object.get("content") {
                let text_items: Vec<&str> = items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(JsonValue::as_str))
                    .collect();
                if !text_items.is_empty() {
                    return text_items.join("\n");
                }
            }
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn truncate_preview_text(text: &str) -> Option<ToolResultPreview> {
    let line_count = if text.is_empty() {
        0
    } else {
        text.lines().count().max(1)
    };
    if line_count <= TOOL_RESULT_PREVIEW_MAX_LINES
        && text.chars().count() <= TOOL_RESULT_PREVIEW_MAX_CHARS
    {
        return None;
    }

    let mut preview = String::new();
    let mut preview_lines = 0usize;
    let mut char_count = 0usize;

    'outer: for (line_index, line) in text.lines().enumerate() {
        if line_index >= TOOL_RESULT_PREVIEW_MAX_LINES {
            break;
        }
        if line_index > 0 {
            preview.push('\n');
        }
        preview_lines += 1;
        for ch in line.chars() {
            if char_count >= TOOL_RESULT_PREVIEW_MAX_CHARS {
                break 'outer;
            }
            preview.push(ch);
            char_count += 1;
        }
    }

    Some(ToolResultPreview {
        text: preview,
        original_bytes: text.len(),
        line_count,
        omitted_lines: line_count.saturating_sub(preview_lines),
    })
}

pub(crate) fn build_tool_result_preview(value: &JsonValue) -> Option<ToolResultPreview> {
    truncate_preview_text(&tool_result_preview_text(value))
}

pub(crate) fn should_skip_tool_result_preview(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let normalized = name.to_ascii_lowercase();
    matches!(normalized.as_str(), "task" | "agent" | "delegate_start")
        || normalized.ends_with("::delegate_start")
        || normalized.ends_with("__delegate_start")
}

pub(crate) fn tool_detail_ref(
    conversation_id: &str,
    message_id: &str,
    tool_call_id: Option<&str>,
    content_block_index: Option<usize>,
) -> JsonValue {
    serde_json::json!({
        "conversation_id": conversation_id,
        "message_id": message_id,
        "tool_call_id": tool_call_id,
        "content_block_index": content_block_index,
    })
}

pub(crate) fn build_tool_result_preview_payload(
    tool_name: Option<&str>,
    result: &JsonValue,
    detail_ref: Option<JsonValue>,
) -> Option<ToolResultPreviewPayload> {
    if should_skip_tool_result_preview(tool_name) {
        return None;
    }
    let preview = build_tool_result_preview(result)?;
    Some(ToolResultPreviewPayload {
        result: JsonValue::String(preview.text),
        original_bytes: preview.original_bytes,
        line_count: preview.line_count,
        omitted_lines: preview.omitted_lines,
        detail_ref,
    })
}

pub(crate) fn build_live_tool_result_preview(
    tool_name: Option<&str>,
    result: &JsonValue,
    detail_ref: Option<JsonValue>,
) -> LiveToolResultPreview {
    let preview = tool_name
        .and_then(|name| build_tool_result_preview_payload(Some(name), result, detail_ref));
    let result = preview
        .as_ref()
        .map(|preview| preview.result.clone())
        .unwrap_or_else(|| result.clone());
    LiveToolResultPreview { result, preview }
}

pub(crate) fn preview_tool_result_object(
    object: &mut JsonMap<String, JsonValue>,
    detail_ref: Option<JsonValue>,
) -> bool {
    if object
        .get("result_preview_truncated")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
    {
        return false;
    }

    let Some(result) = object.get("result") else {
        return false;
    };
    if result.is_null() {
        return false;
    }

    let Some(preview) = build_tool_result_preview_payload(
        object.get("name").and_then(JsonValue::as_str),
        result,
        detail_ref,
    ) else {
        return false;
    };

    object.insert("result".to_string(), preview.result);
    object.insert(
        "result_preview_truncated".to_string(),
        JsonValue::Bool(true),
    );
    object.insert(
        "result_preview_original_bytes".to_string(),
        serde_json::json!(preview.original_bytes),
    );
    object.insert(
        "result_preview_line_count".to_string(),
        serde_json::json!(preview.line_count),
    );
    object.insert(
        "result_preview_omitted_lines".to_string(),
        serde_json::json!(preview.omitted_lines),
    );
    if let Some(detail_ref) = preview.detail_ref {
        object.insert("detail_ref".to_string(), detail_ref);
    }

    true
}
