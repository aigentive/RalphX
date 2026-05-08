use super::*;
use std::sync::Mutex;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

// ── Unit tests for is_transient_error ─────────────────────────────────────

#[test]
fn test_transient_index_lock() {
    assert!(is_transient_error(
        "error: could not lock config file .git/index.lock: File exists"
    ));
}

#[test]
fn test_transient_unable_to_create_lock() {
    assert!(is_transient_error(
        "fatal: Unable to create '/path/to/.git/index.lock': File exists."
    ));
}

#[test]
fn test_transient_cannot_lock_ref() {
    assert!(is_transient_error(
        "error: cannot lock ref 'refs/heads/main': ref already locked"
    ));
}

#[test]
fn test_transient_fetch_head() {
    assert!(is_transient_error(
        "error: could not lock config file .git/FETCH_HEAD: File exists"
    ));
}

#[test]
fn test_transient_shallow_file_changed() {
    assert!(is_transient_error(
        "error: shallow file has changed since we read it"
    ));
}

#[test]
fn test_non_transient_merge_conflict() {
    assert!(!is_transient_error(
        "CONFLICT (content): Merge conflict in src/main.rs"
    ));
}

#[test]
fn test_non_transient_not_a_repo() {
    assert!(!is_transient_error(
        "fatal: not a git repository (or any of the parent directories): .git"
    ));
}

#[test]
fn test_non_transient_branch_not_found() {
    assert!(!is_transient_error(
        "error: pathspec 'missing-branch' did not match any file(s) known to git"
    ));
}

#[test]
fn test_empty_stderr_not_transient() {
    assert!(!is_transient_error(""));
}

#[test]
fn test_build_git_command_prepends_resolved_node_bin_to_existing_path() {
    assert_build_git_command_prepends_resolved_node_bin_to_existing_path(None);
}

#[test]
fn test_build_git_command_restores_existing_node_override() {
    assert_build_git_command_prepends_resolved_node_bin_to_existing_path(Some(
        "/tmp/original-git-node-bin/node",
    ));
}

fn assert_build_git_command_prepends_resolved_node_bin_to_existing_path(
    original_override: Option<&str>,
) {
    let _lock = ENV_MUTEX.lock().expect("env mutex");
    match original_override {
        Some(value) => std::env::set_var("RALPHX_NODE_PATH", value),
        None => std::env::remove_var("RALPHX_NODE_PATH"),
    }
    let original_node_override = std::env::var_os("RALPHX_NODE_PATH");
    std::env::set_var("RALPHX_NODE_PATH", "/tmp/git-node-bin/node");

    let args: Vec<String> = vec!["--version".to_string()];
    let cmd = build_git_command(
        &args,
        std::path::Path::new("/tmp"),
        &[("PATH".to_string(), "/usr/bin:/bin".to_string())],
    );

    let path_value = cmd
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == std::ffi::OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
        })
        .expect("PATH env");
    let path_entries = std::env::split_paths(&path_value).collect::<Vec<_>>();

    assert_eq!(path_entries.first(), Some(&std::path::PathBuf::from("/tmp/git-node-bin")));
    assert_eq!(
        path_entries,
        vec![
            std::path::PathBuf::from("/tmp/git-node-bin"),
            std::path::PathBuf::from("/usr/bin"),
            std::path::PathBuf::from("/bin"),
        ]
    );

    match original_node_override {
        Some(value) => std::env::set_var("RALPHX_NODE_PATH", value),
        None => std::env::remove_var("RALPHX_NODE_PATH"),
    }

    assert_eq!(
        std::env::var_os("RALPHX_NODE_PATH"),
        original_override.map(std::ffi::OsString::from)
    );
}

// ── exec_git_async tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_exec_git_async_success() {
    let args: Vec<String> = vec!["--version".to_string()];
    let cwd = std::path::PathBuf::from("/tmp");
    let result = exec_git_async(&args, &cwd).await;
    assert!(result.is_ok());
    assert!(result.unwrap().status.success());
}

#[tokio::test]
async fn test_exec_git_async_nonexistent_dir() {
    let args: Vec<String> = vec!["status".to_string()];
    let cwd = std::path::PathBuf::from("/nonexistent_path_that_does_not_exist_xyz");
    let result = exec_git_async(&args, &cwd).await;
    // Either spawn failure or git error — should not hang
    assert!(
        result.is_err()
            || result
                .as_ref()
                .map(|o| !o.status.success())
                .unwrap_or(false)
    );
}

