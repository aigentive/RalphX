/// External MCP supervisor — manages the lifecycle of an external Node.js MCP server process.
///
/// Responsibilities:
/// - Spawn the external MCP process with setsid (new process group)
/// - Monitor health via HTTP `/health` and `/ready` endpoints
/// - Restart on crash up to `max_restart_attempts`
/// - Graceful shutdown via SIGTERM → SIGKILL
/// - Orphan cleanup via PID file
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing;

use crate::infrastructure::agents::claude::ExternalMcpConfig;
use crate::infrastructure::tool_paths::resolve_ps_cli_path;
use crate::utils::backend_endpoint::backend_http_base_url;

pub const TAURI_MCP_BYPASS_TOKEN_ENV: &str = "RALPHX_TAURI_MCP_BYPASS_TOKEN";

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
}

impl Default for ExternalMcpHandle {
    fn default() -> Self {
        Self::new()
    }
}

// ── ExternalMcpSupervisor ─────────────────────────────────────────────────

pub struct ExternalMcpSupervisor {
    child: Arc<Mutex<Option<Child>>>,
    io_handles: Mutex<Vec<JoinHandle<()>>>,
    cancel: CancellationToken,
    config: ExternalMcpConfig,
    app_handle: AppHandle,
    app_data_dir: PathBuf,
}

impl ExternalMcpSupervisor {
    pub fn new(config: ExternalMcpConfig, app_handle: AppHandle, app_data_dir: PathBuf) -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            io_handles: Mutex::new(Vec::new()),
            cancel: CancellationToken::new(),
            config,
            app_handle,
            app_data_dir,
        }
    }

    // ── Public API ────────────────────────────────────────────────────────

    pub async fn start(
        self: Arc<Self>,
        node_path: PathBuf,
        entry_path: PathBuf,
    ) -> Result<(), String> {
        if is_test_environment() {
            tracing::info!("Skipping external MCP supervisor start (test environment)");
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

    pub async fn shutdown(&self) {
        self.cancel.cancel();

        let mut child_guard = self.child.lock().await;
        if let Some(ref mut child) = *child_guard {
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
        *child_guard = None;
        drop(child_guard);

        let mut handles = self.io_handles.lock().await;
        for h in handles.drain(..) {
            h.abort();
        }
        drop(handles);

        self.remove_pid_file();
        self.emit_event("stopped", None);
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
                    self.emit_event("failed", Some(format!("Failed to spawn: {}", e)));
                    self.cancel.cancel();
                    return;
                }
                self.emit_event(
                    "restarting",
                    Some(format!("Spawn failed, attempt {}", attempts)),
                );
                tokio::time::sleep(Duration::from_millis(self.config.restart_delay_ms)).await;
                return;
            }
        };

        let pid = child.id();
        if let Some(pid_val) = pid {
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
                *attempts = 0; // reset counter on successful start
            }
            HealthCheckResult::Degraded => {
                tracing::warn!("External MCP server started in degraded state");
                self.emit_event(
                    "degraded",
                    Some("Server responding but not fully ready".to_string()),
                );
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
                self.kill_current().await;
                tokio::time::sleep(Duration::from_millis(self.config.restart_delay_ms)).await;
                return;
            }
        }

        // Wait for process to exit
        let exit_status = {
            let mut guard = self.child.lock().await;
            if let Some(ref mut child) = *guard {
                child.wait().await.ok()
            } else {
                None
            }
        };

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
            self.emit_event("failed", Some("Max restart attempts reached".to_string()));
            self.cancel.cancel();
            return;
        }

        self.emit_event(
            "restarting",
            Some(format!("Restarting, attempt {}", attempts)),
        );
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

    // ── PID file ──────────────────────────────────────────────────────────

    fn pid_file_path(&self) -> PathBuf {
        self.app_data_dir.join("external_mcp.pid")
    }

    fn write_pid_file(&self, pid: u32) {
        if let Err(e) = std::fs::write(self.pid_file_path(), pid.to_string()) {
            tracing::warn!("Failed to write PID file: {}", e);
        }
    }

    fn remove_pid_file(&self) {
        let _ = std::fs::remove_file(self.pid_file_path());
    }

    pub(crate) async fn cleanup_orphan(&self) {
        let pid_path = self.pid_file_path();
        if let Ok(contents) = std::fs::read_to_string(&pid_path) {
            if let Ok(pid) = contents.trim().parse::<i32>() {
                if is_external_mcp_process(pid) {
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

/// Check whether a PID belongs to an external-mcp Node process.
fn is_external_mcp_process(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }

    let pid_arg = pid.to_string();
    let output = std::process::Command::new(resolve_ps_cli_path())
        .args(["-p", pid_arg.as_str(), "-o", "command="])
        .output();

    if let Ok(o) = output {
        let cmd = String::from_utf8_lossy(&o.stdout);
        return cmd.contains("external-mcp") || cmd.contains("external_mcp");
    }

    false
}

/// Returns true if a process with the given PID still exists.
fn process_exists(pid: i32) -> bool {
    // POSIX: kill(pid, 0) → Ok if process exists
    use nix::sys::signal::kill;
    kill(Pid::from_raw(pid), None).is_ok()
}
