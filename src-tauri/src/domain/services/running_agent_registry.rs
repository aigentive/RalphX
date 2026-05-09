// Running Agent Registry
//
// Tracks running agent processes so they can be stopped on demand.
// Uses process IDs (PIDs) to allow cross-thread process termination.
//
// Trait-based design allows SQLite persistence (production) or in-memory (tests).

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use crate::infrastructure::tool_paths::{
    resolve_lsof_cli_path, resolve_pgrep_cli_path, resolve_pkill_cli_path,
};
#[cfg(windows)]
use crate::infrastructure::tool_paths::{resolve_taskkill_cli_path, resolve_tasklist_cli_path};

#[cfg(unix)]
use nix::sys::signal::{self, Signal};
#[cfg(unix)]
use nix::unistd::Pid;

#[derive(Clone)]
struct ProcessOps {
    is_alive: Arc<dyn Fn(u32) -> bool + Send + Sync>,
    kill: Arc<dyn Fn(u32) + Send + Sync>,
    kill_immediate: Arc<dyn Fn(u32) + Send + Sync>,
}

impl std::fmt::Debug for ProcessOps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProcessOps(..)")
    }
}

impl ProcessOps {
    fn real() -> Self {
        Self {
            is_alive: Arc::new(is_process_alive),
            kill: Arc::new(kill_process),
            kill_immediate: Arc::new(kill_process_immediate),
        }
    }

    fn is_alive(&self, pid: u32) -> bool {
        (self.is_alive)(pid)
    }

    fn kill(&self, pid: u32) {
        (self.kill)(pid);
    }

    fn kill_immediate(&self, pid: u32) {
        (self.kill_immediate)(pid);
    }
}

/// Key for identifying a running agent by context
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct RunningAgentKey {
    pub context_type: String,
    pub context_id: String,
}

impl RunningAgentKey {
    pub fn new(context_type: impl Into<String>, context_id: impl Into<String>) -> Self {
        Self {
            context_type: context_type.into(),
            context_id: context_id.into(),
        }
    }
}

/// Information about a running agent process
#[derive(Debug, Clone)]
pub struct RunningAgentInfo {
    /// Process ID of the running agent
    pub pid: u32,
    /// Conversation ID associated with this run
    pub conversation_id: String,
    /// Agent run ID for tracking
    pub agent_run_id: String,
    /// When the agent was started
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Optional worktree path used as the agent's working directory
    pub worktree_path: Option<String>,
    /// Token for cooperative cancellation of the background async task
    pub cancellation_token: Option<CancellationToken>,
    /// Last time a stream event was received (throttled heartbeat, ~5s interval).
    /// Used by the reconciler to distinguish active agents from stale ones.
    pub last_active_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The effective model ID used at spawn time (e.g. "claude-sonnet-4-6").
    pub model: Option<String>,
}

/// Trait for tracking running agent processes.
///
/// Thread-safe registry that maps context keys to process information.
/// Implementations: MemoryRunningAgentRegistry (tests), SqliteRunningAgentRegistry (production).
#[async_trait]
pub trait RunningAgentRegistry: Send + Sync {
    /// Register a running agent process
    async fn register(
        &self,
        key: RunningAgentKey,
        pid: u32,
        conversation_id: String,
        agent_run_id: String,
        worktree_path: Option<String>,
        cancellation_token: Option<CancellationToken>,
    );

    /// Unregister a running agent (called when agent completes or is stopped).
    ///
    /// Only removes the entry if the stored `agent_run_id` matches the caller's, so a
    /// finishing agent cannot accidentally delete a newer agent's slot for the same context.
    async fn unregister(
        &self,
        key: &RunningAgentKey,
        agent_run_id: &str,
    ) -> Option<RunningAgentInfo>;

    /// Get information about a running agent
    async fn get(&self, key: &RunningAgentKey) -> Option<RunningAgentInfo>;

    /// Check if an agent is running for a context
    async fn is_running(&self, key: &RunningAgentKey) -> bool;

    /// Stop a running agent by sending SIGTERM to the process
    async fn stop(&self, key: &RunningAgentKey) -> Result<Option<RunningAgentInfo>, String>;

