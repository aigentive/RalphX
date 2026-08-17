//! Async git command runner — all git operations use `tokio::process::Command`
//! with `kill_on_drop(true)` to prevent zombie processes on timeout.
//!
//! All git commands are executed with:
//! - `kill_on_drop(true)` — process is SIGKILL'd when the future is dropped (e.g. on timeout).
//! - A configurable timeout (from `git_runtime_config()`) to prevent hung processes.
//! - Configurable retry attempts with backoff for known transient errors.
use crate::domain::entities::MergeFailureSource;
use crate::domain::entities::{GitMutationKind, GitTargetIdentity, GitTargetLeaseOwner};
use crate::domain::repositories::{
    BeginGitMutation, BranchUpdateRepository, CompleteGitMutation, GitAuthorityCasOutcome,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::git_auth::{apply_git_subprocess_env, is_git_auth_failure_text};
use crate::infrastructure::services::gh_cli_github_service::is_github_rate_limit_message;
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use std::panic::Location;
use std::path::Path;
use std::process::{Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::{Semaphore, SemaphorePermit, TryAcquireError};
use tokio::time::timeout;

const SLOW_GIT_COMMAND_MS: u64 = 500;
const GIT_PROCESS_CONCURRENCY: usize = 4;
const GIT_BACKGROUND_CONCURRENCY: usize = 1;
const GIT_ADMISSION_WAIT_LOG_MS: u128 = 50;

static GIT_PROCESS_PERMITS: Semaphore = Semaphore::const_new(GIT_PROCESS_CONCURRENCY);
static GIT_BACKGROUND_PERMITS: Semaphore = Semaphore::const_new(GIT_BACKGROUND_CONCURRENCY);
static GIT_FOREGROUND_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static GIT_COMMANDS_QUEUED: AtomicUsize = AtomicUsize::new(0);
static GIT_COMMANDS_EXECUTING: AtomicUsize = AtomicUsize::new(0);

tokio::task_local! {
    static GIT_COMMAND_LANE_OVERRIDE: GitCommandLane;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitCommandLane {
    Foreground,
    Background,
}

#[derive(Clone)]
pub(crate) struct AuthorizedGitMutation {
    repository: Arc<dyn BranchUpdateRepository>,
    identity: GitTargetIdentity,
    owner: GitTargetLeaseOwner,
    fencing_epoch: u64,
    claim_id: String,
    kind: GitMutationKind,
}

impl AuthorizedGitMutation {
    /// Materialize a non-serializable mutation guard from the current durable
    /// lease. The subsequent begin-mutation CAS revalidates this snapshot at
    /// the exact effect boundary.
    pub(crate) async fn from_current_lease(
        repository: Arc<dyn BranchUpdateRepository>,
        identity: GitTargetIdentity,
        owner: GitTargetLeaseOwner,
        fencing_epoch: u64,
        claim_id: String,
        kind: GitMutationKind,
    ) -> AppResult<Self> {
        let lease = repository
            .get_target_lease(&identity)
            .await?
            .ok_or_else(|| {
                AppError::Validation("Git mutation target has no durable lease".to_string())
            })?;
        if lease.is_released() || lease.owner() != &owner || lease.fencing_epoch() != fencing_epoch
        {
            return Err(AppError::Validation(
                "Git mutation target lease is stale or owned by another workflow".to_string(),
            ));
        }
        Ok(Self {
            repository,
            identity,
            owner,
            fencing_epoch,
            claim_id,
            kind,
        })
    }
}

impl GitCommandLane {
    fn as_str(self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
        }
    }
}

struct GitAdmissionGuard {
    lane: GitCommandLane,
    _global: SemaphorePermit<'static>,
    _background: Option<SemaphorePermit<'static>>,
}

impl Drop for GitAdmissionGuard {
    fn drop(&mut self) {
        GIT_COMMANDS_EXECUTING.fetch_sub(1, Ordering::SeqCst);
        if self.lane == GitCommandLane::Foreground {
            GIT_FOREGROUND_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

struct GitAdmissionQueueGuard {
    queued: bool,
    foreground_registered: bool,
}

impl GitAdmissionQueueGuard {
    fn new() -> Self {
        GIT_COMMANDS_QUEUED.fetch_add(1, Ordering::SeqCst);
        Self {
            queued: true,
            foreground_registered: false,
        }
    }

    fn register_foreground(&mut self) {
        GIT_FOREGROUND_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        self.foreground_registered = true;
    }

    fn admitted(&mut self) {
        if self.queued {
            GIT_COMMANDS_QUEUED.fetch_sub(1, Ordering::SeqCst);
            GIT_COMMANDS_EXECUTING.fetch_add(1, Ordering::SeqCst);
            self.queued = false;
        }
        // The admitted command guard now owns the foreground registration.
        self.foreground_registered = false;
    }
}

impl Drop for GitAdmissionQueueGuard {
    fn drop(&mut self) {
        if self.queued {
            GIT_COMMANDS_QUEUED.fetch_sub(1, Ordering::SeqCst);
        }
        if self.foreground_registered {
            GIT_FOREGROUND_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

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
const ERR_UNABLE_CREATE_LOCK: &str = "unable to create";

/// git cannot acquire a ref lock, usually due to concurrent ref updates.
const ERR_CANNOT_LOCK_REF: &str = "cannot lock ref";

/// git cannot update FETCH_HEAD due to concurrent fetch operations.
const ERR_FETCH_HEAD: &str = "fetch_head";

/// git could not write repository state because disk space or quota was exhausted.
const ERR_NO_SPACE_LEFT: &str = "no space left on device";
const ERR_COULD_NOT_WRITE_INDEX: &str = "could not write new index file";
const ERR_ENOSPC: &str = "enospc";
const ERR_DISK_QUOTA: &str = "disk quota exceeded";

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

const DISK_FULL_PATTERNS: &[&str] = &[
    ERR_NO_SPACE_LEFT,
    ERR_COULD_NOT_WRITE_INDEX,
    ERR_ENOSPC,
    ERR_DISK_QUOTA,
];

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Returns true if the given stderr output matches any known transient error pattern.
fn is_transient_error(stderr: &str) -> bool {
    is_transient_git_stderr(stderr)
}

pub(crate) fn is_disk_full_git_stderr(stderr: &str) -> bool {
    let normalized = stderr.to_lowercase();
    DISK_FULL_PATTERNS
        .iter()
        .any(|pattern| normalized.contains(pattern))
}

/// Returns true when stderr is a retryable git failure. Deterministic classes
/// are checked first so broad git substrings cannot mask auth or ENOSPC errors.
pub(crate) fn is_transient_git_stderr(stderr: &str) -> bool {
    let normalized = stderr.to_lowercase();
    if is_disk_full_git_stderr(&normalized) || is_git_auth_failure_text(&normalized) {
        return false;
    }

    TRANSIENT_PATTERNS
        .iter()
        .any(|pattern| normalized.contains(pattern))
}

pub(crate) fn classify_git_failure_text(text: &str) -> MergeFailureSource {
    let normalized = text.to_lowercase();

    if is_git_auth_failure_text(&normalized) {
        return MergeFailureSource::AuthFailure;
    }
    if is_disk_full_git_stderr(&normalized) {
        return MergeFailureSource::DiskFull;
    }
    // `AppError::GithubRateLimited` renders as "GitHub rate limit exceeded: …", which matches none
    // of the git patterns below and would otherwise fall through to `Unknown`. Reuses `TransientGit`
    // rather than adding a variant: `merge_failure_source` is serialized into task metadata, so a
    // new variant would be a persisted-compat change. Classified before the lock/transient git
    // checks and after auth so auth precedence is preserved.
    if is_github_rate_limit_message(&normalized) {
        return MergeFailureSource::TransientGit;
    }
    if normalized.contains(ENOENT_MARKER) {
        return MergeFailureSource::WorktreeMissing;
    }
    if normalized.contains(ERR_INDEX_LOCK)
        || normalized.contains(".lock")
        || normalized.contains(ERR_CANNOT_LOCK_REF)
    {
        return MergeFailureSource::LockContention;
    }
    if is_transient_git_stderr(&normalized) {
        return MergeFailureSource::TransientGit;
    }
    if normalized.contains("database error:") || normalized.contains("infrastructure error:") {
        return MergeFailureSource::DeterministicInfra;
    }

    MergeFailureSource::Unknown
}

pub(crate) fn classify_git_failure_source(error: &AppError) -> MergeFailureSource {
    match error {
        AppError::Database(_) | AppError::Infrastructure(_) => {
            MergeFailureSource::DeterministicInfra
        }
        AppError::GitAuth(_) => MergeFailureSource::AuthFailure,
        // Retryable once GitHub's window resets; see the rate-limit branch in
        // `classify_git_failure_text` for why this reuses `TransientGit`.
        AppError::GithubRateLimited { .. } => MergeFailureSource::TransientGit,
        AppError::GitOperation(message) => classify_git_failure_text(message),
        _ => classify_git_failure_text(&error.to_string()),
    }
}

fn build_git_command(args: &[String], cwd: &Path, env: &[(String, String)]) -> Command {
    let mut cmd = Command::new(resolve_git_cli_path());
    apply_git_subprocess_env(&mut cmd);
    cmd.args(args).current_dir(cwd).kill_on_drop(true);
    for (key, val) in env {
        cmd.env(key, val);
    }
    crate::infrastructure::tool_paths::ensure_resolved_node_bin_in_path(cmd.as_std_mut());
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

pub(crate) async fn with_git_command_lane<F, T>(lane: GitCommandLane, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    GIT_COMMAND_LANE_OVERRIDE.scope(lane, future).await
}

/// Run a git command asynchronously, returning full Output.
///
/// Uses `tokio::process::Command` with `kill_on_drop(true)` — when the timeout
/// fires, the future is dropped, which kills the git process immediately.
/// Applies a configurable timeout and retries for transient errors.
#[track_caller]
pub(crate) fn run<'a>(
    args: &'a [&'a str],
    cwd: &'a Path,
) -> impl std::future::Future<Output = AppResult<Output>> + 'a {
    run_in_lane(args, cwd, GitCommandLane::Foreground)
}

/// Run a mutating Git command under durable target authority.
///
/// The mutation claim is persisted before spawn, the child process group is
/// bound immediately after spawn, and the claim is cleared only after the
/// process group has exited (or has been terminated on timeout).
pub(crate) async fn run_authorized_mutation(
    args: &[&str],
    cwd: &Path,
    authority: AuthorizedGitMutation,
) -> AppResult<Output> {
    match authority
        .repository
        .begin_git_mutation(BeginGitMutation {
            identity: authority.identity.clone(),
            owner: authority.owner.clone(),
            fencing_epoch: authority.fencing_epoch,
            claim_id: authority.claim_id.clone(),
            kind: authority.kind,
        })
        .await?
    {
        GitAuthorityCasOutcome::Applied { .. } => {}
        outcome => {
            return Err(AppError::Validation(format!(
                "Git mutation authority rejected before spawn: {outcome:?}"
            )))
        }
    }

    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let lane = current_git_command_lane(GitCommandLane::Foreground);
    let _admission = match acquire_git_admission(lane, "run_authorized_mutation", &args, cwd).await
    {
        Ok(admission) => admission,
        Err(error) => {
            let _ = authority
                .repository
                .complete_git_mutation(CompleteGitMutation {
                    identity: authority.identity,
                    owner: authority.owner,
                    fencing_epoch: authority.fencing_epoch,
                    claim_id: authority.claim_id,
                })
                .await;
            return Err(error);
        }
    };
    let mut command = build_git_command(&args, cwd, &[]);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = authority
                .repository
                .complete_git_mutation(CompleteGitMutation {
                    identity: authority.identity,
                    owner: authority.owner,
                    fencing_epoch: authority.fencing_epoch,
                    claim_id: authority.claim_id,
                })
                .await;
            return Err(AppError::GitOperation(format!(
                "git {} failed to spawn: {error}",
                args.join(" ")
            )));
        }
    };
    let Some(process_group_id) = child.id().map(i64::from) else {
        let _ = child.kill().await;
        let _ = authority
            .repository
            .complete_git_mutation(CompleteGitMutation {
                identity: authority.identity,
                owner: authority.owner,
                fencing_epoch: authority.fencing_epoch,
                claim_id: authority.claim_id,
            })
            .await;
        return Err(AppError::GitOperation(
            "spawned Git process has no process id".to_string(),
        ));
    };
    let bound = authority
        .repository
        .bind_git_process_group(
            &authority.identity,
            &authority.owner,
            authority.fencing_epoch,
            &authority.claim_id,
            process_group_id,
        )
        .await;
    let bound = match bound {
        Ok(bound) => bound,
        Err(error) => {
            #[cfg(unix)]
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(process_group_id as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            let _ = child.kill().await;
            return Err(error);
        }
    };
    if !matches!(bound, GitAuthorityCasOutcome::Applied { .. }) {
        #[cfg(unix)]
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(process_group_id as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
        let _ = child.kill().await;
        return Err(AppError::Validation(format!(
            "Git mutation authority changed after spawn: {bound:?}"
        )));
    }

    let timeout_secs = git_runtime_config().cmd_timeout_secs;
    let result = timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await;
    #[cfg(unix)]
    if result.is_err() && !terminate_and_confirm_process_group_exit(process_group_id).await {
        return Err(AppError::GitOperation(format!(
            "git command timed out after {timeout_secs}s and its process group is still alive; durable mutation authority remains fenced"
        )));
    }
    let output = match result {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(AppError::GitOperation(format!(
            "git {} failed: {error}",
            args.join(" ")
        ))),
        Err(_) => Err(AppError::GitOperation(format!(
            "git command timed out after {timeout_secs}s"
        ))),
    };
    let completion = authority
        .repository
        .complete_git_mutation(CompleteGitMutation {
            identity: authority.identity,
            owner: authority.owner,
            fencing_epoch: authority.fencing_epoch,
            claim_id: authority.claim_id,
        })
        .await?;
    if !matches!(completion, GitAuthorityCasOutcome::Applied { .. }) {
        return Err(AppError::Validation(format!(
            "Git mutation completion lost authority: {completion:?}"
        )));
    }
    output
}

#[cfg(unix)]
async fn terminate_and_confirm_process_group_exit(process_group_id: i64) -> bool {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::Pid;

    let group = Pid::from_raw(process_group_id as i32);
    let _ = killpg(group, Signal::SIGKILL);
    for _ in 0..50 {
        if killpg(group, None).is_err() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// Run a git command in the background lane.
#[cfg(test)]
#[track_caller]
pub(crate) fn run_background<'a>(
    args: &'a [&'a str],
    cwd: &'a Path,
) -> impl std::future::Future<Output = AppResult<Output>> + 'a {
    run_in_lane(args, cwd, GitCommandLane::Background)
}

#[track_caller]
pub(crate) fn run_in_lane<'a>(
    args: &'a [&'a str],
    cwd: &'a Path,
    default_lane: GitCommandLane,
) -> impl std::future::Future<Output = AppResult<Output>> + 'a {
    let caller = Location::caller();
    async move {
        let lane = current_git_command_lane(default_lane);
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let cwd = cwd.to_path_buf();
        let timeout_secs = git_runtime_config().cmd_timeout_secs;
        let _admission = acquire_git_admission(lane, "run", &args, &cwd).await?;
        let started = Instant::now();

        match timeout(
            Duration::from_secs(timeout_secs),
            exec_with_retry_async(&args, &cwd, None),
        )
        .await
        {
            Ok(result) => {
                log_git_command_result("run", lane, &args, &cwd, started, caller, result.as_ref());
                result
            }
            Err(_) => {
                log_git_command_timeout("run", lane, &args, &cwd, started, caller, timeout_secs);
                Err(AppError::GitOperation(format!(
                    "git command timed out after {timeout_secs}s"
                )))
            }
        }
    }
}

/// Run a git command with additional environment variables.
///
/// Uses `tokio::process::Command` with `kill_on_drop(true)`.
/// Applies a configurable timeout and retries for transient errors.
#[track_caller]
pub(crate) fn run_with_env<'a>(
    args: &'a [&'a str],
    cwd: &'a Path,
    env: &'a [(&'a str, &'a str)],
) -> impl std::future::Future<Output = AppResult<Output>> + 'a {
    run_with_env_in_lane(args, cwd, env, GitCommandLane::Foreground)
}

