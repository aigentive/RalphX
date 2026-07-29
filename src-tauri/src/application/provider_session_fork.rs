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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSessionForkTarget {
    pub working_directory: PathBuf,
    pub git_branch: Option<String>,
}

pub fn fork_provider_session_from_state_home(
    parent_ref: &ProviderSessionRef,
) -> AppResult<ProviderSessionForkResult> {
    fork_provider_session_from_state_home_for_target(parent_ref, None)
}

pub fn fork_provider_session_from_state_home_for_target(
    parent_ref: &ProviderSessionRef,
    target: Option<&ProviderSessionForkTarget>,
) -> AppResult<ProviderSessionForkResult> {
    let child_session_id = Uuid::new_v4().to_string();
    let home = provider_state_home_dir()?;
    fork_provider_session_under_for_target(
        parent_ref.harness,
        &parent_ref.provider_session_id,
        &child_session_id,
        &home,
        target,
    )
}

pub fn fork_provider_session_under(
    harness: AgentHarnessKind,
    parent_session_id: &str,
    child_session_id: &str,
    home: &Path,
) -> AppResult<ProviderSessionForkResult> {
    fork_provider_session_under_for_target(harness, parent_session_id, child_session_id, home, None)
}

pub fn fork_provider_session_under_for_target(
    harness: AgentHarnessKind,
    parent_session_id: &str,
    child_session_id: &str,
    home: &Path,
    target: Option<&ProviderSessionForkTarget>,
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
            fork_claude_session_under(parent_session_id, child_session_id, home, target)
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
    target: Option<&ProviderSessionForkTarget>,
) -> AppResult<ProviderSessionForkResult> {
    let root = home.join(".claude").join("projects");
    let source_path = find_claude_session_file(&root, parent_session_id)?;
    let destination_file_name = format!("{child_session_id}.jsonl");
    let destination_path = if let Some(target) = target {
        safe_destination_in_claude_project_dir(&root, target, &destination_file_name)?
    } else {
        safe_destination_in_source_dir(&root, &source_path, &destination_file_name)?
    };
    copy_rewritten_jsonl(&source_path, &destination_path, |value| {
        rewrite_claude_session_value(value, parent_session_id, child_session_id, target)
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
    validate_safe_path_component(
        destination_file_name,
        "Provider session destination filename",
    )?;
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

fn safe_destination_in_claude_project_dir(
    root: &Path,
    target: &ProviderSessionForkTarget,
    destination_file_name: &str,
) -> AppResult<PathBuf> {
    let root = root.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Provider session root is not readable: {}: {error}",
            root.display()
        ))
    })?;
    validate_safe_path_component(
        destination_file_name,
        "Provider session destination filename",
    )?;

    let project_dir_name = claude_project_dir_name_for_cwd(&target.working_directory)?;
    validate_safe_path_component(&project_dir_name, "Claude project directory name")?;
    let destination_parent = root.join(project_dir_name);
    let destination_parent = if destination_parent.exists() {
        let canonical_parent = destination_parent.canonicalize().map_err(|error| {
            AppError::Infrastructure(format!(
                "Claude project directory is not readable: {}: {error}",
                destination_parent.display()
            ))
        })?;
        if !canonical_parent.starts_with(&root) {
            return Err(AppError::Validation(format!(
                "Claude project directory is outside provider root: {}",
                canonical_parent.display()
            )));
        }
        canonical_parent
    } else {
        destination_parent
    };

    let destination_path = destination_parent.join(destination_file_name);
    if destination_path.exists() {
        return Err(AppError::Conflict(format!(
            "Forked provider session artifact already exists: {}",
            destination_path.display()
        )));
    }
    Ok(destination_path)
}

fn validate_safe_path_component(value: &str, label: &str) -> AppResult<()> {
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value == "."
        || value == ".."
    {
        return Err(AppError::Validation(format!("{label} is unsafe")));
    }
    Ok(())
}