    /// Get all running agents (for debugging/monitoring)
    async fn list_all(&self) -> Vec<(RunningAgentKey, RunningAgentInfo)>;

    /// Get all running agents for a specific context type.
    ///
    /// Unlike `list_all()`, this method propagates row-parse errors rather than
    /// silently dropping them, so callers can detect corrupted registry state.
    ///
    /// Used by `StartupJobRunner::run()` to snapshot ideation agents BEFORE
    /// `stop_all()` clears the table on app restart.
    async fn list_by_context_type(
        &self,
        context_type: &str,
    ) -> Result<Vec<(RunningAgentKey, RunningAgentInfo)>, String>;

    /// Stop all running agents (for cleanup on shutdown/restart)
    async fn stop_all(&self) -> Vec<RunningAgentKey>;

    /// Update the last_active_at timestamp for a running agent (throttled heartbeat).
    /// Called from the streaming loop every ~5 seconds on any parsed event.
    async fn update_heartbeat(&self, key: &RunningAgentKey, at: chrono::DateTime<chrono::Utc>);

    /// Atomically check-and-register an agent slot.
    ///
    /// If no agent is registered for this key, inserts a placeholder (pid=0) and
    /// returns `Ok(())`. If an agent is already registered, returns `Err` with the
    /// existing agent's info. This prevents the TOCTOU race between separate
    /// `is_running()` + `register()` calls.
    ///
    /// After a successful `try_register`, call `update_agent_process()` once the
    /// CLI process has been spawned. On spawn failure, call `unregister()` to
    /// release the slot.
    async fn try_register(
        &self,
        key: RunningAgentKey,
        conversation_id: String,
        agent_run_id: String,
    ) -> Result<(), RunningAgentInfo>;

    /// Update process details for an already-registered agent.
    ///
    /// Called after the CLI process has been spawned to fill in the real PID,
    /// agent_run_id, worktree path, and cancellation token.
    ///
    /// If the placeholder row was pruned between `try_register` and this call
    /// (TOCTOU race with GC), re-inserts the full registration via INSERT OR REPLACE.
    ///
    /// Returns `Err` only if the DB operation itself fails.
    async fn update_agent_process(
        &self,
        key: &RunningAgentKey,
        pid: u32,
        conversation_id: &str,
        agent_run_id: &str,
        worktree_path: Option<String>,
        cancellation_token: Option<CancellationToken>,
        model: Option<String>,
    ) -> Result<(), String>;

    /// Remove a stale registry entry only if its process is no longer alive.
    ///
    /// Semantics (safe to call unconditionally — Proof Obligation 7):
    /// - If no entry exists for `key`: returns `Ok(None)`.
    /// - If entry exists and `is_process_alive(pid) == true`: returns `Ok(None)` (no-op).
    /// - If entry exists and `is_process_alive(pid) == false`: deletes the row and
    ///   returns `Ok(Some(info))` for audit logging.
    ///
    /// Used by `RecoveryQueueProcessor` before calling `send_message()` to unblock
    /// Gate 2's `try_register()` (Constraint 3 of PDM-172).
    async fn cleanup_stale_entry(
        &self,
        key: &RunningAgentKey,
    ) -> Result<Option<RunningAgentInfo>, String>;
}

/// Check if a process with the given PID is still alive.
///
/// Uses `nix::sys::signal::kill(pid, None)` (signal 0) on Unix — a direct syscall
/// that does NOT spawn a child process. The old `std::process::Command::new("kill")`
/// approach was blocking and starved the tokio runtime when called from async context.
///
/// Returns false if the process does not exist or we lack permissions.
/// Returns false for PID 0, which refers to the process group on Unix
/// and would incorrectly report as alive via signal 0.
pub fn is_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // signal::kill with None sends signal 0 — checks existence without signaling.
        // Ok(()) = process exists, Err(ESRCH) = no such process, other errors = treat as dead.
        signal::kill(Pid::from_raw(pid as i32), None).is_ok()
    }

    #[cfg(windows)]
    {
        let output = std::process::Command::new(resolve_tasklist_cli_path())
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        matches!(output, Ok(result) if result.status.success()
            && !String::from_utf8_lossy(&result.stdout).contains("No tasks"))
    }
}

