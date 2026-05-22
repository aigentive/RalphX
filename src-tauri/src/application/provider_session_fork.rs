use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSessionForkResult {
    pub session_ref: ProviderSessionRef,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
}

pub fn fork_provider_session_from_state_home(
    parent_ref: &ProviderSessionRef,
) -> AppResult<ProviderSessionForkResult> {
    let child_session_id = Uuid::new_v4().to_string();
    let home = provider_state_home_dir()?;
    fork_provider_session_under(
        parent_ref.harness,
        &parent_ref.provider_session_id,
        &child_session_id,
        &home,
    )
}

pub fn fork_provider_session_under(
    harness: AgentHarnessKind,
    parent_session_id: &str,
    child_session_id: &str,
    home: &Path,
) -> AppResult<ProviderSessionForkResult> {
    validate_provider_session_id(parent_session_id)?;
    validate_provider_session_id(child_session_id)?;
    if parent_session_id == child_session_id {
        return Err(AppError::Validation(
            "Forked provider session id must differ from parent session id".to_string(),
        ));
    }

    match harness {
        AgentHarnessKind::Claude => {
            fork_claude_session_under(parent_session_id, child_session_id, home)
        }
        AgentHarnessKind::Codex => {
            fork_codex_session_under(parent_session_id, child_session_id, home)
        }
    }
}

fn provider_state_home_dir() -> AppResult<PathBuf> {
    if let Ok(value) = std::env::var("RALPHX_PROVIDER_STATE_HOME_OVERRIDE") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Infrastructure("HOME is not set".to_string()))
}

fn fork_claude_session_under(
    parent_session_id: &str,
    child_session_id: &str,
    home: &Path,
) -> AppResult<ProviderSessionForkResult> {
    let root = home.join(".claude").join("projects");
    let source_path = find_claude_session_file(&root, parent_session_id)?;
    let destination_path =
        safe_destination_in_source_dir(&root, &source_path, &format!("{child_session_id}.jsonl"))?;
    copy_rewritten_jsonl(&source_path, &destination_path, |value| {
        rewrite_claude_session_value(value, parent_session_id, child_session_id)
    })?;

    Ok(ProviderSessionForkResult {
        session_ref: ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: child_session_id.to_string(),
        },
        source_path,
        destination_path,
    })
}

fn fork_codex_session_under(
    parent_session_id: &str,
    child_session_id: &str,
    home: &Path,
) -> AppResult<ProviderSessionForkResult> {
    let codex_root = home.join(".codex");
    let sessions_root = codex_root.join("sessions");
    let source_path = find_codex_session_file(&sessions_root, parent_session_id)?;
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("jsonl");
    let file_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|name| {
            if name.contains(parent_session_id) {
                name.replace(parent_session_id, child_session_id)
            } else {
                format!("{child_session_id}.{extension}")
            }
        })
        .ok_or_else(|| {
            AppError::Infrastructure(format!(
                "Codex session source has no filename: {}",
                source_path.display()
            ))
        })?;
    let destination_path =
        safe_destination_in_source_dir(&sessions_root, &source_path, &file_name)?;

    copy_rewritten_jsonl(&source_path, &destination_path, |value| {
        rewrite_codex_session_value(value, parent_session_id, child_session_id)
    })?;

    if let Err(error) = append_codex_session_index(&codex_root, parent_session_id, child_session_id)
    {
        let _ = fs::remove_file(&destination_path);
        return Err(error);
    }

    Ok(ProviderSessionForkResult {
        session_ref: ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: child_session_id.to_string(),
        },
        source_path,
        destination_path,
    })
}

fn validate_provider_session_id(session_id: &str) -> AppResult<()> {
    let len = session_id.len();
    if !(1..=128).contains(&len) {
        return Err(AppError::Validation(
            "Provider session id must be 1-128 characters".to_string(),
        ));
    }
    if !session_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::Validation(
            "Provider session id contains unsupported path characters".to_string(),
        ));
    }
    Ok(())
}

fn find_claude_session_file(root: &Path, session_id: &str) -> AppResult<PathBuf> {
    find_session_file(root, |path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name == format!("{session_id}.jsonl"))
    })
    .ok_or_else(|| {
        AppError::NotFound(format!(
            "Claude provider session artifact not found for session {session_id}"
        ))
    })
}

fn find_codex_session_file(root: &Path, session_id: &str) -> AppResult<PathBuf> {
    find_session_file(root, |path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| {
                name.contains(session_id) && (name.ends_with(".jsonl") || name.ends_with(".json"))
            })
    })
    .ok_or_else(|| {
        AppError::NotFound(format!(
            "Codex provider session artifact not found for session {session_id}"
        ))
    })
}

