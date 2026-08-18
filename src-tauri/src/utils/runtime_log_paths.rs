use std::collections::BTreeMap;
use std::path::PathBuf;

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

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
        app_data_dir()
    }
}

/// RalphX-owned app data directory for durable runtime-managed state.
pub fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("com.ralphx.app")
}

/// RalphX-owned log directory for backend/runtime logs.
pub fn app_log_dir() -> PathBuf {
    app_runtime_dir().join("logs")
}

/// Removes old per-launch RalphX logs, retaining the current launch and the
/// newest `keep_previous` previous launches. Retention counts launches, not
/// files: a launch that rotated owns both its active log and its `_rolled`
/// chunk, and those are kept or deleted together. The directory is
/// caller-supplied only from the fixed RalphX runtime-root helper.
pub fn cleanup_previous_launch_logs(
    log_dir: &std::path::Path,
    current_filename: &str,
    keep_previous: usize,
) -> Vec<std::io::Error> {
    let entries = match std::fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(error) => return vec![error],
    };
    let mut errors = Vec::new();
    let mut previous_launches: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let current_group_key = launch_log_group_key(current_filename);

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let filename = match entry.file_name().into_string() {
            Ok(filename) if is_launch_log_filename(&filename) => filename,
            _ => continue,
        };
        if filename == current_filename {
            continue;
        }
        let Some(group_key) = launch_log_group_key(&filename) else {
            continue;
        };
        // The current launch keeps every chunk it owns, including a `_rolled`
        // file that a file-level comparison would have treated as "previous".
        if current_group_key.as_deref() == Some(group_key.as_str()) {
            continue;
        }
        match entry.file_type() {
            Ok(file_type) if file_type.is_file() => previous_launches
                .entry(group_key)
                .or_default()
                .push(entry.path()),
            Ok(_) => {}
            Err(error) => errors.push(error),
        }
    }

    let mut group_keys: Vec<String> = previous_launches.keys().cloned().collect();
    group_keys.sort_unstable_by(|left, right| right.cmp(left));
    for group_key in group_keys.into_iter().skip(keep_previous) {
        for path in previous_launches.remove(&group_key).unwrap_or_default() {
            // `path` is a regular file enumerated from the fixed RalphX-owned log directory,
            // and its filename passed the exact ralphx_*.log allowlist above.
            // codeql[rust/path-injection]
            if let Err(error) = std::fs::remove_file(path) {
                errors.push(error);
            }
        }
    }

    errors
}

fn is_launch_log_filename(filename: &str) -> bool {
    filename
        .strip_prefix("ralphx_")
        .and_then(|suffix| suffix.strip_suffix(".log"))
        .is_some_and(|middle| !middle.is_empty())
}

/// Maps a launch log filename to the launch it belongs to, so an active log and
/// its rotated `_rolled` chunk share one retention group. Returns `None` for
/// names outside the `ralphx_*.log` allowlist.
fn launch_log_group_key(filename: &str) -> Option<String> {
    let middle = filename
        .strip_prefix("ralphx_")
        .and_then(|suffix| suffix.strip_suffix(".log"))
        .filter(|middle| !middle.is_empty())?;
    // A name that is *only* the rolled suffix has no base launch to join, so it
    // stays its own group rather than collapsing into an empty key.
    let base = middle
        .strip_suffix("_rolled")
        .filter(|base| !base.is_empty())
        .unwrap_or(middle);
    Some(format!("ralphx_{base}.log"))
}

/// RalphX-owned directory for generated non-log artifacts.
pub fn app_artifact_dir() -> PathBuf {
    app_runtime_dir().join("artifacts")
}

/// RalphX-owned root for managed provider CLI installs.
pub fn managed_provider_cli_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = managed_provider_cli_dir_override()
        .lock()
        .expect("managed provider CLI dir override mutex")
        .clone()
    {
        return path;
    }

    app_data_dir().join("managed-cli")
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

#[cfg(test)]
fn managed_provider_cli_dir_override() -> &'static Mutex<Option<PathBuf>> {
    static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(crate) struct ManagedProviderCliDirOverrideGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl Drop for ManagedProviderCliDirOverrideGuard {
    fn drop(&mut self) {
        *managed_provider_cli_dir_override()
            .lock()
            .expect("managed provider CLI dir override mutex") = self.previous.take();
    }
}

#[cfg(test)]
pub(crate) fn override_managed_provider_cli_dir_for_tests(
    path: PathBuf,
) -> ManagedProviderCliDirOverrideGuard {
    let mut override_path = managed_provider_cli_dir_override()
        .lock()
        .expect("managed provider CLI dir override mutex");
    let previous = override_path.replace(path);
    ManagedProviderCliDirOverrideGuard { previous }
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

pub fn task_validation_log_dir(task_id: &str, run_id: &str) -> PathBuf {
    app_log_dir()
        .join("task-validation")
        .join(hashed_log_component("task", task_id))
        .join(hashed_log_component("run", run_id))
}

pub fn task_validation_command_log_file(
    task_id: &str,
    run_id: &str,
    command_id: &str,
    stream: &str,
) -> PathBuf {
    let stream = match stream {
        "stdout" => "stdout",
        "stderr" => "stderr",
        _ => "output",
    };
    task_validation_log_dir(task_id, run_id).join(format!(
        "{}-{stream}.log",
        hashed_log_component("command", command_id)
    ))
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
#[path = "runtime_log_paths_tests.rs"]
mod tests;