/// Send SIGTERM to a process and all its children (process tree kill).
///
/// On Unix: first kills children via `pkill -TERM -P <pid>`, then kills the parent
/// via `nix::sys::signal::kill` (non-blocking syscall, no process spawn).
/// This prevents orphaned child processes (e.g. MCP server nodes) from lingering.
pub fn kill_process(pid: u32) {
    if pid <= 1 {
        tracing::warn!(pid, "kill_process: refusing to kill PID {pid} (safety guard)");
        return;
    }
    #[cfg(unix)]
    {
        // Kill children first (MCP server nodes, etc.)
        // pkill is still needed because nix can't enumerate children by parent PID.
        // Uses .spawn() instead of .output() to avoid blocking the tokio runtime —
        // .output() waits synchronously for pkill to finish, starving async timeouts.
        let _ = std::process::Command::new(resolve_pkill_cli_path())
            .args(["-TERM", "-P", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        // Then kill the parent process via direct syscall (non-blocking)
        match signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM) {
            Ok(()) => {}
            Err(nix::errno::Errno::ESRCH) => {
                // Process already exited — fine
            }
            Err(e) => {
                tracing::warn!("Failed to SIGTERM process {}: {}", pid, e);
            }
        }
    }

    #[cfg(windows)]
    {
        // /T flag kills the process tree on Windows
        let _ = std::process::Command::new(resolve_taskkill_cli_path())
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

/// Send SIGKILL immediately to a process and its children — no graceful shutdown.
///
/// Use this for merge cleanup where the process is expendable and graceful
/// shutdown would waste time. SIGKILL cannot be caught or ignored.
///
/// On Unix: kills children via `pkill -KILL -P <pid>`, then kills the parent
/// via direct syscall. Also attempts process group kill.
///
/// Safety: refuses to kill PID 0 (caller's process group) or PID 1 (init).
/// Negative PID in `kill(2)` sends to the entire process group — `kill(-1, SIGKILL)`
/// would kill every process the user owns.
pub fn kill_process_immediate(pid: u32) {
    if pid <= 1 {
        tracing::warn!(pid, "kill_process_immediate: refusing to kill PID {pid} (safety guard)");
        return;
    }
    #[cfg(unix)]
    {
        // Kill children first via pkill (SIGKILL)
        // Uses .spawn() instead of .output() — see kill_process() comment.
        let _ = std::process::Command::new(resolve_pkill_cli_path())
            .args(["-KILL", "-P", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        // Try process group kill (negative PID) — catches children in same PGID
        let _ = signal::kill(Pid::from_raw(-(pid as i32)), Signal::SIGKILL);

        // Kill the parent process directly
        match signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL) {
            Ok(()) => {
                tracing::info!(pid, "kill_process_immediate: sent SIGKILL");
            }
            Err(nix::errno::Errno::ESRCH) => {
                // Already dead
            }
            Err(e) => {
                tracing::warn!(pid, error = %e, "kill_process_immediate: SIGKILL failed");
            }
        }
    }

    #[cfg(windows)]
    {
        let _ = std::process::Command::new(resolve_taskkill_cli_path())
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

fn collect_pids_in_worktree(path: &Path) -> Result<Vec<u32>, String> {
    #[cfg(unix)]
    {
        let output = std::process::Command::new(resolve_lsof_cli_path())
            .args(["-t", "+d", path.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("lsof failure: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "lsof exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut pids = Vec::new();
        for line in text.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                pids.push(pid);
            }
        }
        Ok(pids)
    }

    #[cfg(not(unix))]
    {
        Ok(Vec::new())
    }
}

/// Async version of pid collection using `tokio::process::Command` with `kill_on_drop(true)`.
///
/// Unlike the blocking `std::process::Command::output()`, this function is properly
/// cancellable: when the wrapping `tokio::time::timeout` fires and drops this future,
/// the `lsof` child process is immediately sent SIGKILL via the `kill_on_drop` flag.
/// This prevents lsof from continuing to consume the merge deadline after the timeout.
async fn collect_pids_in_worktree_async(path: &Path) -> Result<Vec<u32>, String> {
    #[cfg(unix)]
    {
        let child = tokio::process::Command::new(resolve_lsof_cli_path())
            .args(["-t", "+d", path.to_str().unwrap_or("")])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("lsof spawn failure: {}", e))?;

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| format!("lsof wait failure: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "lsof exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut pids = Vec::new();
        for line in text.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                pids.push(pid);
            }
        }
        Ok(pids)
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(Vec::new())
    }
}

pub fn kill_worktree_processes(path: &Path) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match collect_pids_in_worktree(&canonical) {
        Ok(pids) => {
            for pid in pids.into_iter().collect::<HashSet<_>>() {
                tracing::info!(
                    pid,
                    worktree = %canonical.display(),
                    "Killing lingering process from worktree"
                );
                kill_process(pid);
            }
        }
        Err(err) => {
            tracing::debug!(
                worktree = %canonical.display(),
                error = %err,
                "Could not enumerate processes under worktree"
            );
        }
    }
}

/// Counts how many times lsof failed to enumerate processes (expected + unexpected).
/// Used to preserve observability signal after the WARN → DEBUG downgrade.
static LSOF_ENUMERATE_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Async version of `kill_worktree_processes` with a cancellable lsof scan.
///
/// Uses `tokio::process::Command` with `kill_on_drop(true)` so that when the
/// configurable timeout fires, the `lsof` child process is immediately sent SIGKILL
/// rather than continuing to run (the old `spawn_blocking` approach left the OS
/// process alive after the Tokio future timed out).
///
/// On timeout, logs a warning and returns — this is non-fatal because agents
/// have already been killed by PID before this point.
pub async fn kill_worktree_processes_async(path: &Path, timeout_secs: u64, immediate_kill: bool) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let display_path = canonical.display().to_string();
    let start = std::time::Instant::now();

    tracing::info!(
        worktree = %display_path,
        timeout_secs,
        "kill_worktree_processes_async: starting lsof scan"
    );

    // Use the async version directly — no spawn_blocking needed.
    // When the timeout fires and drops the future, kill_on_drop sends SIGKILL to lsof.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        collect_pids_in_worktree_async(&canonical),
    )
    .await;

    match result {
        Ok(Ok(pids)) => {
            let unique_pids: Vec<u32> = pids.into_iter().collect::<HashSet<u32>>().into_iter().collect();
            let elapsed_ms = start.elapsed().as_millis();
            tracing::info!(
                worktree = %display_path,
                elapsed_ms,
                pid_count = unique_pids.len(),
                "kill_worktree_processes_async: lsof scan complete"
            );
            for &pid in &unique_pids {
                tracing::info!(
                    pid,
                    worktree = %display_path,
                    immediate_kill,
                    "Killing lingering process from worktree (async)"
                );
                if immediate_kill {
                    kill_process_immediate(pid);
                } else {
                    kill_process(pid);
                }
            }
            // Wait for processes to die; immediate_kill skips SIGTERM grace period.
            let survivors =
                await_process_death(&unique_pids, std::time::Duration::from_secs(5), immediate_kill).await;
            if !survivors.is_empty() {
                tracing::warn!(
                    survivor_pids = ?survivors,
                    worktree = %display_path,
                    survivor_count = survivors.len(),
                    "kill_worktree_processes_async: process(es) survived SIGKILL"
                );
            }
        }
        Ok(Err(err)) => {
            let elapsed_ms = start.elapsed().as_millis();
            let fail_count = LSOF_ENUMERATE_FAIL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            // Unexpected errors (spawn/wait failures, permission denied) stay at WARN.
            // Expected cases (lsof exits non-zero when no files are open) go to DEBUG.
            let is_unexpected = err.contains("spawn failure")
                || err.contains("wait failure")
                || err.to_lowercase().contains("permission");
            if is_unexpected {
                tracing::warn!(
                    worktree = %display_path,
                    elapsed_ms,
                    error = %err,
                    enumerate_fail_count = fail_count,
                    "kill_worktree_processes_async: could not enumerate processes (unexpected)"
                );
            } else {
                tracing::debug!(
                    worktree = %display_path,
                    elapsed_ms,
                    error = %err,
                    enumerate_fail_count = fail_count,
                    "kill_worktree_processes_async: could not enumerate processes"
                );
            }
        }
        Err(_) => {
            let elapsed_ms = start.elapsed().as_millis();
            tracing::warn!(
                worktree = %display_path,
                elapsed_ms,
                timeout_secs,
                "kill_worktree_processes_async: lsof scan timed out — lsof process killed (non-fatal)"
            );
        }
    }

    tracing::info!(
        worktree = %display_path,
        total_elapsed_ms = start.elapsed().as_millis() as u64,
        "kill_worktree_processes_async: completed"
    );
}

/// Waits for processes to die, with optional immediate SIGKILL.
///
/// When `immediate_kill` is false (default): polls every 300ms checking `is_process_alive()`
/// for each PID. If any processes remain alive after `timeout`, escalates to SIGKILL.
///
/// When `immediate_kill` is true (merge cleanup): sends SIGKILL immediately via
/// `kill_process_immediate()`, then does a brief poll for confirmation. This avoids
/// the 5s SIGTERM grace period that wastes time during merge cleanup.
///
/// `is_process_alive()` uses a non-blocking `nix` syscall (signal 0), NOT
/// `std::process::Command` — safe to call from async context without starving tokio.
///
/// Returns any PIDs still alive after all attempts (unkillable processes).
pub(crate) async fn await_process_death(
    pids: &[u32],
    timeout: std::time::Duration,
    immediate_kill: bool,
) -> Vec<u32> {
    let process_ops = ProcessOps::real();
    await_process_death_with_process_ops(pids, timeout, immediate_kill, &process_ops).await
}

async fn await_process_death_with_process_ops(
    pids: &[u32],
    timeout: std::time::Duration,
    immediate_kill: bool,
    process_ops: &ProcessOps,
) -> Vec<u32> {
    if pids.is_empty() {
        return Vec::new();
    }

    if immediate_kill {
        tracing::info!(
            pid_count = pids.len(),
            "await_process_death: immediate SIGKILL mode"
        );

        for &pid in pids {
            process_ops.kill_immediate(pid);
        }

        // Brief wait for SIGKILL to take effect
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let still_alive: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|&p| process_ops.is_alive(p))
            .collect();
        if !still_alive.is_empty() {
            tracing::error!(
                unkillable_pids = ?still_alive,
                "await_process_death: processes survived immediate SIGKILL — unkillable"
            );
        }
        return still_alive;
    }

    tracing::info!(
        pid_count = pids.len(),
        timeout_ms = timeout.as_millis(),
        "await_process_death: waiting for processes to exit after SIGTERM"
    );

    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(300);

    loop {
        let survivors: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|&p| process_ops.is_alive(p))
            .collect();

        if survivors.is_empty() {
            tracing::info!("await_process_death: all processes exited cleanly");
            return Vec::new();
        }

        if start.elapsed() >= timeout {
            tracing::warn!(
                survivor_pids = ?survivors,
                elapsed_ms = start.elapsed().as_millis(),
                "await_process_death: processes did not exit after SIGTERM — escalating to SIGKILL"
            );

            for &pid in &survivors {
                process_ops.kill_immediate(pid);
            }

            // Brief wait for SIGKILL to take effect before final check.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let still_alive: Vec<u32> = survivors
                .iter()
                .copied()
                .filter(|&p| process_ops.is_alive(p))
                .collect();
            if !still_alive.is_empty() {
                tracing::error!(
                    unkillable_pids = ?still_alive,
                    "await_process_death: processes survived SIGKILL — unkillable"
                );
            }
            return still_alive;
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Kill orphaned MCP server processes from previous app sessions.
///
/// Pattern-matches on `node ... ralphx-mcp-server/build/index.js` to catch any
/// leaked processes that escaped PID-based tracking (e.g. app crash before registration).
/// Safe to call on startup — only kills ralphx MCP servers, not user processes.
pub fn kill_orphaned_mcp_servers() -> u32 {
    #[cfg(unix)]
    {
        // Find node processes running our MCP server
        let output = std::process::Command::new(resolve_pgrep_cli_path())
            .args(["-f", "ralphx-mcp-server/build/index.js"])
            .output();

        match output {
            Ok(result) if result.status.success() => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let pids: Vec<&str> = stdout.trim().lines().collect();
                let count = pids.len() as u32;

                for pid_str in &pids {
                    if let Ok(pid) = pid_str.trim().parse::<i32>() {
                        tracing::info!(pid, "Killing orphaned MCP server process");
                        // Use nix syscall instead of spawning a `kill` process —
                        // non-blocking and already imported in this file.
                        let _ = signal::kill(Pid::from_raw(pid), Signal::SIGTERM);
                    }
                }

                if count > 0 {
                    tracing::info!(count, "Killed orphaned ralphx MCP server processes");
                }
                count
            }
            _ => 0, // No matches or pgrep failed
        }
    }

    #[cfg(windows)]
    {
        // Windows: use wmic or tasklist to find matching processes
        // For now, rely on process tree kill (/T flag in taskkill)
        0
    }
}

/// In-memory implementation of RunningAgentRegistry (for tests)
#[derive(Debug, Clone)]
pub struct MemoryRunningAgentRegistry {
    agents: Arc<Mutex<HashMap<RunningAgentKey, RunningAgentInfo>>>,
    process_ops: ProcessOps,
}

impl Default for MemoryRunningAgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryRunningAgentRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::with_process_ops(ProcessOps::real())
    }

    #[doc(hidden)]
    fn with_process_ops(process_ops: ProcessOps) -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
            process_ops,
        }
    }

    /// Insert a running agent entry for test setup.
    /// Uses minimal RunningAgentInfo values — only `is_running` (key presence) matters.
    #[doc(hidden)]
    pub async fn set_running(&self, key: RunningAgentKey) {
        let info = RunningAgentInfo {
            pid: 0,
            conversation_id: "test-conversation".to_string(),
            agent_run_id: "test-agent-run".to_string(),
            started_at: chrono::Utc::now(),
            worktree_path: None,
            cancellation_token: None,
            last_active_at: None,
            model: None,
        };
        let mut agents = self.agents.lock().await;
        agents.insert(key, info);
    }
}