#[track_caller]
pub(crate) fn run_with_env_in_lane<'a>(
    args: &'a [&'a str],
    cwd: &'a Path,
    env: &'a [(&'a str, &'a str)],
    default_lane: GitCommandLane,
) -> impl std::future::Future<Output = AppResult<Output>> + 'a {
    let caller = Location::caller();
    async move {
        let lane = current_git_command_lane(default_lane);
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let cwd = cwd.to_path_buf();
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let timeout_secs = git_runtime_config().cmd_timeout_secs;
        let _admission = acquire_git_admission(lane, "run_with_env", &args, &cwd).await?;
        let started = Instant::now();

        match timeout(
            Duration::from_secs(timeout_secs),
            exec_with_retry_async(&args, &cwd, Some(&env)),
        )
        .await
        {
            Ok(result) => {
                log_git_command_result(
                    "run_with_env",
                    lane,
                    &args,
                    &cwd,
                    started,
                    caller,
                    result.as_ref(),
                );
                result
            }
            Err(_) => {
                log_git_command_timeout(
                    "run_with_env",
                    lane,
                    &args,
                    &cwd,
                    started,
                    caller,
                    timeout_secs,
                );
                Err(AppError::GitOperation(format!(
                    "git command timed out after {timeout_secs}s"
                )))
            }
        }
    }
}

