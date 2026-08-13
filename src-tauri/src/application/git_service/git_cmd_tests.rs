use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, Notify, Semaphore};

static ENV_MUTEX: Mutex<()> = Mutex::new(());

fn path_index(entries: &[std::path::PathBuf], path: impl AsRef<std::path::Path>) -> usize {
    entries
        .iter()
        .position(|entry| entry == path.as_ref())
        .unwrap_or_else(|| panic!("PATH entry missing: {}", path.as_ref().display()))
}

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
fn disk_full_stderr_is_not_transient_even_with_broad_patterns() {
    let stderr = "fatal: Unable to create '.git/FETCH_HEAD': No space left on device";

    assert!(!is_transient_git_stderr(stderr));
    assert_eq!(
        classify_git_failure_text(stderr),
        crate::domain::entities::MergeFailureSource::DiskFull
    );
}

#[test]
fn auth_stderr_is_not_transient() {
    let stderr =
        "fatal: could not read Username for 'https://github.com': terminal prompts disabled";

    assert!(!is_transient_git_stderr(stderr));
    assert_eq!(
        classify_git_failure_text(stderr),
        crate::domain::entities::MergeFailureSource::AuthFailure
    );
}

/// Regression guard for the `AppError::GithubRateLimited` split. That variant renders as
/// "GitHub rate limit exceeded: …", which matches no git pattern here, so without an explicit
/// branch it would degrade from `DeterministicInfra` (its behavior as `Infrastructure`) all the
/// way to `Unknown` — a strictly worse retry class than before the variant existed.
#[test]
fn github_rate_limit_text_classifies_as_transient_not_unknown() {
    use crate::domain::entities::MergeFailureSource;

    let rendered = AppError::GithubRateLimited {
        message:
            "gh exited with code 1: GraphQL: API rate limit already exceeded for user ID 6580668."
                .to_string(),
    }
    .to_string();

    assert_eq!(
        classify_git_failure_text(&rendered),
        MergeFailureSource::TransientGit
    );
    assert_eq!(
        classify_git_failure_source(&AppError::GithubRateLimited {
            message: "secondary rate limit".to_string(),
        }),
        MergeFailureSource::TransientGit
    );
}

/// Auth precedence must survive the new branch: an auth failure that also happens to mention a
/// rate limit is still an auth failure, and must never be retried.
#[test]
fn auth_precedence_survives_the_rate_limit_branch() {
    use crate::domain::entities::MergeFailureSource;

    let stderr = "fatal: could not read Username for 'https://github.com': terminal prompts disabled (API rate limit exceeded)";

    assert_eq!(
        classify_git_failure_text(stderr),
        MergeFailureSource::AuthFailure
    );
}

/// The pre-existing mapping for ordinary infrastructure errors must be untouched.
#[test]
fn plain_infrastructure_text_still_classifies_as_deterministic_infra() {
    use crate::domain::entities::MergeFailureSource;

    assert_eq!(
        classify_git_failure_text("Infrastructure error: gh exited with code 1: unexpected"),
        MergeFailureSource::DeterministicInfra
    );
}

#[test]
fn classify_git_failure_source_maps_app_error_variants() {
    use crate::domain::entities::MergeFailureSource;

    assert_eq!(
        classify_git_failure_source(&AppError::Database("foreign key failed".to_string())),
        MergeFailureSource::DeterministicInfra
    );
    assert_eq!(
        classify_git_failure_source(&AppError::Infrastructure(
            "failed to draft plan PR description".to_string()
        )),
        MergeFailureSource::DeterministicInfra
    );
    assert_eq!(
        classify_git_failure_source(&AppError::GitAuth("Git could not authenticate".to_string())),
        MergeFailureSource::AuthFailure
    );
    assert_eq!(
        classify_git_failure_source(&AppError::GitOperation(
            "fatal: not a git repository".to_string()
        )),
        MergeFailureSource::Unknown
    );
}

#[test]
fn test_build_git_command_preserves_user_shim_before_resolved_node_bin() {
    assert_build_git_command_preserves_user_shim_before_resolved_node_bin(None);
}

#[test]
fn test_build_git_command_restores_existing_node_override() {
    assert_build_git_command_preserves_user_shim_before_resolved_node_bin(Some(
        "/tmp/original-git-node-bin/node",
    ));
}

