use std::path::Path;

use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::infrastructure::agents::claude::ToolCall;

const TOOL_RESULT_PREVIEW_MAX_LINES: usize = 10;
const TOOL_RESULT_PREVIEW_MAX_CHARS: usize = 4_000;
const TOOL_RESULT_PREVIEW_MAX_ARRAY_ITEMS: usize = 50;
const TOOL_RESULT_PREVIEW_MAX_OBJECT_FIELDS: usize = 80;
const TOOL_RESULT_PREVIEW_MARKER: &str = "__ralphx_preview_truncated";
const TOOL_RESULT_PREVIEW_OMITTED_ITEMS: &str = "__ralphx_preview_omitted_items";
const TOOL_RESULT_PREVIEW_OMITTED_FIELDS: &str = "__ralphx_preview_omitted_fields";
const TOOL_ARGUMENT_DIFF_PREVIEW_CONTEXT_LINES: usize = 3;
const TOOL_ARGUMENT_FINAL_CONTENT_PREVIEW_MAX_LINES: usize = 10;
const TOOL_ARGUMENT_FINAL_CONTENT_PREVIEW_MAX_CHARS: usize = 4_000;

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
pub(crate) struct ToolArgumentPreviewPayload {
    pub arguments: JsonValue,
    pub diff_context: Option<JsonValue>,
    pub original_bytes: usize,
    pub line_count: usize,
    pub omitted_lines: usize,
    pub diff_preview: Option<JsonValue>,
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
    serde_json::from_str::<JsonValue>(value)
        .ok()
        .and_then(|parsed_json| preview_value(&parsed_json, path))
        .and_then(|preview| {
            let text = serde_json::to_string(&preview.value).ok()?;
            let value = JsonValue::String(text);
            Some(StructuredToolResultPreview {
                value,
                paths: preview.paths,
            })
        })
        .or_else(|| {
            let preview = truncate_preview_text(value)?;
            Some(StructuredToolResultPreview {
                value: JsonValue::String(preview.text),
                paths: vec![path.to_string()],
            })
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
    let normalized =
        canonical_tool_payload_name(Some(name)).unwrap_or_else(|| name.trim().to_ascii_lowercase());
    matches!(
        normalized.as_str(),
        "task" | "agent" | "delegate_start" | "ask_user_question"
    )
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

fn canonical_tool_payload_name(name: Option<&str>) -> Option<String> {
    let mut normalized = name?.trim().to_ascii_lowercase();
    for prefix in [
        "mcp__ralphx__",
        "mcp__ralphx_internal__",
        "ralphx::",
        "ralphx_internal::",
        "ralphx:",
        "ralphx_internal:",
    ] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.to_string();
            break;
        }
    }
    Some(normalized)
}

fn tool_argument_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.split('\n').count()
    }
}

fn tool_argument_diff_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').collect()
    }
}

fn tool_argument_language_from_path(file_path: &str) -> &'static str {
    match Path::new(file_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("ts") | Some("tsx") => "typescript",
        Some("js") | Some("jsx") => "javascript",
        Some("rs") => "rust",
        Some("css") => "css",
        Some("html") => "html",
        Some("json") => "json",
        Some("md") => "markdown",
        _ => "text",
    }
}

fn hunk_header(old_start: usize, old_lines: usize, new_start: usize, new_lines: usize) -> String {
    format!("@@ -{old_start},{old_lines} +{new_start},{new_lines} @@")
}

