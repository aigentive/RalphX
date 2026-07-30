// Claude Code CLI client
// Production implementation using the `claude` CLI
//
// This client supports two modes of operation:
// 1. Simple spawn-and-wait: Use spawn_agent() + wait_for_completion()
// 2. Streaming with persistence: Use spawn_agent_streaming() to get the Child process
//    and handle stream processing externally (used by ExecutionChatService)

use async_trait::async_trait;
use futures::Stream;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Child;
use tokio::sync::Mutex;

use crate::domain::agents::{
    AgentConfig, AgentError, AgentHandle, AgentOutput, AgentResponse, AgentResult, AgenticClient,
    ClientCapabilities, ClientType, ResponseChunk,
};

use super::cli_capabilities::claude_cli_supports_partial_messages;
use super::{
    append_claude_permission_args, apply_common_spawn_env,
    build_spawnable_command_with_mcp_runtime_context_and_profile, claude_runtime_config,
    create_mcp_config, ensure_claude_spawn_allowed, find_claude_cli, get_allowed_tools,
    get_effective_settings, get_preapproved_tools, validate_claude_model_for_cli_path,
    SpawnableCommand,
};

#[cfg(test)]
use super::build_spawnable_command_with_mcp_runtime_context_and_profile_for_test;

// ============================================================================
// Streaming Event Types
// ============================================================================

/// Events emitted during agent stream processing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    /// Text chunk received from agent
    TextChunk { text: String },
    /// Tool call started
    ToolCallStart {
        tool_name: String,
        tool_id: Option<String>,
    },
    /// Tool call input (incremental JSON)
    ToolCallInput {
        tool_name: String,
        tool_id: Option<String>,
        partial_json: String,
    },
    /// Tool call completed
    ToolCallComplete {
        tool_name: String,
        tool_id: Option<String>,
        arguments: serde_json::Value,
    },
    /// Agent execution completed with session ID
    Completed { session_id: Option<String> },
    /// Error occurred during execution
    Error { message: String },
}

/// Result from spawning an agent in streaming mode
#[derive(Debug)]
pub struct StreamingSpawnResult {
    /// Handle to the spawned agent
    pub handle: AgentHandle,
    /// The spawned child process (stdout is piped for stream processing)
    pub child: Child,
    /// Stdin pipe for interactive mode. `Some` when spawned via `spawn_agent_interactive()`.
    /// `None` for standard streaming spawns (backward-compat default).
    pub stdin: Option<tokio::process::ChildStdin>,
}

// ============================================================================
lazy_static! {
    /// Global tracker for spawned child processes with their start time
    static ref PROCESSES: Mutex<HashMap<String, (Child, Instant)>> = Mutex::new(HashMap::new());
}

/// Kill all processes tracked in the global PROCESSES map.
///
/// Called during app exit to ensure no orphaned non-streaming agent processes remain.
/// The lazy_static is not dropped on exit, so explicit cleanup is required.
///
/// On Unix, every Claude spawn runs `setsid()` via
/// [`crate::infrastructure::agents::claude::apply_common_spawn_env`], so the
/// child PID also names its process group. We send SIGTERM to the whole
/// group first (Claude + its Task subagents + the stdio MCP server), give it
/// a short grace window to flush keep-alive sockets and any pending stdio,
/// then escalate to SIGKILL on the group. This is the difference between
/// "MCP server gets a clean close" and "MCP server is reaped mid-burst and
/// leaves orphan TIME_WAITs".
///
/// On Windows we fall back to `child.kill()` (SIGKILL-equivalent on the head
/// PID); taskkill /T tree-kill via the registry path covers the descendants.
pub async fn kill_all_tracked_processes() {
    use crate::infrastructure::agents::spawn_isolation;

    let mut processes = PROCESSES.lock().await;
    let count = processes.len();
    if count == 0 {
        return;
    }

    tracing::info!(
        count,
        "Killing tracked non-streaming Claude agent processes on exit"
    );

    // Phase 1: SIGTERM the whole group for each tracked child (Unix only).
    #[cfg(unix)]
    {
        let pids: Vec<u32> = processes
            .iter()
            .filter_map(|(_, (child, _))| child.id())
            .collect();
        for pid in &pids {
            spawn_isolation::send_signal_to_group(*pid, nix::sys::signal::Signal::SIGTERM);
        }
        // Brief grace window so the MCP server / Claude can close sockets and
        // flush stdio. Keep this short — app exit is already on the hot path.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // Phase 2: drain the map. On Unix we follow up with SIGKILL to the group
    // (catches whatever didn't honor SIGTERM); `child.kill()` covers Windows
    // and serves as a defensive double-kill on Unix.
    for (_id, (mut child, _start_time)) in processes.drain() {
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            spawn_isolation::send_signal_to_group(pid, nix::sys::signal::Signal::SIGKILL);
        }
        let _ = child.kill().await;
    }
}

