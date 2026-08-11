use async_trait::async_trait;
use futures::Stream;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Instant;
use tokio::sync::Mutex;

use crate::domain::agents::{
    AgentConfig, AgentError, AgentHandle, AgentOutput, AgentResponse, AgentResult, AgenticClient,
    ClientCapabilities, ClientType, ResponseChunk, CODEX_DEFAULT_APPROVAL_POLICY,
    CODEX_DEFAULT_SANDBOX_MODE,
};

use super::{
    build_codex_mcp_overrides, build_spawnable_codex_exec_command, compose_codex_prompt,
    normalize_codex_exec_output, probe_codex_cli, resolve_codex_cli, CodexCliCapabilities,
    CodexExecCliConfig,
};

lazy_static! {
    static ref PROCESSES: Mutex<HashMap<String, (tokio::process::Child, Instant)>> =
        Mutex::new(HashMap::new());
}

/// Kill all Codex CLI processes tracked in the global PROCESSES map.
///
/// Mirrors `claude::kill_all_tracked_processes` — called from the Tauri exit
/// handler so Codex children get the same setsid-aware SIGTERM→grace→SIGKILL
/// teardown that Claude already gets. Without this, Codex spawns from the
/// previous session were left to the macOS reaper, and any keep-alive
/// sockets they held leaked into orphaned TIME_WAITs.
pub async fn kill_all_tracked_processes() {
    use crate::infrastructure::agents::spawn_isolation;

    let mut processes = PROCESSES.lock().await;
    let count = processes.len();
    if count == 0 {
        return;
    }

    tracing::info!(count, "Killing tracked Codex agent processes on exit");

    #[cfg(unix)]
    {
        let pids: Vec<u32> = processes
            .iter()
            .filter_map(|(_, (child, _))| child.id())
            .collect();
        for pid in &pids {
            spawn_isolation::send_signal_to_group(*pid, nix::sys::signal::Signal::SIGTERM);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    for (_id, (mut child, _start_time)) in processes.drain() {
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            spawn_isolation::send_signal_to_group(pid, nix::sys::signal::Signal::SIGKILL);
        }
        let _ = child.kill().await;
    }
}

pub struct CodexCliClient {
    cli_path: PathBuf,
    capabilities: ClientCapabilities,
}

impl CodexCliClient {
    pub fn new() -> Self {
        Self {
            cli_path: PathBuf::from("codex"),
            capabilities: ClientCapabilities::codex(),
        }
    }

    fn resolve_cli_path(&self, cli_path: &Path) -> AgentResult<PathBuf> {
        if cli_path.exists() {
            return Ok(cli_path.to_path_buf());
        }

        which::which(cli_path).map_err(|_| {
            AgentError::CliNotAvailable(format!("codex CLI not found at {:?}", cli_path))
        })
    }

    fn resolve_cli(&self) -> AgentResult<(PathBuf, CodexCliCapabilities)> {
        self.resolve_cli_for_path(&self.cli_path)
    }

    fn resolve_cli_for_config(
        &self,
        config: &AgentConfig,
    ) -> AgentResult<(PathBuf, CodexCliCapabilities)> {
        let cli_path = config
            .cli_path_override
            .as_deref()
            .unwrap_or(self.cli_path.as_path());
        self.resolve_cli_for_path(cli_path)
    }

    fn resolve_cli_for_path(
        &self,
        cli_path: &Path,
    ) -> AgentResult<(PathBuf, CodexCliCapabilities)> {
        if cli_path == Path::new("codex") {
            let resolved = resolve_codex_cli().map_err(AgentError::CliNotAvailable)?;
            return Ok((resolved.path, resolved.capabilities));
        }

        let cli_path = self.resolve_cli_path(cli_path)?;
        let capabilities =
            probe_codex_cli(&cli_path).map_err(|error| AgentError::CliNotAvailable(error))?;
        Ok((cli_path, capabilities))
    }

    fn build_prompt(&self, config: &AgentConfig) -> String {
        compose_codex_prompt(
            &config.prompt,
            config.plugin_dir.as_deref(),
            config.agent.as_deref(),
        )
    }

    fn build_exec_config(
        &self,
        config: &AgentConfig,
        config_overrides: Vec<String>,
    ) -> CodexExecCliConfig {
        CodexExecCliConfig {
            model: config.model.clone(),
            reasoning_effort: config.logical_effort,
            ultra_mode: false,
            approval_policy: Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string()),
            sandbox_mode: Some(CODEX_DEFAULT_SANDBOX_MODE.to_string()),
            service_tier: config.service_tier.clone(),
            config_overrides,
            cwd: Some(config.working_directory.clone()),
            add_dirs: Vec::new(),
            skip_git_repo_check: false,
            json_output: true,
            search: false,
        }
    }
}

