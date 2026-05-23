use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::infrastructure::agents::claude::ToolCall;

const TOOL_RESULT_PREVIEW_MAX_LINES: usize = 10;
const TOOL_RESULT_PREVIEW_MAX_CHARS: usize = 4_000;
const TOOL_RESULT_PREVIEW_MAX_ARRAY_ITEMS: usize = 50;
const TOOL_RESULT_PREVIEW_MAX_OBJECT_FIELDS: usize = 80;
const TOOL_RESULT_PREVIEW_MARKER: &str = "__ralphx_preview_truncated";
const TOOL_RESULT_PREVIEW_OMITTED_ITEMS: &str = "__ralphx_preview_omitted_items";
const TOOL_RESULT_PREVIEW_OMITTED_FIELDS: &str = "__ralphx_preview_omitted_fields";

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
    pub paths: Vec<String>,
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

#[derive(Debug, Clone, PartialEq)]
struct StructuredToolResultPreview {
    value: JsonValue,
    paths: Vec<String>,
}

fn object_child_path(parent: &str, key: &str) -> String {
    if parent == "$" {
        format!("$.{key}")
    } else {
        format!("{parent}.{key}")
    }
}

fn array_child_path(parent: &str, index: usize) -> String {
    format!("{parent}[{index}]")
}

fn preview_string_leaf(value: &str, path: &str) -> Option<StructuredToolResultPreview> {
    let preview = truncate_preview_text(value)?;
    Some(StructuredToolResultPreview {
        value: JsonValue::String(preview.text),
        paths: vec![path.to_string()],
    })
}

fn preview_array(items: &[JsonValue], path: &str) -> Option<StructuredToolResultPreview> {
    let keep_count = items.len().min(TOOL_RESULT_PREVIEW_MAX_ARRAY_ITEMS);
    let mut changed = items.len() > keep_count;
    let mut paths = Vec::new();
    let mut preview_items = Vec::with_capacity(keep_count + usize::from(changed));

    for (index, item) in items.iter().take(keep_count).enumerate() {
        if let Some(preview) = preview_value(item, &array_child_path(path, index)) {
            preview_items.push(preview.value);
            paths.extend(preview.paths);
            changed = true;
        } else {
            preview_items.push(item.clone());
        }
    }

    if items.len() > keep_count {
        preview_items.push(serde_json::json!({
            TOOL_RESULT_PREVIEW_MARKER: true,
            TOOL_RESULT_PREVIEW_OMITTED_ITEMS: items.len() - keep_count,
        }));
        paths.push(format!("{path}[{keep_count}:]"));
    }

    changed.then_some(StructuredToolResultPreview {
        value: JsonValue::Array(preview_items),
        paths,
    })
}

fn preview_object(
    object: &JsonMap<String, JsonValue>,
    path: &str,
) -> Option<StructuredToolResultPreview> {
    let keep_count = object.len().min(TOOL_RESULT_PREVIEW_MAX_OBJECT_FIELDS);
    let mut changed = object.len() > keep_count;
    let mut paths = Vec::new();
    let mut preview_object = JsonMap::new();

    for (key, value) in object.iter().take(keep_count) {
        if let Some(preview) = preview_value(value, &object_child_path(path, key)) {
            preview_object.insert(key.clone(), preview.value);
            paths.extend(preview.paths);
            changed = true;
        } else {
            preview_object.insert(key.clone(), value.clone());
        }
    }

    if object.len() > keep_count {
        preview_object.insert(
            TOOL_RESULT_PREVIEW_MARKER.to_string(),
            JsonValue::Bool(true),
        );
        preview_object.insert(
            TOOL_RESULT_PREVIEW_OMITTED_FIELDS.to_string(),
            serde_json::json!(object.len() - keep_count),
        );
        paths.push(format!("{path}.*"));
    }

    changed.then_some(StructuredToolResultPreview {
        value: JsonValue::Object(preview_object),
        paths,
    })
}

fn preview_value(value: &JsonValue, path: &str) -> Option<StructuredToolResultPreview> {
    match value {
        JsonValue::String(text) => preview_string_leaf(text, path),
        JsonValue::Array(items) => preview_array(items, path),
        JsonValue::Object(object) => preview_object(object, path),
        _ => None,
    }
}

fn fallback_preview_value(preview: &ToolResultPreview) -> StructuredToolResultPreview {
    StructuredToolResultPreview {
        value: serde_json::json!({
            TOOL_RESULT_PREVIEW_MARKER: true,
            "preview_text": preview.text,
        }),
        paths: vec!["$".to_string()],
    }
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
    let structured_preview =
        preview_value(result, "$").unwrap_or_else(|| fallback_preview_value(&preview));
    Some(ToolResultPreviewPayload {
        result: structured_preview.value,
        original_bytes: preview.original_bytes,
        line_count: preview.line_count,
        omitted_lines: preview.omitted_lines,
        paths: structured_preview.paths,
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

pub(crate) fn build_live_tool_result_preview_for_tool_id(
    tool_calls: &[ToolCall],
    conversation_id: Option<&str>,
    message_id: Option<&str>,
    tool_call_id: &str,
    result: &JsonValue,
) -> LiveToolResultPreview {
    let original_tool_name = tool_calls
        .iter()
        .find(|tool_call| tool_call.id.as_deref() == Some(tool_call_id))
        .map(|tool_call| tool_call.name.as_str());
    let detail_ref = message_id.and_then(|message_id| {
        conversation_id.map(|conversation_id| {
            tool_detail_ref(conversation_id, message_id, Some(tool_call_id), None)
        })
    });

    build_live_tool_result_preview(original_tool_name, result, detail_ref)
}

pub(crate) fn build_live_tool_result_preview_for_tool_call(
    conversation_id: &str,
    message_id: Option<&str>,
    tool_call: &ToolCall,
) -> Option<LiveToolResultPreview> {
    let result = tool_call.result.as_ref()?;
    let detail_ref = message_id.map(|message_id| {
        tool_detail_ref(conversation_id, message_id, tool_call.id.as_deref(), None)
    });

    Some(build_live_tool_result_preview(
        Some(&tool_call.name),
        result,
        detail_ref,
    ))
}

pub(crate) fn live_tool_result_activity_content(result_preview: &LiveToolResultPreview) -> String {
    serde_json::to_string(&result_preview.result).unwrap_or_default()
}

pub(crate) fn live_tool_result_activity_metadata(
    tool_use_id: &str,
    result_preview: &LiveToolResultPreview,
) -> JsonValue {
    serde_json::json!({
        "tool_use_id": tool_use_id,
        "result_preview_truncated": result_preview.is_previewed(),
    })
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
    if !preview.paths.is_empty() {
        object.insert(
            "result_preview_paths".to_string(),
            serde_json::json!(preview.paths),
        );
    }
    if let Some(detail_ref) = preview.detail_ref {
        object.insert("detail_ref".to_string(), detail_ref);
    }

    true
}
