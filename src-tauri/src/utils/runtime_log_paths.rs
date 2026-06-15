use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// RalphX-owned runtime data directory for generated logs and artifacts.
///
/// Dev builds keep runtime output in the source checkout `.artifacts`; release
/// builds keep it under the platform application data directory. Target project
/// worktrees must never be used as a fallback for RalphX runtime output.
pub fn app_runtime_dir() -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
            .join(".artifacts")
    } else {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("com.ralphx.app")
    }
}

/// RalphX-owned log directory for backend/runtime logs.
pub fn app_log_dir() -> PathBuf {
    app_runtime_dir().join("logs")
}

/// RalphX-owned directory for generated non-log artifacts.
pub fn app_artifact_dir() -> PathBuf {
    app_runtime_dir().join("artifacts")
}

/// RalphX-owned root for managed provider CLI installs.
pub fn managed_provider_cli_dir() -> PathBuf {
    app_runtime_dir().join("managed-cli")
}

/// RalphX-owned visible Codex binary directory used by the standalone installer.
pub fn managed_codex_bin_dir() -> PathBuf {
    managed_provider_cli_dir().join("codex").join("bin")
}

/// RalphX-owned Codex state/package root used by the standalone installer.
pub fn managed_codex_home_dir() -> PathBuf {
    managed_provider_cli_dir().join("codex").join("home")
}

/// RalphX-owned HOME value for the Codex installer process.
pub fn managed_codex_installer_home_dir() -> PathBuf {
    managed_provider_cli_dir()
        .join("codex")
        .join("installer-home")
}

pub fn managed_codex_binary_path() -> PathBuf {
    managed_codex_bin_dir().join(managed_codex_binary_name())
}

fn managed_codex_binary_name() -> &'static str {
    if cfg!(windows) {
        "codex.exe"
    } else {
        "codex"
    }
}

/// RalphX-owned directory for MCP proxy JSONL trace files.
pub fn mcp_proxy_trace_dir() -> PathBuf {
    app_log_dir().join("mcp-proxy")
}