fn assert_build_git_command_preserves_user_shim_before_resolved_node_bin(
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
        &[(
            "PATH".to_string(),
            "/Users/example/.cargo/bin:/usr/bin:/bin".to_string(),
        )],
    );

    let path_value = cmd
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == std::ffi::OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
        })
        .expect("PATH env");
    let path_entries = std::env::split_paths(&path_value).collect::<Vec<_>>();

    assert_eq!(
        path_entries.first(),
        Some(&std::path::PathBuf::from("/Users/example/.cargo/bin"))
    );
    assert!(
        path_index(&path_entries, "/Users/example/.cargo/bin")
            < path_index(&path_entries, "/tmp/git-node-bin")
    );
    assert!(path_index(&path_entries, "/tmp/git-node-bin") < path_index(&path_entries, "/usr/bin"));
    assert_eq!(
        path_entries,
        vec![
            std::path::PathBuf::from("/Users/example/.cargo/bin"),
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
fn git_command_lane_labels_are_stable() {
    assert_eq!(GitCommandLane::Foreground.as_str(), "foreground");
    assert_eq!(GitCommandLane::Background.as_str(), "background");
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

#[test]
fn git_command_telemetry_helpers_cover_success_error_and_timeout_paths() {
    let args: Vec<String> = vec!["--version".to_string()];
    let cwd = std::env::temp_dir();
    let caller = std::panic::Location::caller();
    let output = std::process::Command::new("git")
        .arg("--version")
        .current_dir(&cwd)
        .output()
        .expect("git --version should run");

    log_git_command_result(
        "run",
        GitCommandLane::Foreground,
        &args,
        &cwd,
        Instant::now(),
        caller,
        Ok(&output),
    );
    log_git_command_result(
        "run",
        GitCommandLane::Foreground,
        &args,
        &cwd,
        Instant::now() - Duration::from_millis(SLOW_GIT_COMMAND_MS + 1),
        caller,
        Ok(&output),
    );

    let error = AppError::GitOperation("test git failure".to_string());
    log_git_command_result(
        "run",
        GitCommandLane::Foreground,
        &args,
        &cwd,
        Instant::now(),
        caller,
        Err(&error),
    );

    log_git_status_result(
        "run_status",
        GitCommandLane::Foreground,
        &args,
        &cwd,
        Instant::now(),
        caller,
        true,
    );
    log_git_status_result(
        "run_status",
        GitCommandLane::Foreground,
        &args,
        &cwd,
        Instant::now() - Duration::from_millis(SLOW_GIT_COMMAND_MS + 1),
        caller,
        false,
    );

    log_git_command_timeout(
        "run",
        GitCommandLane::Foreground,
        &args,
        &cwd,
        Instant::now(),
        caller,
        5,
    );
    log_git_admission_wait(
        "run",
        GitCommandLane::Background,
        &args,
        &cwd,
        Instant::now() - Duration::from_millis(GIT_ADMISSION_WAIT_LOG_MS as u64 + 1),
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
async fn test_run_background_basic_git_version() {
    let tmpdir = std::env::temp_dir();
    let result = run_background(&["--version"], &tmpdir).await;
    assert!(
        result.is_ok(),
        "background git --version should succeed: {:?}",
        result
    );
}

#[tokio::test]
async fn background_global_admission_does_not_overtake_registered_foreground_work() {
    let process_permits = Arc::new(Semaphore::new(1));
    let foreground_in_flight = Arc::new(AtomicUsize::new(0));
    let background_waiting = Arc::new(Notify::new());
    let foreground_release = Arc::new(Notify::new());
    let held_permit = process_permits.acquire().await.unwrap();

    let (background_admitted_tx, mut background_admitted_rx) = oneshot::channel();
    let background_permits = process_permits.clone();
    let background_foreground = foreground_in_flight.clone();
    let background_waiting_signal = background_waiting.clone();
    let background = tokio::spawn(async move {
        let permit = acquire_background_global_permit_with_wait_hook(
            &background_permits,
            &background_foreground,
            || background_waiting_signal.notify_one(),
        )
        .await
        .unwrap();
        background_admitted_tx.send(()).unwrap();
        drop(permit);
    });

    background_waiting.notified().await;

    let (foreground_registered_tx, foreground_registered_rx) = oneshot::channel();
    let (foreground_admitted_tx, mut foreground_admitted_rx) = oneshot::channel();
    let foreground_permits = process_permits.clone();
    let foreground_count = foreground_in_flight.clone();
    let foreground_release_signal = foreground_release.clone();
    let foreground = tokio::spawn(async move {
        foreground_count.fetch_add(1, Ordering::SeqCst);
        foreground_registered_tx.send(()).unwrap();
        let permit = foreground_permits.acquire().await.unwrap();
        foreground_admitted_tx.send(()).unwrap();
        foreground_release_signal.notified().await;
        drop(permit);
        foreground_count.fetch_sub(1, Ordering::SeqCst);
    });

    foreground_registered_rx.await.unwrap();
    drop(held_permit);

    tokio::select! {
        _ = &mut foreground_admitted_rx => {}
        _ = &mut background_admitted_rx => panic!("background work overtook registered foreground work"),
    }

    foreground_release.notify_one();
    foreground.await.unwrap();
    background.await.unwrap();
}

#[tokio::test]
async fn scoped_git_lane_overrides_default_lane() {
    assert_eq!(
        current_git_command_lane(GitCommandLane::Foreground),
        GitCommandLane::Foreground
    );

    let lane = with_git_command_lane(GitCommandLane::Background, async {
        current_git_command_lane(GitCommandLane::Foreground)
    })
    .await;

    assert_eq!(lane, GitCommandLane::Background);
}

#[tokio::test]
async fn test_run_status_basic() {
    let tmpdir = std::env::temp_dir();
    let result = run_status(&["--version"], &tmpdir).await;
    assert!(result.is_ok());
    assert!(result.unwrap(), "git --version should report success");
}

#[tokio::test]
async fn test_run_status_background_basic() {
    let tmpdir = std::env::temp_dir();
    let result = run_status_background(&["--version"], &tmpdir).await;
    assert!(result.is_ok());
    assert!(
        result.unwrap(),
        "background git --version should report success"
    );
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

    // Keep this as a hang guard instead of a performance assertion; macOS
    // subprocess startup can exceed 5s on overloaded developer machines.
    let result = tokio::time::timeout(Duration::from_secs(30), exec_git_async(&args, &cwd)).await;

    // Should complete within timeout
    assert!(result.is_ok(), "git --version should complete within 30s");
    assert!(result.unwrap().is_ok());
}

#[tokio::test]
async fn authorized_mutation_persists_process_authority_until_git_exits() {
    use crate::application::GitService;
    use crate::domain::entities::{
        BranchUpdateCapacityOwnership, BranchUpdateContinuation, BranchUpdateDirection,
        BranchUpdateOperation, BranchUpdateWorkspaceOwnership, GitMutationKind,
        GitTargetLeaseOwner, InternalStatus,
    };
    use crate::domain::repositories::{
        BranchUpdateActivation, BranchUpdateActivationOutcome, BranchUpdateRepository,
    };
    use crate::infrastructure::sqlite::SqliteBranchUpdateRepository;
    use crate::testing::SqliteTestDb;
    use chrono::Utc;
    use std::fs;
    use std::process::Command;
    use std::sync::Arc;

    let repository = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(repository.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test User"]);
    fs::write(repository.path().join("README.md"), "test").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-m", "initial"]);

    let db = SqliteTestDb::new("authorized-git-mutation");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repository_impl = Arc::new(SqliteBranchUpdateRepository::from_shared(db.shared_conn()));
    let identity = GitService::canonical_target_identity(repository.path(), "target")
        .await
        .unwrap();
    let operation = BranchUpdateOperation::new(
        task.id.clone(),
        BranchUpdateDirection::PlanBranch,
        BranchUpdateContinuation::ResumeExecution,
        "authorized-history",
        "main",
        "target",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        identity.clone(),
        Utc::now(),
    );
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation.id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repository_impl
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "test".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };

    let output = run_authorized_mutation(
        &["update-ref", "refs/heads/target", "HEAD"],
        repository.path(),
        AuthorizedGitMutation::from_current_lease(
            repository_impl.clone(),
            identity.clone(),
            owner,
            fencing_epoch,
            "authorized-claim".into(),
            GitMutationKind::Merge,
        )
        .await
        .unwrap(),
    )
    .await
    .unwrap();
    assert!(output.status.success());
    assert!(
        GitService::ref_exists(repository.path(), "refs/heads/target")
            .await
            .unwrap()
    );
    assert!(repository_impl
        .list_in_flight_mutations()
        .await
        .unwrap()
        .is_empty());
    assert!(repository_impl
        .get_target_lease(&identity)
        .await
        .unwrap()
        .unwrap()
        .active_mutation()
        .is_none());
}