fn find_session_file<F>(root: &Path, matches: F) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let mut candidates = Vec::new();
    collect_session_file_candidates(root, &matches, &mut candidates);
    candidates
        .into_iter()
        .max_by(|left, right| compare_session_candidate(left, right))
        .map(|candidate| candidate.path)
}

#[derive(Debug)]
struct SessionFileCandidate {
    path: PathBuf,
    modified: Option<SystemTime>,
}

fn collect_session_file_candidates<F>(
    dir: &Path,
    matches: &F,
    candidates: &mut Vec<SessionFileCandidate>,
) where
    F: Fn(&Path) -> bool,
{
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_session_file_candidates(&path, matches, candidates);
        } else if file_type.is_file() && matches(&path) {
            candidates.push(SessionFileCandidate {
                path,
                modified: entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok()),
            });
        }
    }
}

fn compare_session_candidate(
    left: &SessionFileCandidate,
    right: &SessionFileCandidate,
) -> std::cmp::Ordering {
    left.modified
        .cmp(&right.modified)
        .then_with(|| left.path.cmp(&right.path))
}

fn safe_destination_in_source_dir(
    root: &Path,
    source_path: &Path,
    destination_file_name: &str,
) -> AppResult<PathBuf> {
    let root = root.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Provider session root is not readable: {}: {error}",
            root.display()
        ))
    })?;
    let source_path = source_path.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Provider session source is not readable: {}: {error}",
            source_path.display()
        ))
    })?;
    if !source_path.starts_with(&root) {
        return Err(AppError::Validation(format!(
            "Provider session source is outside provider root: {}",
            source_path.display()
        )));
    }
    if destination_file_name.contains('/')
        || destination_file_name.contains('\\')
        || destination_file_name == "."
        || destination_file_name == ".."
    {
        return Err(AppError::Validation(
            "Provider session destination filename is unsafe".to_string(),
        ));
    }
    let parent = source_path.parent().ok_or_else(|| {
        AppError::Infrastructure(format!(
            "Provider session source has no parent directory: {}",
            source_path.display()
        ))
    })?;
    let parent = parent.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Provider session source parent is not readable: {}: {error}",
            parent.display()
        ))
    })?;
    if !parent.starts_with(&root) {
        return Err(AppError::Validation(format!(
            "Provider session source parent is outside provider root: {}",
            parent.display()
        )));
    }
    let destination_path = parent.join(destination_file_name);
    if destination_path.exists() {
        return Err(AppError::Conflict(format!(
            "Forked provider session artifact already exists: {}",
            destination_path.display()
        )));
    }
    Ok(destination_path)
}

fn copy_rewritten_jsonl<F>(
    source_path: &Path,
    destination_path: &Path,
    mut rewrite: F,
) -> AppResult<()>
where
    F: FnMut(&mut Value),
{
    let input = fs::read_to_string(source_path).map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to read provider session artifact {}: {error}",
            source_path.display()
        ))
    })?;
    let output = rewrite_jsonl(&input, &mut rewrite);
    let temp_path = destination_path.with_extension(format!(
        "{}.tmp",
        destination_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("jsonl")
    ));
    fs::write(&temp_path, output).map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to write forked provider session artifact {}: {error}",
            temp_path.display()
        ))
    })?;
    fs::rename(&temp_path, destination_path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        AppError::Infrastructure(format!(
            "Failed to finalize forked provider session artifact {}: {error}",
            destination_path.display()
        ))
    })?;
    Ok(())
}

fn rewrite_jsonl<F>(input: &str, rewrite: &mut F) -> String
where
    F: FnMut(&mut Value),
{
    let mut lines = Vec::new();
    for line in input.lines() {
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            lines.push(line.to_string());
            continue;
        };
        rewrite(&mut value);
        lines.push(value.to_string());
    }

    let mut output = lines.join("\n");
    if input.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn rewrite_claude_session_value(
    value: &mut Value,
    parent_session_id: &str,
    child_session_id: &str,
) {
    rewrite_session_id_fields(value, parent_session_id, child_session_id);
}

fn rewrite_codex_session_value(value: &mut Value, parent_session_id: &str, child_session_id: &str) {
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "session_meta")
    {
        if let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) {
            replace_matching_string_field(payload, "id", parent_session_id, child_session_id);
        }
    }
    rewrite_session_id_fields(value, parent_session_id, child_session_id);
}

fn rewrite_session_id_fields(value: &mut Value, parent_session_id: &str, child_session_id: &str) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map.iter_mut() {
                if matches!(
                    key.as_str(),
                    "sessionId" | "session_id" | "providerSessionId" | "provider_session_id"
                ) {
                    if nested.as_str() == Some(parent_session_id) {
                        *nested = Value::String(child_session_id.to_string());
                    }
                } else {
                    rewrite_session_id_fields(nested, parent_session_id, child_session_id);
                }
            }
        }
        Value::Array(values) => {
            for nested in values {
                rewrite_session_id_fields(nested, parent_session_id, child_session_id);
            }
        }
        _ => {}
    }
}