fn claude_project_dir_name_for_cwd(working_directory: &Path) -> AppResult<String> {
    let value = working_directory.to_string_lossy();
    if value.trim().is_empty() {
        return Err(AppError::Validation(
            "Claude project working directory is empty".to_string(),
        ));
    }
    Ok(value.replace(['/', '\\'], "-"))
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
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to create forked provider session directory {}: {error}",
                parent.display()
            ))
        })?;
    }
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
    target: Option<&ProviderSessionForkTarget>,
) {
    rewrite_session_id_fields(value, parent_session_id, child_session_id);
    if let Some(target) = target {
        rewrite_claude_workspace_fields(value, target);
    }
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

fn rewrite_claude_workspace_fields(value: &mut Value, target: &ProviderSessionForkTarget) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map.iter_mut() {
                match key.as_str() {
                    "cwd" if nested.is_string() => {
                        *nested = Value::String(target.working_directory.display().to_string());
                    }
                    "gitBranch" if nested.is_string() => {
                        if let Some(git_branch) = target.git_branch.as_ref() {
                            *nested = Value::String(git_branch.clone());
                        }
                    }
                    _ => rewrite_claude_workspace_fields(nested, target),
                }
            }
        }
        Value::Array(values) => {
            for nested in values {
                rewrite_claude_workspace_fields(nested, target);
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
    use std::ffi::OsString;
    use std::sync::Mutex;
    use std::time::{Duration, UNIX_EPOCH};

    static PROVIDER_STATE_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvOverrideGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvOverrideGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvOverrideGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

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
    fn forks_codex_session_from_provider_state_home_override() {
        let _lock = PROVIDER_STATE_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temp_home();
        let _guard = EnvOverrideGuard::set("RALPHX_PROVIDER_STATE_HOME_OVERRIDE", home.path());
        let source_dir = home.path().join(".codex/sessions/2026/05/22");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("rollout-parent-session.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"parent-session\"}}\n",
        )
        .expect("write source");

        let result = fork_provider_session_from_state_home(&ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "parent-session".to_string(),
        })
        .expect("fork codex session from override home");

        assert_eq!(result.session_ref.harness, AgentHarnessKind::Codex);
        assert_ne!(result.session_ref.provider_session_id, "parent-session");
        assert!(result.destination_path.exists());
        assert!(
            result
                .destination_path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.contains(&result.session_ref.provider_session_id))
        );
        let copied = fs::read_to_string(&result.destination_path).expect("read copied session");
        assert!(copied.contains(&result.session_ref.provider_session_id));
    }

    #[test]
    fn forks_claude_session_from_state_home_override_into_target_project_dir() {
        let _lock = PROVIDER_STATE_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temp_home();
        let override_value = format!("  {}  ", home.path().display());
        let _guard = EnvOverrideGuard::set("RALPHX_PROVIDER_STATE_HOME_OVERRIDE", override_value);
        let source_dir = home.path().join(".claude/projects/source-project");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("parent-session.jsonl"),
            serde_json::json!({
                "sessionId": "parent-session",
                "cwd": "/tmp/parent-worktree",
                "gitBranch": "parent-branch"
            })
            .to_string(),
        )
        .expect("write source");
        let child_worktree = home.path().join("child-worktree");
        let target = ProviderSessionForkTarget {
            working_directory: child_worktree.clone(),
            git_branch: Some("child-branch".to_string()),
        };

        let result = fork_provider_session_from_state_home_for_target(
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "parent-session".to_string(),
            },
            Some(&target),
        )
        .expect("fork claude session into target project dir");

        let target_dir = home
            .path()
            .join(".claude/projects")
            .canonicalize()
            .expect("canonical claude projects root")
            .join(claude_project_dir_name_for_cwd(&child_worktree).expect("encoded child cwd"));
        assert_eq!(result.destination_path.parent(), Some(target_dir.as_path()));
        let copied = fs::read_to_string(&result.destination_path).expect("read copied session");
        assert!(copied.contains(&result.session_ref.provider_session_id));
        assert!(copied.contains(&child_worktree.display().to_string()));
        assert!(copied.contains("child-branch"));
    }

    #[test]
    fn copies_claude_session_to_target_project_dir_and_rewrites_workspace_fields() {
        let home = temp_home();
        let source_dir = home.path().join(".claude/projects/parent-project");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("parent-session.jsonl"),
            serde_json::json!({
                "sessionId": "parent-session",
                "cwd": "/tmp/parent-worktree",
                "gitBranch": "parent-branch",
                "nested": {
                    "session_id": "parent-session",
                    "cwd": "/tmp/parent-worktree",
                    "gitBranch": "parent-branch"
                }
            })
            .to_string(),
        )
        .expect("write source");

        let child_worktree = home.path().join("worktrees/child-worktree");
        let target = ProviderSessionForkTarget {
            working_directory: child_worktree.clone(),
            git_branch: Some("ralphx/child-branch".to_string()),
        };

        let result = fork_provider_session_under_for_target(
            AgentHarnessKind::Claude,
            "parent-session",
            "child-session",
            home.path(),
            Some(&target),
        )
        .expect("fork claude session");

        let target_project_dir = home
            .path()
            .join(".claude/projects")
            .canonicalize()
            .expect("canonical claude projects root")
            .join(claude_project_dir_name_for_cwd(&child_worktree).expect("encoded child cwd"));
        let copied_path = target_project_dir.join("child-session.jsonl");
        assert_eq!(result.destination_path, copied_path);
        assert!(!source_dir.join("child-session.jsonl").exists());

        let copied = fs::read_to_string(copied_path).expect("read copied session");
        let copied: Value = serde_json::from_str(&copied).expect("copied json line");
        assert_eq!(copied["sessionId"], "child-session");
        assert_eq!(copied["cwd"], child_worktree.display().to_string());
        assert_eq!(copied["gitBranch"], "ralphx/child-branch");
        assert_eq!(copied["nested"]["session_id"], "child-session");
        assert_eq!(
            copied["nested"]["cwd"],
            child_worktree.display().to_string()
        );
        assert_eq!(copied["nested"]["gitBranch"], "ralphx/child-branch");
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
    fn copies_codex_json_session_and_keeps_json_extension() {
        let home = temp_home();
        let source_dir = home.path().join(".codex/sessions");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("rollout-parent-session.json"),
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"parent-session\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"session_id\":\"parent-session\"}}\n"
            ),
        )
        .expect("write source");

        let result = fork_provider_session_under(
            AgentHarnessKind::Codex,
            "parent-session",
            "child-session",
            home.path(),
        )
        .expect("fork codex json session");

        assert_eq!(
            result.destination_path,
            source_dir
                .canonicalize()
                .expect("canonical codex session dir")
                .join("rollout-child-session.json")
        );
        let copied =
            fs::read_to_string(source_dir.join("rollout-child-session.json")).expect("read copy");
        assert!(copied.contains("\"id\":\"child-session\""));
        assert!(copied.contains("\"session_id\":\"child-session\""));
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

    #[test]
    fn rejects_matching_parent_and_child_session_ids() {
        let home = temp_home();
        let error = fork_provider_session_under(
            AgentHarnessKind::Claude,
            "same-session",
            "same-session",
            home.path(),
        )
        .expect_err("matching ids should fail");

        assert!(matches!(error, AppError::Validation(_)));
    }

    #[test]
    fn creates_minimal_codex_index_entry_when_parent_index_is_missing() {
        let home = temp_home();
        let source_dir = home.path().join(".codex/sessions");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("rollout-parent-session.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"parent-session\"}}\n",
        )
        .expect("write source");

        fork_provider_session_under(
            AgentHarnessKind::Codex,
            "parent-session",
            "child-session",
            home.path(),
        )
        .expect("fork codex session");

        let index =
            fs::read_to_string(home.path().join(".codex/session_index.jsonl")).expect("read index");
        assert!(index.contains("\"id\":\"child-session\""));
        assert!(index.contains("\"updated_at\""));
    }

    #[test]
    fn appends_codex_index_child_after_parent_without_trailing_newline() {
        let home = temp_home();
        let source_dir = home.path().join(".codex/sessions");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("rollout-parent-session.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"parent-session\"}}\n",
        )
        .expect("write source");
        fs::write(
            home.path().join(".codex/session_index.jsonl"),
            "{\"id\":\"parent-session\",\"updated_at\":\"2026-01-01T00:00:00Z\"}",
        )
        .expect("write index");

        fork_provider_session_under(
            AgentHarnessKind::Codex,
            "parent-session",
            "child-session",
            home.path(),
        )
        .expect("fork codex session");

        let index =
            fs::read_to_string(home.path().join(".codex/session_index.jsonl")).expect("read index");
        let lines = index.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"id\":\"parent-session\""));
        assert!(lines[1].contains("\"id\":\"child-session\""));
        assert!(index.ends_with('\n'));
    }

    #[test]
    fn rolls_back_codex_session_copy_when_index_already_has_child() {
        let home = temp_home();
        let source_dir = home.path().join(".codex/sessions");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("rollout-parent-session.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"parent-session\"}}\n",
        )
        .expect("write source");
        fs::write(
            home.path().join(".codex/session_index.jsonl"),
            concat!(
                "{\"id\":\"parent-session\",\"updated_at\":\"2026-01-01T00:00:00Z\"}\n",
                "{\"id\":\"child-session\",\"updated_at\":\"2026-01-02T00:00:00Z\"}\n"
            ),
        )
        .expect("write index");

        let error = fork_provider_session_under(
            AgentHarnessKind::Codex,
            "parent-session",
            "child-session",
            home.path(),
        )
        .expect_err("existing child index entry should fail");

        assert!(matches!(error, AppError::Conflict(_)));
        assert!(!source_dir.join("rollout-child-session.jsonl").exists());
    }

    #[test]
    fn rejects_empty_claude_target_working_directory() {
        let home = temp_home();
        let source_dir = home.path().join(".claude/projects/project-a");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(source_dir.join("parent-session.jsonl"), "{}\n").expect("write source");
        let target = ProviderSessionForkTarget {
            working_directory: PathBuf::from(""),
            git_branch: Some("main".to_string()),
        };

        let error = fork_provider_session_under_for_target(
            AgentHarnessKind::Claude,
            "parent-session",
            "child-session",
            home.path(),
            Some(&target),
        )
        .expect_err("empty target cwd should fail");

        assert!(matches!(error, AppError::Validation(_)));
    }

    #[test]
    fn rejects_existing_claude_target_destination() {
        let home = temp_home();
        let source_dir = home.path().join(".claude/projects/project-a");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(source_dir.join("parent-session.jsonl"), "{}\n").expect("write source");
        let child_worktree = home.path().join("worktrees/child-worktree");
        let target_dir = home
            .path()
            .join(".claude/projects")
            .join(claude_project_dir_name_for_cwd(&child_worktree).expect("encoded cwd"));
        fs::create_dir_all(&target_dir).expect("create target dir");
        fs::write(target_dir.join("child-session.jsonl"), "existing\n")
            .expect("write existing child");
        let target = ProviderSessionForkTarget {
            working_directory: child_worktree,
            git_branch: None,
        };

        let error = fork_provider_session_under_for_target(
            AgentHarnessKind::Claude,
            "parent-session",
            "child-session",
            home.path(),
            Some(&target),
        )
        .expect_err("existing child artifact should fail");

        assert!(matches!(error, AppError::Conflict(_)));
    }

    #[test]
    fn rewrite_jsonl_rewrites_valid_lines_and_preserves_invalid_lines() {
        let input = concat!(
            "{\"sessionId\":\"parent-session\"}\n",
            "not-json\n",
            "{\"provider_session_id\":\"parent-session\"}\n"
        );

        let output = rewrite_jsonl(input, &mut |value| {
            rewrite_session_id_fields(value, "parent-session", "child-session")
        });

        assert!(output.contains("\"sessionId\":\"child-session\""));
        assert!(output.contains("not-json"));
        assert!(output.contains("\"provider_session_id\":\"child-session\""));
        assert!(output.ends_with('\n'));
    }

    #[test]
    fn compare_session_candidate_prefers_modified_time_then_path() {
        let older = SessionFileCandidate {
            path: PathBuf::from("z.jsonl"),
            modified: Some(UNIX_EPOCH),
        };
        let newer = SessionFileCandidate {
            path: PathBuf::from("a.jsonl"),
            modified: Some(UNIX_EPOCH + Duration::from_secs(1)),
        };
        assert_eq!(
            compare_session_candidate(&newer, &older),
            std::cmp::Ordering::Greater
        );

        let left = SessionFileCandidate {
            path: PathBuf::from("b.jsonl"),
            modified: Some(UNIX_EPOCH),
        };
        let right = SessionFileCandidate {
            path: PathBuf::from("a.jsonl"),
            modified: Some(UNIX_EPOCH),
        };
        assert_eq!(
            compare_session_candidate(&left, &right),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn safe_destination_rejects_source_outside_provider_root() {
        let home = temp_home();
        let root = home.path().join(".codex/sessions");
        let outside = home.path().join("outside");
        fs::create_dir_all(&root).expect("create provider root");
        fs::create_dir_all(&outside).expect("create outside dir");
        let source_path = outside.join("parent-session.jsonl");
        fs::write(&source_path, "{}\n").expect("write outside source");

        let error = safe_destination_in_source_dir(&root, &source_path, "child-session.jsonl")
            .expect_err("source outside root should fail");

        assert!(matches!(error, AppError::Validation(_)));
    }

    #[test]
    fn safe_destination_rejects_unsafe_destination_filename() {
        let home = temp_home();
        let root = home.path().join(".codex/sessions");
        fs::create_dir_all(&root).expect("create provider root");
        let source_path = root.join("parent-session.jsonl");
        fs::write(&source_path, "{}\n").expect("write source");

        let error = safe_destination_in_source_dir(&root, &source_path, "../child-session.jsonl")
            .expect_err("unsafe destination filename should fail");

        assert!(matches!(error, AppError::Validation(_)));
    }

    #[test]
    fn rewrite_session_id_fields_rewrites_nested_provider_fields() {
        let mut value = serde_json::json!({
            "sessionId": "parent-session",
            "session_id": "other-session",
            "nested": [
                { "providerSessionId": "parent-session" },
                { "provider_session_id": "parent-session" },
                { "text": "keep parent-session in prose" }
            ]
        });

        rewrite_session_id_fields(&mut value, "parent-session", "child-session");

        assert_eq!(value["sessionId"], "child-session");
        assert_eq!(value["session_id"], "other-session");
        assert_eq!(value["nested"][0]["providerSessionId"], "child-session");
        assert_eq!(value["nested"][1]["provider_session_id"], "child-session");
        assert_eq!(value["nested"][2]["text"], "keep parent-session in prose");
    }

    #[test]
    fn rewrite_claude_workspace_fields_preserves_branch_when_target_branch_absent() {
        let home = temp_home();
        let child_worktree = home.path().join("child-worktree");
        let target = ProviderSessionForkTarget {
            working_directory: child_worktree.clone(),
            git_branch: None,
        };
        let mut value = serde_json::json!({
            "cwd": "/tmp/parent-worktree",
            "gitBranch": "parent-branch",
            "nested": {
                "cwd": "/tmp/parent-worktree",
                "gitBranch": "parent-branch"
            }
        });

        rewrite_claude_workspace_fields(&mut value, &target);

        assert_eq!(value["cwd"], child_worktree.display().to_string());
        assert_eq!(value["gitBranch"], "parent-branch");
        assert_eq!(value["nested"]["cwd"], child_worktree.display().to_string());
        assert_eq!(value["nested"]["gitBranch"], "parent-branch");
    }
}
