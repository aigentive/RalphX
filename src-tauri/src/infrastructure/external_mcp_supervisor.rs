/// External MCP supervisor — manages the lifecycle of an external Node.js MCP server process.
///
/// Responsibilities:
/// - Spawn the external MCP process with setsid (new process group)
/// - Monitor health via HTTP `/health` and `/ready` endpoints
/// - Restart on crash up to `max_restart_attempts`
/// - Graceful shutdown via SIGTERM → SIGKILL
/// - Orphan cleanup via PID file
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::process::{Child, Command};
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing;

use crate::infrastructure::agents::claude::{
    bounded_external_mcp_shutdown_grace_ms, ExternalMcpConfig, MAX_EXTERNAL_MCP_SHUTDOWN_GRACE_MS,
};
use crate::infrastructure::tool_paths::{resolve_lsof_cli_path, resolve_ps_cli_path};
use crate::utils::backend_endpoint::backend_http_base_url;

pub const TAURI_MCP_BYPASS_TOKEN_ENV: &str = "RALPHX_TAURI_MCP_BYPASS_TOKEN";
const PROCESS_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminateOutcome {
    NoProcess,
    ExitedAfterTerm,
    Killed,
}

pub(crate) fn register_child_pid(slot: &StdMutex<Option<u32>>, pid: u32) {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(pid);
}

pub(crate) fn clear_child_pid(slot: &StdMutex<Option<u32>>) {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

pub(crate) fn current_child_pid(slot: &StdMutex<Option<u32>>) -> Option<u32> {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn terminate_process_group_with(
    pid: Option<u32>,
    grace: Duration,
    poll_interval: Duration,
    send_term: impl Fn(i32),
    send_kill: impl Fn(i32),
    is_alive: impl Fn(i32) -> bool,
) -> TerminateOutcome {
    let Some(pid) = pid
        .filter(|pid| *pid > 1)
        .and_then(|pid| i32::try_from(pid).ok())
    else {
        return TerminateOutcome::NoProcess;
    };

    send_term(pid);
    let grace = grace.min(Duration::from_millis(MAX_EXTERNAL_MCP_SHUTDOWN_GRACE_MS));
    let deadline = Instant::now() + grace;
    loop {
        if !is_alive(pid) {
            return TerminateOutcome::ExitedAfterTerm;
        }

        let now = Instant::now();
        if now >= deadline {
            break;
        }
        std::thread::sleep(poll_interval.min(deadline.saturating_duration_since(now)));
    }

    send_kill(pid);
    TerminateOutcome::Killed
}

pub(crate) fn terminate_process_group(pid: Option<u32>, grace: Duration) -> TerminateOutcome {
    terminate_process_group_with(
        pid,
        grace,
        PROCESS_EXIT_POLL_INTERVAL,
        |pid| {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM);
        },
        |pid| {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
        },
        process_group_exists,
    )
}

pub fn ensure_tauri_mcp_bypass_token() -> String {
    if let Ok(token) = std::env::var(TAURI_MCP_BYPASS_TOKEN_ENV) {
        if !token.trim().is_empty() {
            return token;
        }
    }

    let token = format!("rx_tauri_{}", uuid::Uuid::new_v4().simple());
    std::env::set_var(TAURI_MCP_BYPASS_TOKEN_ENV, &token);
    token
}

// ── Environment detection ─────────────────────────────────────────────────

fn is_test_environment() -> bool {
    if cfg!(test) {
        return true;
    }
    if std::env::var("RUST_TEST_THREADS").is_ok() {
        return true;
    }
    if let Ok(v) = std::env::var("RALPHX_TEST_MODE") {
        return v == "1" || v.eq_ignore_ascii_case("true");
    }
    false
}

/// Exposed for test modules that cannot access the private fn directly.
#[cfg(test)]
pub(crate) fn is_test_environment_for_test() -> bool {
    is_test_environment()
}

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum HealthCheckResult {
    Ready,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalMcpReadinessState {
    Disabled,
    Starting,
    Ready,
    Degraded,
    Failed,
}

pub(crate) fn stderr_indicates_address_in_use(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        line.contains("EADDRINUSE") || line.to_ascii_lowercase().contains("address already in use")
    })
}