/// Run a git command returning just success/failure (for existence checks).
///
/// Uses `tokio::process::Command` with `kill_on_drop(true)`.
/// Status checks are not retried (they should be fast and idempotent).
#[track_caller]
pub(crate) fn run_status<'a>(
    args: &'a [&'a str],
    cwd: &'a Path,
) -> impl std::future::Future<Output = AppResult<bool>> + 'a {
    run_status_in_lane(args, cwd, GitCommandLane::Foreground)
}

/// Run a git status-style command in the background lane.
#[cfg(test)]
#[track_caller]
pub(crate) fn run_status_background<'a>(
    args: &'a [&'a str],
    cwd: &'a Path,
) -> impl std::future::Future<Output = AppResult<bool>> + 'a {
    run_status_in_lane(args, cwd, GitCommandLane::Background)
}

#[track_caller]
pub(crate) fn run_status_in_lane<'a>(
    args: &'a [&'a str],
    cwd: &'a Path,
    default_lane: GitCommandLane,
) -> impl std::future::Future<Output = AppResult<bool>> + 'a {
    let caller = Location::caller();
    async move {
        let lane = current_git_command_lane(default_lane);
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let cwd = cwd.to_path_buf();
        let timeout_secs = git_runtime_config().cmd_timeout_secs;
        let _admission = acquire_git_admission(lane, "run_status", &args, &cwd).await?;
        let started = Instant::now();

        let result = match timeout(Duration::from_secs(timeout_secs), async {
            let mut cmd = build_git_command(&args, &cwd, &[]);
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
            cmd.status().await.map(|s| s.success()).unwrap_or(false)
        })
        .await
        {
            Ok(result) => {
                log_git_status_result("run_status", lane, &args, &cwd, started, caller, result);
                result
            }
            Err(_) => {
                log_git_command_timeout(
                    "run_status",
                    lane,
                    &args,
                    &cwd,
                    started,
                    caller,
                    timeout_secs,
                );
                return Err(AppError::GitOperation(format!(
                    "git status check timed out after {timeout_secs}s"
                )));
            }
        };

        Ok(result)
    }
}