#[async_trait]
impl RunningAgentRegistry for MemoryRunningAgentRegistry {
    async fn register(
        &self,
        key: RunningAgentKey,
        pid: u32,
        conversation_id: String,
        agent_run_id: String,
        worktree_path: Option<String>,
        cancellation_token: Option<CancellationToken>,
    ) {
        let info = RunningAgentInfo {
            pid,
            conversation_id,
            agent_run_id,
            started_at: chrono::Utc::now(),
            worktree_path,
            cancellation_token,
            last_active_at: None,
            model: None,
        };
        let mut agents = self.agents.lock().await;

        // Stop orphaned agent if one already exists for this key
        if let Some(existing) = agents.get(&key) {
            let old_pid = existing.pid;
            if old_pid != pid && self.process_ops.is_alive(old_pid) {
                tracing::warn!(
                    old_pid,
                    new_pid = pid,
                    context_type = %key.context_type,
                    context_id = %key.context_id,
                    "Detected orphaned agent process — stopping before re-registration"
                );
                if let Some(ref token) = existing.cancellation_token {
                    token.cancel();
                }
                self.process_ops.kill(old_pid);
            }
        }

        agents.insert(key, info);
    }

    async fn unregister(
        &self,
        key: &RunningAgentKey,
        agent_run_id: &str,
    ) -> Option<RunningAgentInfo> {
        let mut agents = self.agents.lock().await;
        if agents.get(key).map(|i| i.agent_run_id.as_str()) == Some(agent_run_id) {
            agents.remove(key)
        } else {
            None
        }
    }