fn build_tool_argument_diff_preview(
    file_path: &str,
    old_content: &str,
    new_content: &str,
) -> JsonValue {
    let old_lines = tool_argument_diff_lines(old_content);
    let new_lines = tool_argument_diff_lines(new_content);

    let mut prefix = 0usize;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }

    if prefix == old_lines.len() && prefix == new_lines.len() {
        return serde_json::json!({
            "file_path": file_path,
            "language": tool_argument_language_from_path(file_path),
            "hunks": [],
            "old_total_lines": tool_argument_line_count(old_content),
            "new_total_lines": tool_argument_line_count(new_content),
            "is_binary": false,
        });
    }

    let old_changed_end = if prefix < old_lines.len() {
        prefix + 1
    } else {
        prefix
    };
    let new_changed_end = if prefix < new_lines.len() {
        prefix + 1
    } else {
        prefix
    };

    let old_context_start = prefix.saturating_sub(TOOL_ARGUMENT_DIFF_PREVIEW_CONTEXT_LINES);
    let new_context_start = prefix.saturating_sub(TOOL_ARGUMENT_DIFF_PREVIEW_CONTEXT_LINES);
    let old_context_end =
        (old_changed_end + TOOL_ARGUMENT_DIFF_PREVIEW_CONTEXT_LINES).min(old_lines.len());
    let new_context_end =
        (new_changed_end + TOOL_ARGUMENT_DIFF_PREVIEW_CONTEXT_LINES).min(new_lines.len());
    let post_context_count = old_context_end
        .saturating_sub(old_changed_end)
        .min(new_context_end.saturating_sub(new_changed_end));

    let mut lines = Vec::new();
    let mut old_line_num = old_context_start + 1;
    let mut new_line_num = new_context_start + 1;

    for content in old_lines.iter().take(prefix).skip(old_context_start) {
        lines.push(serde_json::json!({
            "kind": "context",
            "content": *content,
            "old_line_num": old_line_num,
            "new_line_num": new_line_num,
        }));
        old_line_num += 1;
        new_line_num += 1;
    }

    for content in old_lines.iter().take(old_changed_end).skip(prefix) {
        lines.push(serde_json::json!({
            "kind": "deletion",
            "content": *content,
            "old_line_num": old_line_num,
            "new_line_num": null,
        }));
        old_line_num += 1;
    }

    for content in new_lines.iter().take(new_changed_end).skip(prefix) {
        lines.push(serde_json::json!({
            "kind": "addition",
            "content": *content,
            "old_line_num": null,
            "new_line_num": new_line_num,
        }));
        new_line_num += 1;
    }

    for offset in 0..post_context_count {
        lines.push(serde_json::json!({
            "kind": "context",
            "content": old_lines[old_changed_end + offset],
            "old_line_num": old_line_num,
            "new_line_num": new_line_num,
        }));
        old_line_num += 1;
        new_line_num += 1;
    }

    let old_hunk_lines = prefix.saturating_sub(old_context_start)
        + old_changed_end.saturating_sub(prefix)
        + post_context_count;
    let new_hunk_lines = prefix.saturating_sub(new_context_start)
        + new_changed_end.saturating_sub(prefix)
        + post_context_count;
    let old_start = old_context_start + 1;
    let new_start = new_context_start + 1;

    serde_json::json!({
        "file_path": file_path,
        "language": tool_argument_language_from_path(file_path),
        "hunks": [{
            "old_start": old_start,
            "old_lines": old_hunk_lines,
            "new_start": new_start,
            "new_lines": new_hunk_lines,
            "header": hunk_header(old_start, old_hunk_lines, new_start, new_hunk_lines),
            "lines": lines,
        }],
        "old_total_lines": tool_argument_line_count(old_content),
        "new_total_lines": tool_argument_line_count(new_content),
        "is_binary": false,
    })
}

fn diff_preview_visible_line_count(diff_preview: &JsonValue) -> usize {
    diff_preview
        .get("hunks")
        .and_then(JsonValue::as_array)
        .map(|hunks| {
            hunks
                .iter()
                .filter_map(|hunk| hunk.get("lines").and_then(JsonValue::as_array))
                .map(Vec::len)
                .sum()
        })
        .unwrap_or(0)
}

fn truncate_tool_argument_content_preview(content: &str) -> Option<(String, usize, usize)> {
    let line_count = tool_argument_line_count(content);
    if line_count <= TOOL_ARGUMENT_FINAL_CONTENT_PREVIEW_MAX_LINES
        && content.chars().count() <= TOOL_ARGUMENT_FINAL_CONTENT_PREVIEW_MAX_CHARS
    {
        return None;
    }

    let mut preview = String::new();
    let mut preview_lines = 0usize;
    let mut char_count = 0usize;
    'outer: for (line_index, line) in content.lines().enumerate() {
        if line_index >= TOOL_ARGUMENT_FINAL_CONTENT_PREVIEW_MAX_LINES {
            break;
        }
        if line_index > 0 {
            preview.push('\n');
        }
        preview_lines += 1;
        for ch in line.chars() {
            if char_count >= TOOL_ARGUMENT_FINAL_CONTENT_PREVIEW_MAX_CHARS {
                break 'outer;
            }
            preview.push(ch);
            char_count += 1;
        }
    }

    Some((
        preview,
        line_count,
        line_count.saturating_sub(preview_lines),
    ))
}