fn current_git_command_lane(default_lane: GitCommandLane) -> GitCommandLane {
    GIT_COMMAND_LANE_OVERRIDE
        .try_with(|lane| *lane)
        .unwrap_or(default_lane)
}

async fn acquire_git_admission(
    lane: GitCommandLane,
    operation: &'static str,
    args: &[String],
    cwd: &Path,
) -> AppResult<GitAdmissionGuard> {
    let started = Instant::now();
    let mut queue_guard = GitAdmissionQueueGuard::new();
    match lane {
        GitCommandLane::Foreground => {
            queue_guard.register_foreground();
            let global = match GIT_PROCESS_PERMITS.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    return Err(AppError::GitOperation(
                        "git foreground admission closed".to_string(),
                    ));
                }
            };
            queue_guard.admitted();
            log_git_admission_wait(operation, lane, args, cwd, started);
            Ok(GitAdmissionGuard {
                lane,
                _global: global,
                _background: None,
            })
        }
        GitCommandLane::Background => {
            let background = GIT_BACKGROUND_PERMITS.acquire().await.map_err(|_| {
                AppError::GitOperation("git background admission closed".to_string())
            })?;
            let global =
                acquire_background_global_permit(&GIT_PROCESS_PERMITS, &GIT_FOREGROUND_IN_FLIGHT)
                    .await?;
            queue_guard.admitted();
            log_git_admission_wait(operation, lane, args, cwd, started);
            Ok(GitAdmissionGuard {
                lane,
                _global: global,
                _background: Some(background),
            })
        }
    }
}