impl Default for CodexCliClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgenticClient for CodexCliClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        let (cli_path, capabilities) = self.resolve_cli_for_config(&config)?;
        if !capabilities.has_core_exec_support() {
            return Err(AgentError::CliNotAvailable(format!(
                "Codex CLI is missing required capability: {}",
                capabilities.missing_core_exec_features().join(", ")
            )));
        }

        let mut config_overrides = if let (Some(plugin_dir), Some(agent_name)) =
            (config.plugin_dir.as_ref(), config.agent.as_deref())
        {
            build_codex_mcp_overrides(plugin_dir, agent_name, false, None)
                .map_err(AgentError::SpawnFailed)?
        } else {
            Vec::new()
        };
        config_overrides.extend(config.mcp_launch_policy.codex_config_overrides());

        let prompt = self.build_prompt(&config);
        let exec_config = self.build_exec_config(&config, config_overrides);
        let mut spawnable =
            build_spawnable_codex_exec_command(&cli_path, &prompt, &capabilities, &exec_config)
                .map_err(AgentError::SpawnFailed)?;

        for (key, value) in &config.env {
            spawnable.env(key, value);
        }

        let start_time = Instant::now();
        let child = spawnable
            .spawn()
            .await
            .map_err(|error| AgentError::SpawnFailed(error.to_string()))?;
        let handle = AgentHandle::new(ClientType::Codex, config.role);

        PROCESSES
            .lock()
            .await
            .insert(handle.id.clone(), (child, start_time));

        Ok(handle)
    }

    async fn stop_agent(&self, handle: &AgentHandle) -> AgentResult<()> {
        let mut processes = PROCESSES.lock().await;
        if let Some((mut child, _)) = processes.remove(&handle.id) {
            child
                .kill()
                .await
                .map_err(|error| AgentError::CommunicationFailed(error.to_string()))?;
        }
        Ok(())
    }

    async fn wait_for_completion(&self, handle: &AgentHandle) -> AgentResult<AgentOutput> {
        let mut processes = PROCESSES.lock().await;
        let (child, start_time) = processes
            .remove(&handle.id)
            .ok_or_else(|| AgentError::NotFound(handle.id.clone()))?;

        let output = child
            .wait_with_output()
            .await
            .map_err(|error| AgentError::CommunicationFailed(error.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let normalized_stdout = normalize_codex_exec_output(&stdout);
        let content = if normalized_stdout.trim().is_empty() && !stderr.trim().is_empty() {
            stderr
        } else {
            normalized_stdout
        };

        Ok(AgentOutput {
            success: output.status.success(),
            content,
            exit_code: output.status.code(),
            duration_ms: Some(start_time.elapsed().as_millis() as u64),
        })
    }

    async fn send_prompt(&self, _handle: &AgentHandle, prompt: &str) -> AgentResult<AgentResponse> {
        let handle = self
            .spawn_agent(
                AgentConfig::worker(prompt)
                    .with_harness(crate::domain::agents::AgentHarnessKind::Codex),
            )
            .await?;
        let output = self.wait_for_completion(&handle).await?;
        Ok(AgentResponse {
            content: output.content,
            model: Some("codex".to_string()),
            tokens_used: None,
        })
    }

    fn stream_response(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
        let chunks = vec![
            Ok(ResponseChunk::new(
                "Use codex exec JSONL handling instead of AgenticClient::stream_response",
            )),
            Ok(ResponseChunk::final_chunk("")),
        ];
        Box::pin(futures::stream::iter(chunks))
    }

    fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    async fn is_available(&self) -> AgentResult<bool> {
        let Ok((_, capabilities)) = self.resolve_cli() else {
            return Ok(false);
        };

        Ok(capabilities.has_core_exec_support())
    }
}