fn preview_argument_object_without_keys(
    arguments: &JsonMap<String, JsonValue>,
    remove_keys: &[&str],
) -> JsonValue {
    let mut preview = arguments.clone();
    for key in remove_keys {
        preview.remove(*key);
    }
    JsonValue::Object(preview)
}

fn insert_tool_argument_preview_metadata(
    object: &mut JsonMap<String, JsonValue>,
    original_bytes: usize,
    line_count: usize,
    omitted_lines: usize,
    detail_ref: Option<JsonValue>,
) {
    object.insert(
        "arguments_preview_truncated".to_string(),
        JsonValue::Bool(true),
    );
    object.insert(
        "arguments_preview_original_bytes".to_string(),
        serde_json::json!(original_bytes),
    );
    object.insert(
        "arguments_preview_line_count".to_string(),
        serde_json::json!(line_count),
    );
    object.insert(
        "arguments_preview_omitted_lines".to_string(),
        serde_json::json!(omitted_lines),
    );
    if let Some(detail_ref) = detail_ref {
        object.insert("detail_ref".to_string(), detail_ref);
    }
}

fn remove_diff_context_old_content(object: &mut JsonMap<String, JsonValue>) {
    for key in ["diff_context", "diffContext"] {
        if let Some(diff_context) = object.get_mut(key).and_then(JsonValue::as_object_mut) {
            diff_context.remove("old_content");
            diff_context.remove("oldContent");
        }
    }
}

fn tool_call_diff_context_old_content(object: &JsonMap<String, JsonValue>) -> Option<String> {
    for key in ["diff_context", "diffContext"] {
        let Some(diff_context) = object.get(key).and_then(JsonValue::as_object) else {
            continue;
        };
        if let Some(old_content) = diff_context
            .get("old_content")
            .or_else(|| diff_context.get("oldContent"))
            .and_then(JsonValue::as_str)
        {
            return Some(old_content.to_string());
        }
    }
    None
}

fn tool_call_diff_context_old_file_exists(object: &JsonMap<String, JsonValue>) -> Option<bool> {
    for key in ["diff_context", "diffContext"] {
        let Some(diff_context) = object.get(key).and_then(JsonValue::as_object) else {
            continue;
        };
        if let Some(old_file_exists) = diff_context
            .get("old_file_exists")
            .or_else(|| diff_context.get("oldFileExists"))
            .and_then(JsonValue::as_bool)
        {
            return Some(old_file_exists);
        }
    }
    None
}

fn preview_edit_tool_arguments(
    object: &mut JsonMap<String, JsonValue>,
    arguments: &JsonMap<String, JsonValue>,
    detail_ref: Option<JsonValue>,
) -> bool {
    let Some(file_path) = arguments.get("file_path").and_then(JsonValue::as_str) else {
        return false;
    };
    let Some(old_string) = arguments.get("old_string").and_then(JsonValue::as_str) else {
        return false;
    };
    let Some(new_string) = arguments.get("new_string").and_then(JsonValue::as_str) else {
        return false;
    };

    let diff_preview = build_tool_argument_diff_preview(file_path, old_string, new_string);
    let line_count = tool_argument_line_count(old_string) + tool_argument_line_count(new_string);
    let omitted_lines = line_count.saturating_sub(diff_preview_visible_line_count(&diff_preview));
    object.insert(
        "arguments".to_string(),
        preview_argument_object_without_keys(arguments, &["old_string", "new_string"]),
    );
    object.insert("diff_preview".to_string(), diff_preview);
    insert_tool_argument_preview_metadata(
        object,
        old_string.len() + new_string.len(),
        line_count,
        omitted_lines,
        detail_ref,
    );
    true
}