/// Client for Claude Code CLI
///
/// Uses the `claude` CLI tool to spawn and communicate with Claude agents.
pub struct ClaudeCodeClient {
    /// Path to the claude CLI
    cli_path: PathBuf,
    /// Client capabilities
    capabilities: ClientCapabilities,
}

impl ClaudeCodeClient {
    /// Create a new Claude Code client
    ///
    /// Attempts to find `claude` in terminal and GUI app contexts.
    pub fn new() -> Self {
        let cli_path = find_claude_cli().unwrap_or_else(|| PathBuf::from("claude"));
        Self {
            cli_path,
            capabilities: ClientCapabilities::claude_code(),
        }
    }

    /// Create with a specific CLI path
    pub fn with_cli_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cli_path = path.into();
        self
    }

    /// Get the CLI path
    pub fn cli_path(&self) -> &PathBuf {
        &self.cli_path
    }

    fn cli_path_for_config<'a>(&'a self, config: &'a AgentConfig) -> &'a Path {
        config
            .cli_path_override
            .as_deref()
            .unwrap_or(self.cli_path.as_path())
    }

    fn cli_path_is_available(cli_path: &Path) -> bool {
        cli_path.exists() || which::which(cli_path).is_ok()
    }
}

fn command_log_program(cmd: &tokio::process::Command) -> String {
    cmd.as_std().get_program().to_string_lossy().into_owned()
}

fn command_log_arg_count(cmd: &tokio::process::Command) -> usize {
    cmd.as_std().get_args().count()
}

fn command_log_env_keys(cmd: &tokio::process::Command) -> Vec<String> {
    cmd.as_std()
        .get_envs()
        .filter_map(|(key, value)| value.map(|_| key.to_string_lossy().into_owned()))
        .collect()
}