/// Frontend event emitted on `external-mcp:status`.
#[derive(Serialize, Clone)]
pub struct ExternalMcpEvent {
    pub status: &'static str, // "started"|"stopped"|"crashed"|"restarting"|"failed"|"degraded"
    pub port: u16,
    pub message: Option<String>,
}

// ── ExternalMcpHandle ─────────────────────────────────────────────────────

/// Singleton handle to the running supervisor.  Stored in AppState.
pub struct ExternalMcpHandle {
    inner: OnceLock<Arc<ExternalMcpSupervisor>>,
}

impl ExternalMcpHandle {
    pub fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    pub fn set(
        &self,
        supervisor: Arc<ExternalMcpSupervisor>,
    ) -> Result<(), Arc<ExternalMcpSupervisor>> {
        self.inner.set(supervisor)
    }

    pub fn get(&self) -> Option<&Arc<ExternalMcpSupervisor>> {
        self.inner.get()
    }

    pub fn readiness(&self) -> ExternalMcpReadinessState {
        self.get()
            .map(|supervisor| supervisor.readiness())
            .unwrap_or(ExternalMcpReadinessState::Disabled)
    }

    pub async fn await_ready(&self, timeout: Duration) -> Result<(), String> {
        let supervisor = self
            .get()
            .ok_or_else(|| "External MCP transport is disabled".to_string())?;
        supervisor.await_ready(timeout).await
    }
}

impl Default for ExternalMcpHandle {
    fn default() -> Self {
        Self::new()
    }
}

// ── ExternalMcpSupervisor ─────────────────────────────────────────────────

pub struct ExternalMcpSupervisor {
    child: Arc<Mutex<Option<Child>>>,
    child_pid: Arc<StdMutex<Option<u32>>>,
    io_handles: Mutex<Vec<JoinHandle<()>>>,
    cancel: CancellationToken,
    config: ExternalMcpConfig,
    app_handle: AppHandle,
    app_data_dir: PathBuf,
    readiness_tx: watch::Sender<ExternalMcpReadinessState>,
}

impl ExternalMcpSupervisor {
    pub fn new(config: ExternalMcpConfig, app_handle: AppHandle, app_data_dir: PathBuf) -> Self {
        let (readiness_tx, _) = watch::channel(ExternalMcpReadinessState::Starting);
        Self {
            child: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(StdMutex::new(None)),
            io_handles: Mutex::new(Vec::new()),
            cancel: CancellationToken::new(),
            config,
            app_handle,
            app_data_dir,
            readiness_tx,
        }
    }

    // ── Public API ────────────────────────────────────────────────────────

    pub async fn start(
        self: Arc<Self>,
        node_path: PathBuf,
        entry_path: PathBuf,
    ) -> Result<(), String> {
        self.set_readiness(ExternalMcpReadinessState::Starting);
        if is_test_environment() {
            tracing::info!("Skipping external MCP supervisor start (test environment)");
            self.set_readiness(ExternalMcpReadinessState::Ready);
            return Ok(());
        }

        self.cleanup_orphan().await;

        let this = Arc::clone(&self);
        tokio::spawn(async move {
            this.run_supervisor_with_panic_guard(node_path, entry_path)
                .await;
        });

        Ok(())
    }