fn preview_write_tool_arguments(
    object: &mut JsonMap<String, JsonValue>,
    arguments: &JsonMap<String, JsonValue>,
    detail_ref: Option<JsonValue>,
) -> bool {
    let Some(file_path) = arguments.get("file_path").and_then(JsonValue::as_str) else {
        return false;
    };
    let Some(content) = arguments.get("content").and_then(JsonValue::as_str) else {
        return false;
    };

    if let Some(old_content) = tool_call_diff_context_old_content(object) {
        let diff_preview = build_tool_argument_diff_preview(file_path, &old_content, content);
        let line_count = tool_argument_line_count(&old_content) + tool_argument_line_count(content);
        let omitted_lines =
            line_count.saturating_sub(diff_preview_visible_line_count(&diff_preview));
        object.insert(
            "arguments".to_string(),
            preview_argument_object_without_keys(arguments, &["content"]),
        );
        object.insert("diff_preview".to_string(), diff_preview);
        remove_diff_context_old_content(object);
        insert_tool_argument_preview_metadata(
            object,
            old_content.len() + content.len(),
            line_count,
            omitted_lines,
            detail_ref,
        );
        return true;
    }

    if tool_call_diff_context_old_file_exists(object) == Some(false) {
        let diff_preview = build_tool_argument_diff_preview(file_path, "", content);
        let line_count = tool_argument_line_count(content);
        let omitted_lines =
            line_count.saturating_sub(diff_preview_visible_line_count(&diff_preview));
        object.insert(
            "arguments".to_string(),
            preview_argument_object_without_keys(arguments, &["content"]),
        );
        object.insert("diff_preview".to_string(), diff_preview);
        remove_diff_context_old_content(object);
        insert_tool_argument_preview_metadata(
            object,
            content.len(),
            line_count,
            omitted_lines,
            detail_ref,
        );
        return true;
    }

    let Some((preview, line_count, omitted_lines)) =
        truncate_tool_argument_content_preview(content)
    else {
        return false;
    };
    let mut preview_arguments = arguments.clone();
    preview_arguments.insert("content".to_string(), JsonValue::String(preview));
    object.insert(
        "arguments".to_string(),
        JsonValue::Object(preview_arguments),
    );
    insert_tool_argument_preview_metadata(
        object,
        content.len(),
        line_count,
        omitted_lines,
        detail_ref,
    );
    true
}

pub(crate) fn preview_tool_arguments_object(
    object: &mut JsonMap<String, JsonValue>,
    detail_ref: Option<JsonValue>,
) -> bool {
    if object
        .get("arguments_preview_truncated")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
    {
        return false;
    }

    let Some(tool_name) =
        canonical_tool_payload_name(object.get("name").and_then(JsonValue::as_str))
    else {
        return false;
    };
    let Some(arguments) = object
        .get("arguments")
        .and_then(JsonValue::as_object)
        .cloned()
    else {
        return false;
    };

    match tool_name.as_str() {
        "edit" => preview_edit_tool_arguments(object, &arguments, detail_ref),
        "write" => preview_write_tool_arguments(object, &arguments, detail_ref),
        _ => false,
    }
}

fn usize_object_field(object: &JsonMap<String, JsonValue>, key: &str) -> Option<usize> {
    object
        .get(key)
        .and_then(JsonValue::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

pub(crate) fn build_live_tool_argument_preview(
    tool_call: &ToolCall,
    diff_context: Option<&JsonValue>,
    detail_ref: Option<JsonValue>,
) -> Option<ToolArgumentPreviewPayload> {
    let mut object = JsonMap::new();
    object.insert(
        "name".to_string(),
        JsonValue::String(tool_call.name.clone()),
    );
    object.insert("arguments".to_string(), tool_call.arguments.clone());
    if let Some(diff_context) = diff_context {
        object.insert("diff_context".to_string(), diff_context.clone());
    }

    if !preview_tool_arguments_object(&mut object, detail_ref) {
        return None;
    }

    Some(ToolArgumentPreviewPayload {
        arguments: object
            .remove("arguments")
            .unwrap_or_else(|| tool_call.arguments.clone()),
        diff_context: object.remove("diff_context"),
        original_bytes: usize_object_field(&object, "arguments_preview_original_bytes")
            .unwrap_or(0),
        line_count: usize_object_field(&object, "arguments_preview_line_count").unwrap_or(0),
        omitted_lines: usize_object_field(&object, "arguments_preview_omitted_lines").unwrap_or(0),
        diff_preview: object.remove("diff_preview"),
        detail_ref: object.remove("detail_ref"),
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