impl Default for ClaudeCodeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeClient {
    fn build_spawnable_agent_command(
        &self,
        config: &AgentConfig,
        resume_session_id: Option<&str>,
        enforce_spawn_guard: bool,
    ) -> Result<SpawnableCommand, String> {
        let plugin_dir = config
            .plugin_dir
            .as_deref()
            .unwrap_or_else(|| Path::new("./plugins/app"));
        let effort_override = config.logical_effort.map(|effort| effort.to_string());
        let cli_path = self.cli_path_for_config(config);

        let mut spawnable = if enforce_spawn_guard {
            build_spawnable_command_with_mcp_runtime_context_and_profile(
                cli_path,
                plugin_dir,
                &config.prompt,
                config.agent.as_deref(),
                None,
                None,
                resume_session_id,
                &config.working_directory,
                false,
                effort_override.as_deref(),
                config.model.as_deref(),
                None,
            )?
        } else {
            #[cfg(test)]
            {
                build_spawnable_command_with_mcp_runtime_context_and_profile_for_test(
                    cli_path,
                    plugin_dir,
                    &config.prompt,
                    config.agent.as_deref(),
                    None,
                    None,
                    resume_session_id,
                    &config.working_directory,
                    false,
                    effort_override.as_deref(),
                    config.model.as_deref(),
                    None,
                )?
            }
            #[cfg(not(test))]
            {
                let _ = enforce_spawn_guard;
                build_spawnable_command_with_mcp_runtime_context_and_profile(
                    cli_path,
                    plugin_dir,
                    &config.prompt,
                    config.agent.as_deref(),
                    None,
                    None,
                    resume_session_id,
                    &config.working_directory,
                    false,
                    effort_override.as_deref(),
                    config.model.as_deref(),
                    None,
                )?
            }
        };

        if let Some(max_tokens) = config.max_tokens {
            spawnable.arg("--max-tokens");
            spawnable.arg(&max_tokens.to_string());
        }
        let disallowed_tools = config.mcp_launch_policy.claude_disallowed_tools();
        if !disallowed_tools.is_empty() {
            spawnable.arg("--disallowedTools");
            spawnable.arg(&disallowed_tools.join(","));
        }

        Ok(spawnable)
    }
}

fn resolved_config_model(config: &AgentConfig) -> Option<String> {
    config.model.clone().or_else(|| {
        config.agent.as_ref().and_then(|agent_name| {
            crate::infrastructure::agents::claude::get_agent_config(agent_name)
                .and_then(|cfg| cfg.model.clone())
        })
    })
}

fn append_validated_model_args(
    args: &mut Vec<String>,
    cli_path: &Path,
    model: &str,
) -> Result<(), String> {
    validate_claude_model_for_cli_path(cli_path, model)?;
    args.extend(["--model".to_string(), model.to_string()]);
    Ok(())
}

