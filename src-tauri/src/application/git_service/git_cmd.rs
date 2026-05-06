//! Async git command runner — all git operations use `tokio::process::Command`
//! with `kill_on_drop(true)` to prevent zombie processes on timeout.
//!
//! All git commands are executed with:
//! - `kill_on_drop(true)` — process is SIGKILL'd when the future is dropped (e.g. on timeout).
//! - A configurable timeout (from `git_runtime_config()`) to prevent hung processes.
//! - Configurable retry attempts with backoff for known transient errors.
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::git_auth::apply_git_subprocess_env;
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

// ── Transient error pattern constants ────────────────────────────────────────
// Source: git stderr output on Linux/macOS. These are transient errors caused by
// lock contention, concurrent operations, or temporary I/O issues.

/// Lowercase marker fragment present in errors when a git spawn fails with ENOENT.
/// Classification sites matching against lowercased error strings should use this constant.
/// See: `exec_git_async` for the injection site.
pub(crate) const ENOENT_MARKER: &str = "working directory not found (enoent)";

/// git reports a stale or held index.lock file preventing operations.
const ERR_INDEX_LOCK: &str = "index.lock";

/// git cannot create a .lock file (e.g. for a ref or the index).
const ERR_UNABLE_CREATE_LOCK: &str = "Unable to create";

/// git cannot acquire a ref lock, usually due to concurrent ref updates.
const ERR_CANNOT_LOCK_REF: &str = "cannot lock ref";

/// git cannot update FETCH_HEAD due to concurrent fetch operations.
const ERR_FETCH_HEAD: &str = "FETCH_HEAD";

/// git's shallow file was modified concurrently during a fetch/clone.
const ERR_SHALLOW_FILE_CHANGED: &str = "shallow file has changed";

/// All patterns that indicate a transient (retriable) git failure.
/// These strings are matched against git's stderr output.
const TRANSIENT_PATTERNS: &[&str] = &[
    ERR_INDEX_LOCK,
    ERR_UNABLE_CREATE_LOCK,
    ERR_CANNOT_LOCK_REF,
    ERR_FETCH_HEAD,
    ERR_SHALLOW_FILE_CHANGED,
];

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Returns true if the given stderr output matches any known transient error pattern.
fn is_transient_error(stderr: &str) -> bool {
    TRANSIENT_PATTERNS.iter().any(|pat| stderr.contains(pat))
}

fn build_git_command(args: &[String], cwd: &Path, env: &[(String, String)]) -> Command {
    let mut cmd = Command::new(resolve_git_cli_path());
    apply_git_subprocess_env(&mut cmd);
    cmd.args(args).current_dir(cwd).kill_on_drop(true);
    for (key, val) in env {
        cmd.env(key, val);
    }
    crate::infrastructure::tool_paths::prepend_resolved_node_bin_to_path(cmd.as_std_mut());
    cmd
}

/// Execute a single git command asynchronously with `kill_on_drop(true)`.
async fn exec_git_async(args: &[String], cwd: &Path) -> AppResult<Output> {
    build_git_command(args, cwd, &[])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::GitOperation(format!(
                    "git {}: working directory not found (ENOENT): {}",
                    args.join(" "),
                    e
                ))
            } else {
                AppError::GitOperation(format!("git {}: {}", args.join(" "), e))
            }
        })
}

/// Execute a single git command asynchronously with env vars and `kill_on_drop(true)`.
async fn exec_git_with_env_async(
    args: &[String],
    cwd: &Path,
    env: &[(String, String)],
) -> AppResult<Output> {
    build_git_command(args, cwd, env)
        .output()
        .await
        .map_err(|e| AppError::GitOperation(format!("git {}: {}", args.join(" "), e)))
}

/// Async retry loop for transient git errors. Uses `tokio::time::sleep` for backoff.
async fn exec_with_retry_async(
    args: &[String],
    cwd: &Path,
    env: Option<&[(String, String)]>,
) -> AppResult<Output> {
    let git_cfg = git_runtime_config();
    let max_retries = git_cfg.max_retries as u32;
    let retry_backoff = &git_cfg.retry_backoff_secs;
    let mut last_err: Option<AppError> = None;

    for attempt in 0..max_retries {
        let result = match env {
            Some(e) => exec_git_with_env_async(args, cwd, e).await,
            None => exec_git_async(args, cwd).await,
        };

        match result {
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.success() || !is_transient_error(&stderr) {
                    // Success or non-transient failure — return immediately.
                    return Ok(output);
                }
                // Transient failure: record and retry after backoff.
                let backoff = retry_backoff.get(attempt as usize).copied().unwrap_or(4);
                tracing::warn!(
                    attempt = attempt + 1,
                    max = max_retries,
                    backoff_secs = backoff,
                    stderr = %stderr.trim(),
                    args = %args.join(" "),
                    "git transient error — retrying"
                );
                last_err = Some(AppError::GitOperation(format!(
                    "git {} transient error (attempt {}): {}",
                    args.join(" "),
                    attempt + 1,
                    stderr.trim()
                )));
                tokio::time::sleep(Duration::from_secs(backoff)).await;
            }
            Err(e) => {
                // spawn/IO error — not retriable
                return Err(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        AppError::GitOperation(format!(
            "git {} failed after {} retries",
            args.join(" "),
            max_retries
        ))
    }))
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Run a git command asynchronously, returning full Output.
///
/// Uses `tokio::process::Command` with `kill_on_drop(true)` — when the timeout
/// fires, the future is dropped, which kills the git process immediately.
/// Applies a configurable timeout and retries for transient errors.
pub(crate) async fn run(args: &[&str], cwd: &Path) -> AppResult<Output> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let cwd = cwd.to_path_buf();
    let timeout_secs = git_runtime_config().cmd_timeout_secs;

    timeout(
        Duration::from_secs(timeout_secs),
        exec_with_retry_async(&args, &cwd, None),
    )
    .await
    .map_err(|_| AppError::GitOperation(format!("git command timed out after {timeout_secs}s")))?
}

/// Run a git command with additional environment variables.
///
/// Uses `tokio::process::Command` with `kill_on_drop(true)`.
/// Applies a configurable timeout and retries for transient errors.
pub(crate) async fn run_with_env(
    args: &[&str],
    cwd: &Path,
    env: &[(&str, &str)],
) -> AppResult<Output> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let cwd = cwd.to_path_buf();
    let env: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let timeout_secs = git_runtime_config().cmd_timeout_secs;

    timeout(
        Duration::from_secs(timeout_secs),
        exec_with_retry_async(&args, &cwd, Some(&env)),
    )
    .await
    .map_err(|_| AppError::GitOperation(format!("git command timed out after {timeout_secs}s")))?
}

/// Run a git command returning just success/failure (for existence checks).
///
/// Uses `tokio::process::Command` with `kill_on_drop(true)`.
/// Status checks are not retried (they should be fast and idempotent).
pub(crate) async fn run_status(args: &[&str], cwd: &Path) -> AppResult<bool> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let cwd = cwd.to_path_buf();
    let timeout_secs = git_runtime_config().cmd_timeout_secs;

    let result = timeout(Duration::from_secs(timeout_secs), async {
        let mut cmd = build_git_command(&args, &cwd, &[]);
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        cmd.status().await.map(|s| s.success()).unwrap_or(false)
    })
    .await
    .map_err(|_| {
        AppError::GitOperation(format!("git status check timed out after {timeout_secs}s"))
    })?;

    Ok(result)
}

#[cfg(test)]
#[path = "git_cmd_tests.rs"]
mod tests;