pub fn ensure_mcp_proxy_trace_dir() -> PathBuf {
    let dir = mcp_proxy_trace_dir();
    // dir is RalphX-owned app runtime storage, never target-project input.
    // codeql[rust/path-injection]
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn claude_debug_log_dir() -> PathBuf {
    app_log_dir().join("claude-debug")
}

pub fn claude_debug_log_file() -> PathBuf {
    claude_debug_log_dir().join(format!(
        "ralphx-claude-debug-{}-{}.log",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ))
}

pub fn codex_prompt_debug_dir() -> PathBuf {
    app_log_dir().join("codex-prompts")
}

pub fn agent_screenshot_dir() -> PathBuf {
    app_artifact_dir().join("screenshots")
}

pub fn memory_archive_dir() -> PathBuf {
    app_artifact_dir().join("memory-archive")
}

pub fn memory_archive_project_dir(project_id: &str) -> PathBuf {
    memory_archive_dir().join(memory_archive_project_relative_dir(project_id))
}

pub fn memory_archive_project_relative_dir(project_id: &str) -> PathBuf {
    PathBuf::from(hashed_log_component("project", project_id))
}

pub fn memory_archive_memory_snapshot_file(project_id: &str, memory_id: &str) -> PathBuf {
    memory_archive_dir().join(memory_archive_memory_snapshot_relative_file(
        project_id, memory_id,
    ))
}

pub fn memory_archive_memory_snapshot_relative_file(project_id: &str, memory_id: &str) -> PathBuf {
    memory_archive_project_relative_dir(project_id)
        .join("memories")
        .join(format!("{}.md", hashed_log_component("memory", memory_id)))
}

pub fn memory_archive_rule_snapshot_file(
    project_id: &str,
    scope_key: &str,
    timestamp: &str,
) -> PathBuf {
    memory_archive_dir().join(memory_archive_rule_snapshot_relative_file(
        project_id, scope_key, timestamp,
    ))
}

pub fn memory_archive_rule_snapshot_relative_file(
    project_id: &str,
    scope_key: &str,
    timestamp: &str,
) -> PathBuf {
    memory_archive_project_relative_dir(project_id)
        .join("rules")
        .join(hashed_log_component("rule", scope_key))
        .join(format!("{}.md", fixed_timestamp_component(timestamp)))
}

pub fn memory_archive_project_snapshot_file(project_id: &str, timestamp: &str) -> PathBuf {
    memory_archive_dir().join(memory_archive_project_snapshot_relative_file(
        project_id, timestamp,
    ))
}

pub fn memory_archive_project_snapshot_relative_file(project_id: &str, timestamp: &str) -> PathBuf {
    memory_archive_project_relative_dir(project_id)
        .join("projects")
        .join(format!("{}.md", fixed_timestamp_component(timestamp)))
}

pub fn merge_validation_log_dir(task_id: &str) -> PathBuf {
    app_log_dir()
        .join("merge-validation")
        .join(hashed_log_component("task", task_id))
}

pub fn stream_debug_log_file(conversation_id: &str) -> PathBuf {
    app_log_dir().join("stream-debug").join(format!(
        "{}.log",
        hashed_log_component("conversation", conversation_id)
    ))
}

pub fn codex_prompt_debug_file(mode: &str) -> PathBuf {
    let mode = match mode {
        "exec" => "exec",
        "resume" => "resume",
        _ => "unknown",
    };
    app_log_dir().join("codex-prompts").join(format!(
        "{}-{}-{}.txt",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        mode,
        uuid::Uuid::new_v4()
    ))
}

fn hashed_log_component(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(24);
    for byte in &digest[..12] {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    format!("{prefix}-{encoded}")
}

fn fixed_timestamp_component(timestamp: &str) -> &str {
    if timestamp
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'_' || byte == b'T' || byte == b'Z')
    {
        timestamp
    } else {
        "unknown-timestamp"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_validation_log_dir_hashes_task_id_components() {
        let path = merge_validation_log_dir("../task/with\\separators");
        let suffix = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("log dir suffix");

        assert!(path.starts_with(app_log_dir().join("merge-validation")));
        assert!(suffix.starts_with("task-"));
        assert!(!suffix.contains(".."));
        assert!(!suffix.contains('/'));
        assert!(!suffix.contains('\\'));
    }

    #[test]
    fn codex_prompt_debug_file_maps_unknown_modes_to_fixed_component() {
        let path = codex_prompt_debug_file("../resume");
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("prompt debug filename");

        assert!(path.starts_with(codex_prompt_debug_dir()));
        assert!(filename.contains("-unknown-"));
        assert!(!filename.contains(".."));
        assert!(!filename.contains('/'));
        assert!(!filename.contains('\\'));
    }

    #[test]
    fn stream_debug_log_file_hashes_conversation_id() {
        let path = stream_debug_log_file("../conversation/with\\separators");
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("stream debug filename");

        assert!(path.starts_with(app_log_dir().join("stream-debug")));
        assert!(filename.starts_with("conversation-"));
        assert!(filename.ends_with(".log"));
        assert!(!filename.contains(".."));
        assert!(!filename.contains('/'));
        assert!(!filename.contains('\\'));
    }

    #[test]
    fn ensure_mcp_proxy_trace_dir_creates_app_owned_dir() {
        let dir = ensure_mcp_proxy_trace_dir();

        assert!(dir.starts_with(app_log_dir()));
        assert_eq!(
            dir.file_name().and_then(|value| value.to_str()),
            Some("mcp-proxy")
        );
        assert!(dir.is_dir());
    }

    #[test]
    fn memory_archive_paths_hash_runtime_components() {
        let project_dir = memory_archive_project_dir("../project/with\\separators");
        let project_relative_dir =
            memory_archive_project_relative_dir("../project/with\\separators");
        let memory_path = memory_archive_memory_snapshot_file(
            "../project/with\\separators",
            "../memory/unsafe.md",
        );
        let memory_relative_path = memory_archive_memory_snapshot_relative_file(
            "../project/with\\separators",
            "../memory/unsafe.md",
        );
        let rule_path = memory_archive_rule_snapshot_file(
            "../project/with\\separators",
            "../rules/unsafe.md",
            "20260516_120000",
        );
        let project_path =
            memory_archive_project_snapshot_file("../project/with\\separators", "../bad");
        let project_relative_path =
            memory_archive_project_snapshot_relative_file("../project/with\\separators", "../bad");

        for path in [&project_dir, &memory_path, &rule_path, &project_path] {
            let rendered = path.to_string_lossy();
            assert!(path.starts_with(memory_archive_dir()));
            assert!(!rendered.contains("../project"));
            assert!(!rendered.contains("../memory"));
            assert!(!rendered.contains("../rules"));
            assert!(!rendered.contains("../bad"));
        }

        assert_eq!(project_dir, memory_archive_dir().join(project_relative_dir));
        assert_eq!(memory_path, memory_archive_dir().join(memory_relative_path));
        assert_eq!(
            project_path,
            memory_archive_dir().join(project_relative_path)
        );
        assert!(rule_path.ends_with("20260516_120000.md"));
        assert!(project_path.ends_with("unknown-timestamp.md"));
    }
}