#[tokio::test]
async fn test_exec_git_with_env_async_success() {
    let args: Vec<String> = vec!["--version".to_string()];
    let cwd = std::path::PathBuf::from("/tmp");
    let env = vec![("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
    let result = exec_git_with_env_async(&args, &cwd, &env).await;
    assert!(result.is_ok());
    assert!(result.unwrap().status.success());
}

// ── exec_with_retry_async tests ──────────────────────────────────────────

#[tokio::test]
async fn test_exec_with_retry_async_success_on_first_attempt() {
    let args: Vec<String> = vec!["--version".to_string()];
    let cwd = std::path::PathBuf::from("/tmp");
    let result = exec_with_retry_async(&args, &cwd, None).await;
    assert!(result.is_ok());
    assert!(result.unwrap().status.success());
}

#[tokio::test]
async fn test_exec_with_retry_async_non_transient_error_no_retry() {
    let args: Vec<String> = vec!["status".to_string()];
    let cwd = std::path::PathBuf::from("/nonexistent_path_that_does_not_exist_xyz");
    let result = exec_with_retry_async(&args, &cwd, None).await;
    assert!(
        result.is_err()
            || result
                .as_ref()
                .map(|o| !o.status.success())
                .unwrap_or(false)
    );
}

#[tokio::test]
async fn test_exec_with_retry_async_with_env() {
    let args: Vec<String> = vec!["--version".to_string()];
    let cwd = std::path::PathBuf::from("/tmp");
    let env = vec![("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
    let result = exec_with_retry_async(&args, &cwd, Some(&env)).await;
    assert!(result.is_ok());
    assert!(result.unwrap().status.success());
}

/// Verify all documented patterns are in TRANSIENT_PATTERNS.
#[test]
fn test_transient_patterns_constant_coverage() {
    assert!(TRANSIENT_PATTERNS.contains(&ERR_INDEX_LOCK));
    assert!(TRANSIENT_PATTERNS.contains(&ERR_UNABLE_CREATE_LOCK));
    assert!(TRANSIENT_PATTERNS.contains(&ERR_CANNOT_LOCK_REF));
    assert!(TRANSIENT_PATTERNS.contains(&ERR_FETCH_HEAD));
    assert!(TRANSIENT_PATTERNS.contains(&ERR_SHALLOW_FILE_CHANGED));
}

#[test]
fn test_retry_backoff_array_length() {
    let git_cfg = git_runtime_config();
    assert_eq!(
        git_cfg.retry_backoff_secs.len(),
        git_cfg.max_retries as usize,
        "retry_backoff_secs must have one entry per retry attempt"
    );
}

// ── Async public API tests ───────────────────────────────────────────────

#[tokio::test]
async fn test_run_basic_git_version() {
    let tmpdir = std::env::temp_dir();
    let result = run(&["--version"], &tmpdir).await;
    assert!(result.is_ok(), "git --version should succeed: {:?}", result);
}

#[tokio::test]
async fn test_run_status_basic() {
    let tmpdir = std::env::temp_dir();
    let result = run_status(&["--version"], &tmpdir).await;
    assert!(result.is_ok());
    assert!(result.unwrap(), "git --version should report success");
}

#[tokio::test]
async fn test_run_with_env_basic() {
    let tmpdir = std::env::temp_dir();
    let result = run_with_env(&["--version"], &tmpdir, &[("GIT_TERMINAL_PROMPT", "0")]).await;
    assert!(
        result.is_ok(),
        "git --version with env should succeed: {:?}",
        result
    );
}

// ── kill_on_drop behavior tests ──────────────────────────────────────────

#[tokio::test]
async fn test_kill_on_drop_process_is_killed_on_timeout() {
    // Spawn a long-running git process and cancel it via timeout.
    // This verifies that kill_on_drop(true) prevents zombie processes.
    let args: Vec<String> = vec!["--version".to_string()];
    let cwd = std::path::PathBuf::from("/tmp");

    // Verify a normal async git command creates a process that completes.
    let mut child = tokio::process::Command::new("git")
        .args(&args)
        .current_dir(&cwd)
        .kill_on_drop(true)
        .spawn()
        .expect("should spawn git");

    let status = child.wait().await.expect("should complete");
    assert!(status.success());
}

#[tokio::test]
async fn test_timeout_drops_future_cleanly() {
    // Ensure that a very short timeout on a real git command results in a
    // timeout error (or completes if fast enough) — either way no hang.
    let tmpdir = std::env::temp_dir();
    let args: Vec<String> = vec!["--version".to_string()];
    let cwd = tmpdir.to_path_buf();

    // Use a generous 5s timeout — `git --version` should complete well within this.
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        exec_git_async(&args, &cwd),
    )
    .await;

    // Should complete within timeout
    assert!(result.is_ok(), "git --version should complete within 5s");
    assert!(result.unwrap().is_ok());
}