fn replace_matching_string_field(
    map: &mut serde_json::Map<String, Value>,
    key: &str,
    old_value: &str,
    new_value: &str,
) {
    if let Some(value) = map.get_mut(key) {
        if value.as_str() == Some(old_value) {
            *value = Value::String(new_value.to_string());
        }
    }
}

fn append_codex_session_index(
    codex_root: &Path,
    parent_session_id: &str,
    child_session_id: &str,
) -> AppResult<()> {
    let index_path = codex_root.join("session_index.jsonl");
    let input = if index_path.exists() {
        fs::read_to_string(&index_path).map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to read Codex session index {}: {error}",
                index_path.display()
            ))
        })?
    } else {
        String::new()
    };

    let mut child_line = None;
    for line in input.lines() {
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = value.get("id").and_then(Value::as_str);
        if id == Some(child_session_id) {
            return Err(AppError::Conflict(format!(
                "Codex session index already contains fork session {child_session_id}"
            )));
        }
        if id == Some(parent_session_id) && child_line.is_none() {
            if let Some(map) = value.as_object_mut() {
                map.insert(
                    "id".to_string(),
                    Value::String(child_session_id.to_string()),
                );
                let now = Utc::now().to_rfc3339();
                if map.contains_key("updated_at") {
                    map.insert("updated_at".to_string(), Value::String(now));
                }
            }
            child_line = Some(value.to_string());
        }
    }

    let child_line = child_line.unwrap_or_else(|| {
        serde_json::json!({
            "id": child_session_id,
            "updated_at": Utc::now().to_rfc3339(),
        })
        .to_string()
    });

    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to create Codex session index directory {}: {error}",
                parent.display()
            ))
        })?;
    }

    let mut output = input;
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&child_line);
    output.push('\n');
    fs::write(&index_path, output).map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to write Codex session index {}: {error}",
            index_path.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp home")
    }

    #[test]
    fn copies_claude_session_with_rewritten_session_id() {
        let home = temp_home();
        let source_dir = home.path().join(".claude/projects/project-a");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("parent-session.jsonl"),
            concat!(
                "{\"sessionId\":\"parent-session\",\"message\":{\"content\":\"keep parent-session in text\"}}\n",
                "not-json\n"
            ),
        )
        .expect("write source");

        let result = fork_provider_session_under(
            AgentHarnessKind::Claude,
            "parent-session",
            "child-session",
            home.path(),
        )
        .expect("fork claude session");

        assert_eq!(result.session_ref.provider_session_id, "child-session");
        let copied = fs::read_to_string(source_dir.join("child-session.jsonl"))
            .expect("read copied session");
        assert!(copied.contains("\"sessionId\":\"child-session\""));
        assert!(copied.contains("keep parent-session in text"));
        assert!(copied.contains("not-json"));
    }

    #[test]
    fn copies_codex_session_and_updates_index() {
        let home = temp_home();
        let source_dir = home.path().join(".codex/sessions/2026/05/21");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::create_dir_all(home.path().join(".codex")).expect("create codex root");
        fs::write(
            source_dir.join("rollout-parent-session.jsonl"),
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"parent-session\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"session_id\":\"parent-session\",\"text\":\"keep parent-session text\"}}\n"
            ),
        )
        .expect("write source");
        fs::write(
            home.path().join(".codex/session_index.jsonl"),
            "{\"id\":\"parent-session\",\"thread_name\":\"Old title\",\"updated_at\":\"2026-01-01T00:00:00Z\"}\n",
        )
        .expect("write index");

        let result = fork_provider_session_under(
            AgentHarnessKind::Codex,
            "parent-session",
            "child-session",
            home.path(),
        )
        .expect("fork codex session");

        assert_eq!(result.session_ref.provider_session_id, "child-session");
        let copied = fs::read_to_string(source_dir.join("rollout-child-session.jsonl"))
            .expect("read copied session");
        assert!(copied.contains("\"id\":\"child-session\""));
        assert!(copied.contains("\"session_id\":\"child-session\""));
        assert!(copied.contains("keep parent-session text"));

        let index =
            fs::read_to_string(home.path().join(".codex/session_index.jsonl")).expect("read index");
        assert!(index.contains("\"id\":\"parent-session\""));
        assert!(index.contains("\"id\":\"child-session\""));
        assert!(index.contains("\"thread_name\":\"Old title\""));
    }

    #[test]
    fn rejects_unsafe_session_ids_before_filesystem_writes() {
        let home = temp_home();
        let error = fork_provider_session_under(
            AgentHarnessKind::Codex,
            "../parent",
            "child-session",
            home.path(),
        )
        .expect_err("unsafe parent id should fail");

        assert!(matches!(error, AppError::Validation(_)));
    }
}