    async fn get(&self, key: &RunningAgentKey) -> Option<RunningAgentInfo> {
        let agents = self.agents.lock().await;
        agents.get(key).cloned()
    }

    async fn is_running(&self, key: &RunningAgentKey) -> bool {
        let agents = self.agents.lock().await;
        agents.contains_key(key)
    }

    async fn stop(&self, key: &RunningAgentKey) -> Result<Option<RunningAgentInfo>, String> {
        let agent_run_id = {
            let agents = self.agents.lock().await;
            agents
                .get(key)
                .map(|i| i.agent_run_id.clone())
                .unwrap_or_default()
        };
        let info = self.unregister(key, &agent_run_id).await;

        if let Some(ref agent_info) = info {
            // Cancel the async task before killing the process
            if let Some(ref token) = agent_info.cancellation_token {
                token.cancel();
            }
            self.process_ops.kill(agent_info.pid);
        }

        Ok(info)
    }

    async fn list_all(&self) -> Vec<(RunningAgentKey, RunningAgentInfo)> {
        let agents = self.agents.lock().await;
        agents.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    async fn list_by_context_type(
        &self,
        context_type: &str,
    ) -> Result<Vec<(RunningAgentKey, RunningAgentInfo)>, String> {
        let agents = self.agents.lock().await;
        let results = agents
            .iter()
            .filter(|(k, _)| k.context_type == context_type)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(results)
    }

    async fn stop_all(&self) -> Vec<RunningAgentKey> {
        let keys: Vec<RunningAgentKey> = {
            let agents = self.agents.lock().await;
            agents.keys().cloned().collect()
        };

        let mut stopped = Vec::new();
        for key in keys {
            if self.stop(&key).await.is_ok() {
                stopped.push(key);
            }
        }
        stopped
    }

    async fn update_heartbeat(&self, key: &RunningAgentKey, at: chrono::DateTime<chrono::Utc>) {
        let mut agents = self.agents.lock().await;
        if let Some(info) = agents.get_mut(key) {
            info.last_active_at = Some(at);
        }
    }

    async fn try_register(
        &self,
        key: RunningAgentKey,
        conversation_id: String,
        agent_run_id: String,
    ) -> Result<(), RunningAgentInfo> {
        let mut agents = self.agents.lock().await;
        if let Some(existing) = agents.get(&key) {
            return Err(existing.clone());
        }
        agents.insert(
            key,
            RunningAgentInfo {
                pid: 0,
                conversation_id,
                agent_run_id,
                started_at: chrono::Utc::now(),
                worktree_path: None,
                cancellation_token: None,
                last_active_at: None,
                model: None,
            },
        );
        Ok(())
    }

    async fn update_agent_process(
        &self,
        key: &RunningAgentKey,
        pid: u32,
        conversation_id: &str,
        agent_run_id: &str,
        worktree_path: Option<String>,
        cancellation_token: Option<CancellationToken>,
        model: Option<String>,
    ) -> Result<(), String> {
        let mut agents = self.agents.lock().await;
        if let Some(info) = agents.get_mut(key) {
            info.pid = pid;
            info.agent_run_id = agent_run_id.to_string();
            info.worktree_path = worktree_path;
            info.cancellation_token = cancellation_token;
            info.model = model;
        } else {
            tracing::warn!(
                context_type = %key.context_type,
                context_id = %key.context_id,
                pid,
                "update_agent_process: entry pruned — re-inserting full registration"
            );
            agents.insert(
                key.clone(),
                RunningAgentInfo {
                    pid,
                    conversation_id: conversation_id.to_string(),
                    agent_run_id: agent_run_id.to_string(),
                    started_at: chrono::Utc::now(),
                    worktree_path,
                    cancellation_token,
                    last_active_at: None,
                    model,
                },
            );
        }
        Ok(())
    }

    async fn cleanup_stale_entry(
        &self,
        key: &RunningAgentKey,
    ) -> Result<Option<RunningAgentInfo>, String> {
        let info = {
            let agents = self.agents.lock().await;
            agents.get(key).cloned()
        };
        let info = match info {
            Some(i) => i,
            None => return Ok(None),
        };

        // Only remove if the process is actually dead (Proof Obligation 7)
        if self.process_ops.is_alive(info.pid) {
            tracing::debug!(
                pid = info.pid,
                context_type = %key.context_type,
                context_id = %key.context_id,
                "cleanup_stale_entry: process is still alive, skipping"
            );
            return Ok(None);
        }

        // Process is dead — remove the entry
        let mut agents = self.agents.lock().await;
        let removed = agents.remove(key);
        Ok(removed)
    }
}

#[cfg(test)]
#[path = "running_agent_registry_tests.rs"]
mod tests;