    /// Fully synchronous, bounded teardown for `RunEvent::Exit`.
    ///
    /// INCIDENT GUARD: this must remain a non-async fn and must not acquire
    /// `self.child` or `self.io_handles`; Tauri's runtime may no longer poll
    /// tasks while exit cleanup is running.
    pub fn shutdown_blocking(&self) {
        self.cancel.cancel();

        // Only signal the PID registered by this process. A PID-file fallback can
        // be stale or corrupted and would make killpg target an unrelated group.
        let pid = self.current_child_pid();
        let grace = Duration::from_millis(bounded_external_mcp_shutdown_grace_ms(
            self.config.shutdown_grace_ms,
        ));
        let _ = terminate_process_group(pid, grace);

        self.clear_child_pid();
        self.remove_pid_file();
        self.set_readiness(ExternalMcpReadinessState::Disabled);
        self.emit_event("stopped", None);
    }

    pub fn readiness(&self) -> ExternalMcpReadinessState {
        *self.readiness_tx.borrow()
    }

    pub async fn await_ready(&self, timeout: Duration) -> Result<(), String> {
        let mut readiness = self.readiness_tx.subscribe();
        tokio::time::timeout(timeout, async {
            loop {
                match *readiness.borrow() {
                    ExternalMcpReadinessState::Ready => return Ok(()),
                    ExternalMcpReadinessState::Degraded => {
                        return Err("External MCP transport is degraded".to_string())
                    }
                    ExternalMcpReadinessState::Failed => {
                        return Err("External MCP transport failed to start".to_string())
                    }
                    ExternalMcpReadinessState::Disabled => {
                        return Err("External MCP transport is disabled".to_string())
                    }
                    ExternalMcpReadinessState::Starting => {}
                }
                readiness
                    .changed()
                    .await
                    .map_err(|_| "External MCP readiness monitor stopped".to_string())?;
            }
        })
        .await
        .map_err(|_| "Timed out waiting for external MCP transport readiness".to_string())?
    }

    // ── Internal — supervisor lifecycle ──────────────────────────────────

    async fn run_supervisor_with_panic_guard(
        self: Arc<Self>,
        node_path: PathBuf,
        entry_path: PathBuf,
    ) {
        let this = Arc::clone(&self);
        let np = node_path.clone();
        let ep = entry_path.clone();
        let handle = tokio::spawn(async move {
            this.supervisor_loop(np, ep).await;
        });
        match handle.await {
            Ok(()) => {}
            Err(e) if e.is_panic() => {
                tracing::error!("External MCP supervisor panicked: {:?}", e);
                // One restart attempt after panic — do not reset attempt counter
                self.supervisor_loop(node_path, entry_path).await;
            }
            Err(e) => tracing::error!("Supervisor task cancelled: {:?}", e),
        }
    }