#[async_trait]
impl AgenticClient for ClaudeCodeClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        if let Err(err) = ensure_claude_spawn_allowed() {
            return Err(AgentError::SpawnNotAllowed(err));
        }

        // Check if CLI is available first
        let cli_path = self.cli_path_for_config(&config);
        if !Self::cli_path_is_available(cli_path) {
            return Err(AgentError::CliNotAvailable(format!(
                "claude CLI not found at {:?}",
                cli_path
            )));
        }

        let mut spawnable = self
            .build_spawnable_agent_command(&config, None, true)
            .map_err(AgentError::SpawnFailed)?;

        // Add environment variables
        for (key, value) in &config.env {
            spawnable.env(key, value);
        }

        // Spawn the process and record start time for duration tracking
        tracing::info!(cmd = ?spawnable, "Spawning CLI agent (agentic)");
        let start_time = Instant::now();
        let child = spawnable
            .spawn()
            .await
            .map_err(|e| AgentError::SpawnFailed(e.to_string()))?;

        let handle = AgentHandle::new(ClientType::ClaudeCode, config.role);

        // Store the child process with its start time
        PROCESSES
            .lock()
            .await
            .insert(handle.id.clone(), (child, start_time));

        Ok(handle)
    }

    async fn stop_agent(&self, handle: &AgentHandle) -> AgentResult<()> {
        let mut processes = PROCESSES.lock().await;
        if let Some((mut child, _start_time)) = processes.remove(&handle.id) {
            child
                .kill()
                .await
                .map_err(|e| AgentError::CommunicationFailed(e.to_string()))?;
        }
        // If not found, consider it already stopped (no error)
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
            .map_err(|e| AgentError::CommunicationFailed(e.to_string()))?;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        Ok(AgentOutput {
            success: output.status.success(),
            content: String::from_utf8_lossy(&output.stdout).to_string(),
            exit_code: output.status.code(),
            duration_ms: Some(duration_ms),
        })
    }

    async fn send_prompt(&self, _handle: &AgentHandle, prompt: &str) -> AgentResult<AgentResponse> {
        // For send_prompt, we spawn a new one-shot agent
        let config = AgentConfig::worker(prompt);

        let handle = self.spawn_agent(config).await?;
        let output = self.wait_for_completion(&handle).await?;

        Ok(AgentResponse {
            content: output.content,
            model: Some("claude".to_string()),
            tokens_used: None,
        })
    }

    fn stream_response(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
        // Note: Production streaming uses spawn_agent_streaming() which returns Child process
        // for external stream handling (see ExecutionChatService). This trait method is a
        // placeholder for potential future trait-level streaming support.
        let chunks = vec![
            Ok(ResponseChunk::new(
                "Use spawn_agent_streaming() for production streaming",
            )),
            Ok(ResponseChunk::final_chunk("")),
        ];
        Box::pin(futures::stream::iter(chunks))
    }

    fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    async fn is_available(&self) -> AgentResult<bool> {
        // Check if the CLI exists
        if self.cli_path.exists() {
            return Ok(true);
        }

        // Try to find it in PATH
        match which::which(&self.cli_path) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

// ============================================================================
// Streaming Spawn Support
// ============================================================================

impl ClaudeCodeClient {
    /// Build CLI arguments from an AgentConfig
    ///
    /// This is used by both spawn_agent and spawn_agent_streaming to ensure
    /// consistent argument construction.
    ///
    /// When `interactive` is `true`, the `-p` flag is omitted so the process stays
    /// alive for multi-turn messaging via stdin. All other flags (tools, model, etc.)
    /// are still applied.
    fn build_cli_args(
        &self,
        config: &AgentConfig,
        resume_session_id: Option<&str>,
        interactive: bool,
    ) -> Result<Vec<String>, String> {
        let cli_path = self.cli_path_for_config(config);
        let mut args = Vec::new();

        // Prompt — omitted for interactive mode (caller sends prompt via stdin)
        if !interactive {
            args.extend(["-p".to_string(), config.prompt.clone()]);
        }

        // Output format for streaming
        args.extend(["--output-format".to_string(), "stream-json".to_string()]);
        args.push("--verbose".to_string()); // Required for stream-json with -p
        if !interactive && claude_cli_supports_partial_messages(cli_path) {
            args.push("--include-partial-messages".to_string());
        }
        if let Some(sources) = &claude_runtime_config().setting_sources {
            if !sources.is_empty() {
                args.extend(["--setting-sources".to_string(), sources.join(",")]);
            }
        }
        // Avoid startup parser crashes in slash-command/skills loading path.
        args.push("--disable-slash-commands".to_string());

        // Plugin directory for agent/skill discovery
        if let Some(plugin_dir) = &config.plugin_dir {
            args.extend(["--plugin-dir".to_string(), plugin_dir.display().to_string()]);

            // Add RalphX's dynamic MCP config without suppressing provider-native
            // user, project, and local MCP configuration layers.
            if let Some(agent) = &config.agent {
                let temp_path = create_mcp_config(plugin_dir, agent, false)?;
                args.extend(["--mcp-config".to_string(), temp_path.display().to_string()]);
            }
        }

        // Resume session - always include agent to enforce tool restrictions
        if let Some(session_id) = resume_session_id {
            args.extend(["--resume".to_string(), session_id.to_string()]);
            // CRITICAL: Also pass --agent to enforce disallowedTools on resume
            // Without this, resumed sessions bypass tool restrictions
            if let Some(agent) = &config.agent {
                args.extend(["--agent".to_string(), agent.clone()]);
            }
        } else if let Some(agent) = &config.agent {
            args.extend(["--agent".to_string(), agent.clone()]);
        }

        // Apply CLI tool restrictions from agent_config
        // Frontmatter tools/disallowedTools only work for subagent spawning,
        // NOT for direct CLI invocations with --agent -p. Pass --tools only when
        // there are built-in CLI tools to allow; the Claude CLI treats an empty
        // value as disabling MCP tools too.
        if let Some(agent_name) = &config.agent {
            if let Some(allowed_tools) = get_allowed_tools(agent_name) {
                if allowed_tools.is_empty() {
                    tracing::debug!(
                        agent = %agent_name,
                        "Agent configured as MCP-only; omitting --tools because Claude CLI treats an empty value as disabling MCP tools"
                    );
                } else {
                    tracing::debug!(agent = %agent_name, tools = allowed_tools.as_str(), "Agent restricted to CLI tools");
                    args.extend(["--tools".to_string(), allowed_tools]);
                }
            }
        }

        // Model override: explicit config first, then per-agent default from config/ralphx.yaml
        if let Some(model) = resolved_config_model(config) {
            append_validated_model_args(&mut args, cli_path, &model)?;
        }

        // Max tokens
        if let Some(max_tokens) = config.max_tokens {
            args.extend(["--max-tokens".to_string(), max_tokens.to_string()]);
        }

        // Permission handling from config/harnesses/claude.yaml. This base-agent path
        // builds MCP config without a profile, so resolve permissions with no profile.
        append_claude_permission_args(&mut args, config.agent.as_deref(), None);
        // Optional settings JSON passed to claude CLI via --settings.
        // Agent-specific profile overrides global profile when configured.
        if let Some(s) = get_effective_settings(config.agent.as_deref()) {
            if let Ok(json) = serde_json::to_string(s) {
                args.extend(["--settings".to_string(), json]);
            }
        }

        // Pre-approve agent-specific tools (MCP + CLI permissions, no prompts)
        if let Some(agent) = &config.agent {
            if let Some(preapproved) = get_preapproved_tools(agent) {
                args.push("--allowedTools".to_string());
                args.push(preapproved);
            }
        }
        let disallowed_tools = config.mcp_launch_policy.claude_disallowed_tools();
        if !disallowed_tools.is_empty() {
            args.extend(["--disallowedTools".to_string(), disallowed_tools.join(",")]);
        }

        Ok(args)
    }

    /// Spawn an agent in streaming mode, returning the Child process for external processing
    ///
    /// Unlike `spawn_agent`, this method does NOT store the child process internally.
    /// The caller is responsible for:
    /// 1. Processing stdout for stream-json events
    /// 2. Waiting for the process to complete
    /// 3. Capturing the provider session id from the Result event
    ///
    /// This is used by ExecutionChatService to persist stream events to the database
    /// while emitting Tauri events for real-time UI updates.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = client.spawn_agent_streaming(config, None).await?;
    /// let stdout = result.child.stdout.take().unwrap();
    /// let reader = BufReader::new(stdout);
    /// // Process stream-json lines from reader...
    /// ```
    pub async fn spawn_agent_streaming(
        &self,
        config: AgentConfig,
        resume_session_id: Option<&str>,
    ) -> AgentResult<StreamingSpawnResult> {
        if let Err(err) = ensure_claude_spawn_allowed() {
            return Err(AgentError::SpawnNotAllowed(err));
        }

        // Check if CLI is available first
        let cli_path = self.cli_path_for_config(&config);
        if !Self::cli_path_is_available(cli_path) {
            return Err(AgentError::CliNotAvailable(format!(
                "claude CLI not found at {:?}",
                cli_path
            )));
        }

        let args = self
            .build_cli_args(&config, resume_session_id, false)
            .map_err(|e| AgentError::SpawnFailed(e))?;

        // Build command
        let mut cmd = tokio::process::Command::new(cli_path);
        cmd.args(&args)
            .current_dir(&config.working_directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped()); // piped (was null) — supports interactive callers in task #4
        apply_common_spawn_env(&mut cmd);
        if let Some(plugin_dir) = &config.plugin_dir {
            cmd.env("CLAUDE_PLUGIN_ROOT", plugin_dir);
        }

        // Add environment variables from config
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        // Spawn the process
        tracing::info!(
            program = %command_log_program(&cmd),
            arg_count = command_log_arg_count(&cmd),
            env_keys = ?command_log_env_keys(&cmd),
            "Spawning CLI agent (streaming)"
        );
        let mut child = cmd
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AgentError::SpawnFailed(e.to_string()))?;

        // Take stdin (piped above). None is returned for standard streaming mode
        // (backward compat). Interactive callers use spawn_agent_interactive() instead.
        let stdin = child.stdin.take();

        let handle = AgentHandle::new(ClientType::ClaudeCode, config.role);

        Ok(StreamingSpawnResult {
            handle,
            child,
            stdin,
        })
    }

    /// Spawn an agent in interactive mode (no `-p` flag, stdin kept open).
    ///
    /// Unlike `spawn_agent_streaming` (which uses `-p <prompt>` for one-shot turns),
    /// this starts the Claude CLI without a prompt flag so it enters interactive/REPL
    /// mode and waits for input via stdin. The caller sends the initial prompt via
    /// the returned `stdin` handle and can write additional messages for multi-turn.
    ///
    /// The returned `StreamingSpawnResult.stdin` is always `Some` from this method.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = client.spawn_agent_interactive(config, None).await?;
    /// let mut stdin = result.stdin.unwrap();
    /// // Send initial prompt
    /// stdin.write_all(b"Analyze this codebase\n").await?;
    /// // Process stdout stream-json events...
    /// // Send a follow-up message later:
    /// stdin.write_all(b"Now summarize your findings\n").await?;
    /// ```
    pub async fn spawn_agent_interactive(
        &self,
        config: AgentConfig,
        resume_session_id: Option<&str>,
    ) -> AgentResult<StreamingSpawnResult> {
        if let Err(err) = ensure_claude_spawn_allowed() {
            return Err(AgentError::SpawnNotAllowed(err));
        }

        // Check if CLI is available first
        let cli_path = self.cli_path_for_config(&config);
        if !Self::cli_path_is_available(cli_path) {
            return Err(AgentError::CliNotAvailable(format!(
                "claude CLI not found at {:?}",
                cli_path
            )));
        }

        // Build args without -p (interactive=true)
        let args = self
            .build_cli_args(&config, resume_session_id, true)
            .map_err(|e| AgentError::SpawnFailed(e))?;

        // Build command with stdin piped for message delivery
        let mut cmd = tokio::process::Command::new(cli_path);
        cmd.args(&args)
            .current_dir(&config.working_directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped()); // Piped — caller writes prompt + future messages
        apply_common_spawn_env(&mut cmd);
        if let Some(plugin_dir) = &config.plugin_dir {
            cmd.env("CLAUDE_PLUGIN_ROOT", plugin_dir);
        }

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        tracing::info!(
            program = %command_log_program(&cmd),
            arg_count = command_log_arg_count(&cmd),
            env_keys = ?command_log_env_keys(&cmd),
            "Spawning CLI agent (interactive, no -p)"
        );
        let mut child = cmd
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AgentError::SpawnFailed(e.to_string()))?;

        // stdin must be present — we configured Stdio::piped() above
        let stdin = child.stdin.take().ok_or_else(|| {
            AgentError::SpawnFailed(
                "Failed to capture stdin pipe for interactive agent".to_string(),
            )
        })?;

        let handle = AgentHandle::new(ClientType::ClaudeCode, config.role);

        Ok(StreamingSpawnResult {
            handle,
            child,
            stdin: Some(stdin),
        })
    }

    /// Check if the Claude CLI is available
    ///
    /// This is a simpler version of is_available() that doesn't require async.
    pub fn cli_available(&self) -> bool {
        self.cli_path.exists() || which::which(&self.cli_path).is_ok()
    }
}

// ============================================================================
#[cfg(test)]
#[path = "claude_code_client_tests.rs"]
mod tests;