async fn acquire_background_global_permit<'a>(
    process_permits: &'a Semaphore,
    foreground_in_flight: &AtomicUsize,
) -> AppResult<SemaphorePermit<'a>> {
    acquire_background_global_permit_with_wait_hook(process_permits, foreground_in_flight, || {})
        .await
}

async fn acquire_background_global_permit_with_wait_hook<'a, F>(
    process_permits: &'a Semaphore,
    foreground_in_flight: &AtomicUsize,
    on_wait: F,
) -> AppResult<SemaphorePermit<'a>>
where
    F: Fn(),
{
    loop {
        while foreground_in_flight.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        match process_permits.try_acquire() {
            Ok(permit) => {
                if foreground_in_flight.load(Ordering::SeqCst) == 0 {
                    return Ok(permit);
                }
                drop(permit);
            }
            Err(TryAcquireError::NoPermits) => on_wait(),
            Err(TryAcquireError::Closed) => {
                return Err(AppError::GitOperation(
                    "git global admission closed".to_string(),
                ));
            }
        }

        // Do not wait in the fair global semaphore queue: a foreground command
        // registered after this check must be able to queue and acquire first.
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn log_git_admission_wait(
    operation: &'static str,
    lane: GitCommandLane,
    args: &[String],
    cwd: &Path,
    started: Instant,
) {
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms >= GIT_ADMISSION_WAIT_LOG_MS {
        tracing::info!(
            operation,
            lane = lane.as_str(),
            command = %args.join(" "),
            cwd = %cwd.display(),
            admission_wait_ms = elapsed_ms as u64,
            queued = GIT_COMMANDS_QUEUED.load(Ordering::SeqCst),
            in_flight = GIT_COMMANDS_EXECUTING.load(Ordering::SeqCst),
            "Git command admission waited"
        );
    }
}

fn log_git_command_result(
    operation: &'static str,
    lane: GitCommandLane,
    args: &[String],
    cwd: &Path,
    started: Instant,
    caller: &'static Location<'static>,
    result: Result<&Output, &AppError>,
) {
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let command = args.join(" ");
    match result {
        Ok(output) => {
            let success = output.status.success();
            let exit_code = output.status.code();
            if elapsed_ms >= SLOW_GIT_COMMAND_MS {
                tracing::warn!(
                    operation,
                    lane = lane.as_str(),
                    command = %command,
                    cwd = %cwd.display(),
                    caller_file = caller.file(),
                    caller_line = caller.line(),
                    elapsed_ms,
                    success,
                    exit_code,
                    "Slow git command completed"
                );
            } else {
                tracing::debug!(
                    operation,
                    lane = lane.as_str(),
                    command = %command,
                    cwd = %cwd.display(),
                    caller_file = caller.file(),
                    caller_line = caller.line(),
                    elapsed_ms,
                    success,
                    exit_code,
                    "Git command completed"
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                operation,
                lane = lane.as_str(),
                command = %command,
                cwd = %cwd.display(),
                caller_file = caller.file(),
                caller_line = caller.line(),
                elapsed_ms,
                error = %error,
                "Git command failed"
            );
        }
    }
}

fn log_git_status_result(
    operation: &'static str,
    lane: GitCommandLane,
    args: &[String],
    cwd: &Path,
    started: Instant,
    caller: &'static Location<'static>,
    success: bool,
) {
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let command = args.join(" ");
    if elapsed_ms >= SLOW_GIT_COMMAND_MS {
        tracing::warn!(
            operation,
            lane = lane.as_str(),
            command = %command,
            cwd = %cwd.display(),
            caller_file = caller.file(),
            caller_line = caller.line(),
            elapsed_ms,
            success,
            "Slow git status command completed"
        );
    } else {
        tracing::debug!(
            operation,
            lane = lane.as_str(),
            command = %command,
            cwd = %cwd.display(),
            caller_file = caller.file(),
            caller_line = caller.line(),
            elapsed_ms,
            success,
            "Git status command completed"
        );
    }
}

fn log_git_command_timeout(
    operation: &'static str,
    lane: GitCommandLane,
    args: &[String],
    cwd: &Path,
    started: Instant,
    caller: &'static Location<'static>,
    timeout_secs: u64,
) {
    tracing::warn!(
        operation,
        lane = lane.as_str(),
        command = %args.join(" "),
        cwd = %cwd.display(),
        caller_file = caller.file(),
        caller_line = caller.line(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        timeout_secs,
        "Git command timed out"
    );
}

#[cfg(test)]
#[path = "git_cmd_tests.rs"]
mod tests;