    async fn supervisor_loop(self: Arc<Self>, node_path: PathBuf, entry_path: PathBuf) {
        let mut attempts = 0u32;
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    tracing::info!("External MCP supervisor cancelled");
                    return;
                }
                _ = self.run_once(&node_path, &entry_path, &mut attempts) => {}
            }
        }
    }

    async fn run_once(&self, node_path: &Path, entry_path: &Path, attempts: &mut u32) {
        let spawn_start = std::time::Instant::now();

        let child = match self.spawn_process(node_path, entry_path).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to spawn external MCP process: {}", e);
                *attempts += 1;
                if *attempts >= self.config.max_restart_attempts {
                    self.set_readiness(ExternalMcpReadinessState::Failed);
                    self.emit_event("failed", Some(format!("Failed to spawn: {}", e)));
                    self.cancel.cancel();
                    return;
                }
                self.emit_event(
                    "restarting",
                    Some(format!("Spawn failed, attempt {}", attempts)),
                );
                self.set_readiness(ExternalMcpReadinessState::Starting);
                tokio::time::sleep(Duration::from_millis(self.config.restart_delay_ms)).await;
                return;
            }
        };

        let pid = child.id();
        if let Some(pid_val) = pid {
            self.register_child_pid(pid_val);
            self.write_pid_file(pid_val);
        }

        // Collect stderr lines for EADDRINUSE detection
        let stderr_lines = Arc::new(Mutex::new(Vec::<String>::new()));

        // Pipe stdout/stderr to tracing and collect stderr
        let child = self
            .attach_io_handles(child, Arc::clone(&stderr_lines))
            .await;

        *self.child.lock().await = Some(child);

        // Health check
        let health_check = self.health_check().await;
        if matches!(
            health_check,
            HealthCheckResult::Ready | HealthCheckResult::Degraded
        ) && self
            .detect_startup_bind_conflict(&stderr_lines, spawn_start, attempts)
            .await
        {
            return;
        }

        match health_check {
            HealthCheckResult::Ready => {
                tracing::info!("External MCP server is ready on port {}", self.config.port);
                self.emit_event("started", None);
                self.set_readiness(ExternalMcpReadinessState::Ready);
                *attempts = 0; // reset counter on successful start
            }
            HealthCheckResult::Degraded => {
                tracing::warn!("External MCP server started in degraded state");
                self.emit_event(
                    "degraded",
                    Some("Server responding but not fully ready".to_string()),
                );
                self.set_readiness(ExternalMcpReadinessState::Degraded);
                *attempts = 0;
            }
            HealthCheckResult::Failed => {
                // Check for EADDRINUSE before counting as restart attempt
                if self.stderr_has_address_in_use(&stderr_lines).await {
                    self.fail_port_in_use().await;
                    return;
                }

                tracing::warn!("External MCP health check failed");
                *attempts += 1;
                if *attempts >= self.config.max_restart_attempts {
                    self.set_readiness(ExternalMcpReadinessState::Failed);
                    self.emit_event(
                        "failed",
                        Some("Health check failed after max attempts".to_string()),
                    );
                    self.cancel.cancel();
                    return;
                }
                self.emit_event(
                    "restarting",
                    Some(format!("Health check failed, attempt {}", attempts)),
                );
                self.set_readiness(ExternalMcpReadinessState::Starting);
                self.kill_current().await;
                tokio::time::sleep(Duration::from_millis(self.config.restart_delay_ms)).await;
                return;
            }
        }

        // Take ownership before the lifetime wait so the tokio mutex is never
        // held across an unbounded await. If cancellation drops this future,
        // exit cleanup terminates the process through the independent PID slot.
        let taken = self.child.lock().await.take();
        let exit_status = match taken {
            Some(mut child) => child.wait().await.ok(),
            None => None,
        };

        self.clear_child_pid();
        self.remove_pid_file();

        if self.cancel.is_cancelled() {
            return;
        }

        if self.stderr_has_address_in_use(&stderr_lines).await {
            self.fail_port_in_use().await;
            return;
        }

        let exit_code = exit_status.and_then(|s| s.code());
        self.handle_process_exit(spawn_start, exit_code, attempts)
            .await;
    }

    async fn detect_startup_bind_conflict(
        &self,
        stderr_lines: &Arc<Mutex<Vec<String>>>,
        spawn_start: std::time::Instant,
        attempts: &mut u32,
    ) -> bool {
        tokio::time::sleep(Duration::from_millis(350)).await;

        if self.stderr_has_address_in_use(stderr_lines).await {
            self.fail_port_in_use().await;
            return true;
        }

        let exit_status = {
            let mut guard = self.child.lock().await;
            let status = if let Some(ref mut child) = *guard {
                child.try_wait().ok().flatten()
            } else {
                None
            };
            if status.is_some() {
                *guard = None;
            }
            status
        };

        if let Some(status) = exit_status {
            self.clear_child_pid();
            self.remove_pid_file();
            if self.stderr_has_address_in_use(stderr_lines).await {
                self.fail_port_in_use().await;
                return true;
            }
            self.handle_process_exit(spawn_start, status.code(), attempts)
                .await;
            return true;
        }

        false
    }

    async fn stderr_has_address_in_use(&self, stderr_lines: &Arc<Mutex<Vec<String>>>) -> bool {
        let lines = stderr_lines.lock().await;
        stderr_indicates_address_in_use(&lines)
    }

    async fn fail_port_in_use(&self) {
        tracing::error!(
            "External MCP port {} already in use; stop the conflicting process",
            self.config.port
        );
        self.emit_event(
            "failed",
            Some(format!(
                "Port {} already in use; stop the conflicting process first",
                self.config.port
            )),
        );
        self.set_readiness(ExternalMcpReadinessState::Failed);
        self.kill_current().await;
        self.cancel.cancel();
    }

    async fn handle_process_exit(
        &self,
        spawn_start: std::time::Instant,
        exit_code: Option<i32>,
        attempts: &mut u32,
    ) {
        let runtime = spawn_start.elapsed();
        tracing::warn!(
            "External MCP process exited after {:?} (code: {:?})",
            runtime,
            exit_code
        );
        self.emit_event(
            "crashed",
            Some(format!("Process exited (code: {:?})", exit_code)),
        );

        *attempts += 1;
        if *attempts >= self.config.max_restart_attempts {
            self.set_readiness(ExternalMcpReadinessState::Failed);
            self.emit_event("failed", Some("Max restart attempts reached".to_string()));
            self.cancel.cancel();
            return;
        }

        self.emit_event(
            "restarting",
            Some(format!("Restarting, attempt {}", attempts)),
        );
        self.set_readiness(ExternalMcpReadinessState::Starting);
        tokio::time::sleep(Duration::from_millis(self.config.restart_delay_ms)).await;
    }

    // ── Process management ────────────────────────────────────────────────

    async fn spawn_process(
        &self,
        node_path: &Path,
        entry_path: &Path,
    ) -> Result<Child, std::io::Error> {
        let mut cmd = Command::new(node_path);
        cmd.arg(entry_path);

        cmd.env("EXTERNAL_MCP_PORT", self.config.port.to_string());
        cmd.env("EXTERNAL_MCP_HOST", &self.config.host);
        cmd.env("RALPHX_BACKEND_URL", backend_http_base_url());
        cmd.env(TAURI_MCP_BYPASS_TOKEN_ENV, ensure_tauri_mcp_bypass_token());
        if let Some(token) = &self.config.auth_token {
            cmd.env("EXTERNAL_MCP_AUTH_TOKEN", token);
        }
        crate::infrastructure::subprocess_env_policy::github_cli_env_policy()
            .apply_to_tokio_command(&mut cmd);

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // setsid — detach child into its own process group so killpg works correctly
        #[cfg(unix)]
        // SAFETY: setsid() is async-signal-safe and idempotent; failure is logged but non-fatal.
        unsafe {
            cmd.pre_exec(|| {
                match nix::unistd::setsid() {
                    Ok(_) => {}
                    Err(e) => eprintln!(
                        "setsid() failed: {} — grandchild cleanup may be incomplete",
                        e
                    ),
                }
                Ok(())
            });
        }

        cmd.spawn()
    }

    async fn attach_io_handles(
        &self,
        mut child: Child,
        stderr_lines: Arc<Mutex<Vec<String>>>,
    ) -> Child {
        use tokio::io::{AsyncBufReadExt, BufReader};

        if let Some(stdout) = child.stdout.take() {
            let handle = tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    tracing::debug!(target: "external_mcp", "[stdout] {}", line);
                }
            });
            self.io_handles.lock().await.push(handle);
        }

        if let Some(stderr) = child.stderr.take() {
            let handle = tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    tracing::warn!(target: "external_mcp", "[stderr] {}", line);
                    let mut lines = stderr_lines.lock().await;
                    if lines.len() < 100 {
                        lines.push(line);
                    }
                }
            });
            self.io_handles.lock().await.push(handle);
        }

        child
    }

    async fn kill_current(&self) {
        let mut guard = self.child.lock().await;
        if let Some(ref mut child) = *guard {
            if let Some(pid) = child.id() {
                let pgid = Pid::from_raw(pid as i32);
                let _ = killpg(pgid, Signal::SIGTERM);
                match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                    Ok(_) => {}
                    Err(_) => {
                        let _ = killpg(pgid, Signal::SIGKILL);
                        let _ = child.wait().await;
                    }
                }
            }
        }
        *guard = None;
        self.clear_child_pid();
    }

    // ── Health check ──────────────────────────────────────────────────────

    async fn health_check(&self) -> HealthCheckResult {
        let start = std::time::Instant::now();
        let total_timeout = Duration::from_secs(15);

        // Phase 1: wait for /health → 200
        loop {
            if start.elapsed() > total_timeout {
                return HealthCheckResult::Failed;
            }
            match http_get_status(&self.config.host, self.config.port, "/health").await {
                Ok(200) => break,
                Ok(status) => tracing::debug!("Health check /health returned {}", status),
                Err(e) => tracing::debug!("Health check /health error: {}", e),
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Phase 2: /ready — 5x consecutive 503 → Degraded, 200 → Ready
        let mut consecutive_503 = 0u32;
        for _ in 0..20 {
            if start.elapsed() > total_timeout {
                return HealthCheckResult::Failed;
            }
            match http_get_status(&self.config.host, self.config.port, "/ready").await {
                Ok(200) => return HealthCheckResult::Ready,
                Ok(503) => {
                    consecutive_503 += 1;
                    if consecutive_503 >= 5 {
                        return HealthCheckResult::Degraded;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Ok(status) => {
                    tracing::warn!("Unexpected /ready status: {}", status);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    tracing::debug!("Ready check error: {}", e);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        HealthCheckResult::Failed
    }

    // ── Events ────────────────────────────────────────────────────────────

    fn emit_event(&self, status: &'static str, message: Option<String>) {
        let event = ExternalMcpEvent {
            status,
            port: self.config.port,
            message,
        };
        if let Err(e) = self.app_handle.emit("external-mcp:status", event) {
            tracing::warn!("Failed to emit external MCP event: {}", e);
        }
    }

    fn set_readiness(&self, state: ExternalMcpReadinessState) {
        self.readiness_tx.send_replace(state);
    }

    // ── PID file ──────────────────────────────────────────────────────────

    fn pid_file_path(&self) -> PathBuf {
        external_mcp_pid_file_path(&self.app_data_dir, self.config.port)
    }

    fn write_pid_file(&self, pid: u32) {
        if let Err(e) = std::fs::write(self.pid_file_path(), pid.to_string()) {
            tracing::warn!("Failed to write PID file: {}", e);
        }
    }

    fn remove_pid_file(&self) {
        let _ = std::fs::remove_file(self.pid_file_path());
    }

    fn register_child_pid(&self, pid: u32) {
        register_child_pid(&self.child_pid, pid);
    }

    fn clear_child_pid(&self) {
        clear_child_pid(&self.child_pid);
    }

    fn current_child_pid(&self) -> Option<u32> {
        current_child_pid(&self.child_pid)
    }

    pub(crate) async fn cleanup_orphan(&self) {
        let pid_path = self.pid_file_path();
        if let Ok(contents) = std::fs::read_to_string(&pid_path) {
            if let Ok(pid) = contents.trim().parse::<i32>() {
                if is_external_mcp_process(pid, self.config.port) {
                    tracing::warn!("Found orphaned external MCP process (PID {}), killing", pid);
                    let pgid = Pid::from_raw(pid);
                    let _ = killpg(pgid, Signal::SIGTERM);
                    let _ = nix::sys::signal::kill(Pid::from_raw(pid), Signal::SIGTERM);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if process_exists(pid) {
                        let _ = killpg(pgid, Signal::SIGKILL);
                        let _ = nix::sys::signal::kill(Pid::from_raw(pid), Signal::SIGKILL);
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }
}

// ── Free functions ────────────────────────────────────────────────────────

/// Raw HTTP GET returning the response status code.
/// Uses tokio TcpStream + hand-crafted HTTP/1.0 request — no reqwest dependency.
async fn http_get_status(host: &str, port: u16, path: &str) -> Result<u16, std::io::Error> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("{}:{}", host, port);
    let mut stream = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timeout"))??;

    let request = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );
    tokio::time::timeout(Duration::from_secs(2), stream.write_all(request.as_bytes()))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "write timeout"))??;

    let mut buf = [0u8; 512];
    let n = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "read timeout"))??;

    let response = std::str::from_utf8(&buf[..n]).unwrap_or("");
    // Parse "HTTP/1.x NNN ..."
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    Ok(status)
}

pub(crate) fn external_mcp_pid_file_name(port: u16) -> String {
    format!("external_mcp_{port}.pid")
}

pub(crate) fn external_mcp_pid_file_path(app_data_dir: &Path, port: u16) -> PathBuf {
    app_data_dir.join(external_mcp_pid_file_name(port))
}

pub(crate) fn command_matches_external_mcp_process_for_port(command: &str, port: u16) -> bool {
    let is_external_mcp = command_mentions_external_mcp_runtime(command);
    if !is_external_mcp {
        return false;
    }

    let external_port = format!("EXTERNAL_MCP_PORT={port}");
    let ralphx_port = format!("RALPHX_EXTERNAL_MCP_PORT={port}");
    command.contains(&external_port) || command.contains(&ralphx_port)
}

pub(crate) fn command_mentions_external_mcp_runtime(command: &str) -> bool {
    command.contains("ralphx-external-mcp") || command.contains("ralphx_external_mcp")
}

pub(crate) fn external_mcp_process_matches_expected_port(
    command: &str,
    pid: i32,
    expected_port: u16,
) -> bool {
    let is_external_mcp = command_mentions_external_mcp_runtime(command);
    is_external_mcp
        && (command_matches_external_mcp_process_for_port(command, expected_port)
            || process_listens_on_port(pid, expected_port))
}

/// Check whether a PID belongs to this supervisor's external-MCP Node process.
pub(crate) fn is_external_mcp_process(pid: i32, expected_port: u16) -> bool {
    if pid <= 0 {
        return false;
    }

    let pid_arg = pid.to_string();
    let output = std::process::Command::new(resolve_ps_cli_path())
        .args(["eww", "-p", pid_arg.as_str(), "-o", "command="])
        .output();

    if let Ok(o) = output {
        let cmd = String::from_utf8_lossy(&o.stdout);
        return external_mcp_process_matches_expected_port(&cmd, pid, expected_port);
    }

    false
}

pub(crate) fn process_listens_on_port(pid: i32, expected_port: u16) -> bool {
    if pid <= 0 {
        return false;
    }

    let pid_arg = pid.to_string();
    let port_arg = format!("-iTCP:{expected_port}");
    let output = std::process::Command::new(resolve_lsof_cli_path())
        .args([
            "-nP",
            "-a",
            "-p",
            pid_arg.as_str(),
            port_arg.as_str(),
            "-sTCP:LISTEN",
        ])
        .output();

    match output {
        Ok(o) => o.status.success() && !o.stdout.is_empty(),
        Err(_) => false,
    }
}

/// Returns true if a process with the given PID still exists.
fn process_exists(pid: i32) -> bool {
    // POSIX: kill(pid, 0) → Ok if process exists
    use nix::sys::signal::kill;
    kill(Pid::from_raw(pid), None).is_ok()
}

pub(crate) fn process_group_exists(pgid: i32) -> bool {
    use nix::errno::Errno;
    use nix::sys::signal::kill;

    match kill(Pid::from_raw(-pgid), None) {
        Ok(()) => true,
        Err(Errno::ESRCH) => false,
        Err(_) => true,
    }
}
