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
    AgentConfig, AgentError, AgentHandle, AgentOutput, AgentResponse, AgentResult, AgentRole,
    AgenticClient, ClientCapabilities, ClientType, ResponseChunk,
};
use crate::infrastructure::agents::mcp_runtime_context::McpRuntimeContext;

use super::{
    append_claude_permission_args, apply_common_spawn_env,
    build_spawnable_command_with_mcp_runtime_context_and_profile, claude_runtime_config,
    create_mcp_config, ensure_claude_spawn_allowed, find_claude_cli, get_allowed_tools,
    get_effective_settings, get_preapproved_tools, normalize_claude_effort_for_cli_path,
    validate_claude_model_for_cli_path, SpawnableCommand,
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
// Teammate Interactive Spawn Types
// ============================================================================

/// RalphX session/project context propagated from lead agent to teammates.
///
/// Carried as env vars (RALPHX_CONTEXT_ID, RALPHX_CONTEXT_TYPE, RALPHX_PROJECT_ID)
/// so teammates can filter MCP tools and resolve project-scoped resources.
///
/// **Not** the same as `parent_session_id` (the lead's Claude Code session ID
/// for team registry/messaging). Using a separate struct prevents accidentally
/// passing `context_id` where `parent_session_id` is expected.
#[derive(Debug, Clone, Default)]
pub struct TeammateContext {
    /// RalphX session ID (e.g., ideation session UUID or task ID)
    pub context_id: String,
    /// Context type (e.g., "ideation", "task_execution")
    pub context_type: String,
    /// Project ID for project-scoped resources
    pub project_id: Option<String>,
}

/// Configuration for spawning a team teammate in interactive mode (no `-p` flag).
///
/// Unlike `AgentConfig` (print mode), teammates are long-lived interactive sessions
/// that receive messages via Claude Code's native SendMessage tool. The process stays
/// alive until a shutdown_request is received.
///
/// # Construction
///
/// Use `new(name, team_name, prompt)` for required fields, then builder methods:
/// - `.with_parent_session_id()` — **required** for team messaging
/// - `.with_context()` — RalphX session context (env vars)
/// - `.with_model()`, `.with_tools()`, etc. — optional overrides
#[derive(Debug, Clone)]
pub struct TeammateSpawnConfig {
    /// Teammate name (e.g., "transport-researcher")
    pub name: String,
    /// Team name (e.g., "ideation-abc123")
    pub team_name: String,
    /// Lead agent's Claude Code session ID for team registry/messaging.
    /// Set via `with_parent_session_id()` — NOT the RalphX context_id.
    pub parent_session_id: String,
    /// Lead-generated role prompt (passed via --append-system-prompt)
    pub prompt: String,
    /// Model to use (within model ceiling, e.g. "sonnet")
    pub model: String,
    /// Approved CLI tools (e.g. ["Read", "Grep", "Glob"])
    pub tools: Vec<String>,
    /// Approved MCP tools (short names; will be prefixed with mcp__ralphx__)
    pub mcp_tools: Vec<String>,
    /// Agent color for terminal distinction (e.g. "blue", "green")
    pub color: String,
    /// Working directory for the teammate process
    pub working_directory: PathBuf,
    /// Plugin directory path for MCP server and agent discovery
    pub plugin_dir: Option<PathBuf>,
    /// Claude Code agent type controlling built-in tool set (default: "general-purpose")
    pub agent_type: String,
    /// MCP agent type for tool filtering (default: "ideation-team-member")
    pub mcp_agent_type: String,
    /// Additional environment variables
    pub env: HashMap<String, String>,
    /// Optional print-mode prompt. When set, teammate uses `-p <prompt>` (one-shot)
    /// instead of interactive `--append-system-prompt` mode. Used for auto-spawning
    /// teammates detected from the lead's stream.
    pub print_mode_prompt: Option<String>,
    /// RalphX session/project context (propagated as env vars to teammates)
    pub context: TeammateContext,
    /// Effort level override (e.g. "max"). Falls back to global default_effort when None.
    pub effort: Option<String>,
    /// Resolved provider-native MCP deny policy for this teammate launch.
    pub mcp_launch_policy: crate::domain::agents::McpLaunchPolicy,
}

impl TeammateSpawnConfig {
    /// Create a new teammate config with team identity and prompt.
    ///
    /// Use builder methods for remaining required/optional fields:
    /// - `.with_parent_session_id()` — lead's Claude Code session ID (required)
    /// - `.with_context()` — RalphX session/project context
    pub fn new(
        name: impl Into<String>,
        team_name: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            team_name: team_name.into(),
            parent_session_id: String::new(),
            prompt: prompt.into(),
            model: "sonnet".to_string(),
            tools: Vec::new(),
            mcp_tools: Vec::new(),
            color: "blue".to_string(),
            working_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            plugin_dir: Some(PathBuf::from("./plugins/app")),
            agent_type: "general-purpose".to_string(),
            mcp_agent_type: "ideation-team-member".to_string(),
            env: HashMap::new(),
            print_mode_prompt: None,
            context: TeammateContext::default(),
            effort: None,
            mcp_launch_policy: Default::default(),
        }
    }

    /// Set the lead agent's Claude Code session ID for team messaging.
    ///
    /// This is the lead's actual Claude Code session ID (from the team config file
    /// at `~/.claude/teams/{team}/config.json` → `leadSessionId`), NOT the RalphX
    /// context_id. Teammates need this to join the team registry and receive messages.
    pub fn with_parent_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.parent_session_id = session_id.into();
        self
    }

    /// Set the RalphX session/project context (propagated as env vars).
    pub fn with_context(mut self, context: TeammateContext) -> Self {
        self.context = context;
        self
    }

    /// Set the model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Set the CLI tools.
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the MCP tools.
    pub fn with_mcp_tools(mut self, mcp_tools: Vec<String>) -> Self {
        self.mcp_tools = mcp_tools;
        self
    }

    /// Set the agent color.
    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = color.into();
        self
    }

    /// Set the working directory.
    pub fn with_working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = path.into();
        self
    }

    /// Set the plugin directory.
    pub fn with_plugin_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.plugin_dir = Some(path.into());
        self
    }

    /// Set the Claude Code agent type (controls built-in tool set).
    pub fn with_agent_type(mut self, agent_type: impl Into<String>) -> Self {
        self.agent_type = agent_type.into();
        self
    }

    /// Set the MCP agent type (controls MCP-side tool filtering).
    pub fn with_mcp_agent_type(mut self, mcp_agent_type: impl Into<String>) -> Self {
        self.mcp_agent_type = mcp_agent_type.into();
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set print-mode prompt for one-shot `-p` execution (auto-spawn mode).
    pub fn with_print_mode_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.print_mode_prompt = Some(prompt.into());
        self
    }

    pub fn with_effort(mut self, effort: impl Into<String>) -> Self {
        self.effort = Some(effort.into());
        self
    }

    pub fn with_mcp_launch_policy(
        mut self,
        policy: crate::domain::agents::McpLaunchPolicy,
    ) -> Self {
        self.mcp_launch_policy = policy;
        self
    }
}

/// Result from spawning a teammate in interactive mode.
#[derive(Debug)]
pub struct TeammateSpawnResult {
    /// Handle to the spawned teammate
    pub handle: AgentHandle,
    /// The spawned child process (stdout piped for stream processing)
    pub child: Child,
    /// Stdin pipe for sending messages to the teammate
    pub stdin: tokio::process::ChildStdin,
}

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
        let mcp_runtime_context = agent_mcp_runtime_context(config);
        let agent_profile = config
            .env
            .get("RALPHX_AGENT_PROFILE")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let mut spawnable = if enforce_spawn_guard {
            build_spawnable_command_with_mcp_runtime_context_and_profile(
                cli_path,
                plugin_dir,
                &config.prompt,
                config.agent.as_deref(),
                agent_profile,
                None,
                resume_session_id,
                &config.working_directory,
                false,
                effort_override.as_deref(),
                config.model.as_deref(),
                mcp_runtime_context.as_ref(),
            )?
        } else {
            #[cfg(test)]
            {
                build_spawnable_command_with_mcp_runtime_context_and_profile_for_test(
                    cli_path,
                    plugin_dir,
                    &config.prompt,
                    config.agent.as_deref(),
                    agent_profile,
                    None,
                    resume_session_id,
                    &config.working_directory,
                    false,
                    effort_override.as_deref(),
                    config.model.as_deref(),
                    mcp_runtime_context.as_ref(),
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
                    agent_profile,
                    None,
                    resume_session_id,
                    &config.working_directory,
                    false,
                    effort_override.as_deref(),
                    config.model.as_deref(),
                    mcp_runtime_context.as_ref(),
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

fn agent_mcp_runtime_context(config: &AgentConfig) -> Option<McpRuntimeContext> {
    McpRuntimeContext::from_agent_env(&config.env, &config.working_directory)
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
// Teammate Interactive Spawn Support
// ============================================================================

impl ClaudeCodeClient {
    /// Build CLI arguments for an interactive teammate spawn.
    ///
    /// Key differences from `build_cli_args`:
    /// - **No `-p` flag** — teammates are interactive sessions
    /// - **Team CLI flags** — `--agent-id`, `--agent-name`, `--team-name`, etc.
    /// - **`--append-system-prompt`** — lead-generated role prompt
    /// - **`--dangerously-skip-permissions`** — automated teammates skip prompts
    pub fn build_teammate_cli_args(
        &self,
        config: &TeammateSpawnConfig,
    ) -> Result<Vec<String>, String> {
        let mut args = Vec::new();

        // Output format for streaming (same as other modes)
        args.extend(["--output-format".to_string(), "stream-json".to_string()]);
        args.push("--verbose".to_string());

        // Setting sources from runtime config
        if let Some(sources) = &claude_runtime_config().setting_sources {
            if !sources.is_empty() {
                args.extend(["--setting-sources".to_string(), sources.join(",")]);
            }
        }

        // Avoid startup parser crashes in slash-command/skills loading path
        args.push("--disable-slash-commands".to_string());

        // Plugin directory for agent/skill discovery
        if let Some(plugin_dir) = &config.plugin_dir {
            args.extend(["--plugin-dir".to_string(), plugin_dir.display().to_string()]);

            // Create dynamic MCP config with MCP agent type for tool filtering
            // Uses mcp_agent_type (e.g., "ideation-team-member") not the Claude Code agent_type
            // Hard error on invalid config — MCP is critical infra, fail loud.
            let temp_path = create_mcp_config(plugin_dir, &config.mcp_agent_type, false)?;
            args.extend(["--mcp-config".to_string(), temp_path.display().to_string()]);
        }

        // --- Team-specific CLI flags ---
        args.extend([
            "--agent-id".to_string(),
            format!("{}@{}", config.name, config.team_name),
        ]);
        args.extend(["--agent-name".to_string(), config.name.clone()]);
        args.extend(["--team-name".to_string(), config.team_name.clone()]);
        args.extend(["--agent-color".to_string(), config.color.clone()]);
        args.extend([
            "--parent-session-id".to_string(),
            config.parent_session_id.clone(),
        ]);
        // Claude Code agent type controls built-in tool set (e.g., "general-purpose")
        args.extend(["--agent-type".to_string(), config.agent_type.clone()]);

        // Model selection (within model ceiling)
        append_validated_model_args(&mut args, &self.cli_path, &config.model)?;

        // Effort level — explicitly passed by spawner via .with_effort(), or global default
        let effort = config
            .effort
            .clone()
            .unwrap_or_else(|| claude_runtime_config().default_effort.clone());
        let normalized_effort = normalize_claude_effort_for_cli_path(&self.cli_path, &effort);
        if normalized_effort != effort {
            tracing::warn!(
                requested_effort = %effort,
                effective_effort = %normalized_effort,
                cli_path = %self.cli_path.display(),
                "Normalized Claude teammate effort for installed CLI capability"
            );
        }
        args.extend(["--effort".to_string(), normalized_effort]);

        // CLI tools restriction
        if !config.tools.is_empty() {
            args.extend(["--tools".to_string(), config.tools.join(",")]);
        }

        // Pre-approved MCP tools (prefixed with mcp__ralphx__)
        if !config.mcp_tools.is_empty() {
            let mcp_server_name = &claude_runtime_config().mcp_server_name;
            let prefixed: Vec<String> = config
                .mcp_tools
                .iter()
                .map(|t| format!("mcp__{mcp_server_name}__{t}"))
                .collect();
            args.extend(["--allowedTools".to_string(), prefixed.join(",")]);
        }
        let disallowed_tools = config.mcp_launch_policy.claude_disallowed_tools();
        if !disallowed_tools.is_empty() {
            args.extend(["--disallowedTools".to_string(), disallowed_tools.join(",")]);
        }

        // Prompt mode: -p is REQUIRED for --output-format stream-json to produce output.
        // One-shot: -p <prompt> for single-turn teammates.
        // Interactive: -p - with --input-format stream-json enables print mode so stdout
        // emits stream-json events. The initial prompt is sent via stdin to activate stdout
        // output — without it Claude Code waits for stdin and team inbox messages (SendMessage)
        // don't generate stdout. Subsequent work arrives via the team inbox.
        if let Some(ref prompt) = config.print_mode_prompt {
            // One-shot mode: prompt passed directly via -p
            args.extend(["-p".to_string(), prompt.clone()]);
        } else {
            // Interactive mode: -p - enables print mode (required for stream-json output)
            args.extend([
                "-p".to_string(),
                "-".to_string(),
                "--input-format".to_string(),
                "stream-json".to_string(),
            ]);
        }

        append_claude_permission_args(&mut args, Some(&config.agent_type), None);

        // Optional settings JSON passed to claude CLI via --settings.
        // Uses agent_type for profile lookup, same as task agents.
        if let Some(s) = get_effective_settings(Some(&config.agent_type)) {
            tracing::debug!(agent_type = %config.agent_type, "Resolved settings profile for teammate");
            if let Ok(json) = serde_json::to_string(s) {
                args.extend(["--settings".to_string(), json]);
            }
        }

        Ok(args)
    }

    /// Build environment variables for a teammate spawn.
    ///
    /// Returns the team-specific env vars that must be set on the process
    /// in addition to the common spawn env.
    pub fn build_teammate_env_vars(config: &TeammateSpawnConfig) -> HashMap<String, String> {
        let mut env = HashMap::new();

        // Team feature flags (required for agent teams)
        env.insert("CLAUDECODE".to_string(), "1".to_string());
        env.insert(
            "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS".to_string(),
            "1".to_string(),
        );

        // MCP agent type for tool filtering (also available as env fallback)
        env.insert(
            "RALPHX_AGENT_TYPE".to_string(),
            config.mcp_agent_type.clone(),
        );

        // Context/project env vars (propagated from lead to teammates for MCP tool filtering)
        if !config.context.context_id.is_empty() {
            env.insert(
                "RALPHX_CONTEXT_ID".to_string(),
                config.context.context_id.clone(),
            );
        }
        if !config.context.context_type.is_empty() {
            env.insert(
                "RALPHX_CONTEXT_TYPE".to_string(),
                config.context.context_type.clone(),
            );
        }
        if let Some(ref pid) = config.context.project_id {
            if !pid.is_empty() {
                env.insert("RALPHX_PROJECT_ID".to_string(), pid.clone());
            }
        }

        // Merge in any custom env vars from config
        for (key, value) in &config.env {
            env.insert(key.clone(), value.clone());
        }

        env
    }

    /// Spawn a teammate in interactive mode for agent team participation.
    ///
    /// Unlike `spawn_agent` (print mode with `-p`), this spawns an interactive session:
    /// - No `-p` flag — the teammate stays alive for multi-turn messaging
    /// - stdin is piped for message injection
    /// - Team env vars (`CLAUDECODE=1`, `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`)
    /// - Team CLI flags (`--agent-id`, `--agent-name`, `--team-name`, etc.)
    /// - Role prompt via `--append-system-prompt`
    ///
    /// The caller is responsible for:
    /// 1. Writing messages to the returned stdin pipe
    /// 2. Processing stdout for stream-json events
    /// 3. Monitoring the process lifecycle
    /// 4. Sending shutdown_request when done
    pub async fn spawn_teammate_interactive(
        &self,
        config: TeammateSpawnConfig,
    ) -> AgentResult<TeammateSpawnResult> {
        if let Err(err) = ensure_claude_spawn_allowed() {
            return Err(AgentError::SpawnNotAllowed(err));
        }

        // Check if CLI is available
        if !self.cli_path.exists() && which::which(&self.cli_path).is_err() {
            return Err(AgentError::CliNotAvailable(format!(
                "claude CLI not found at {:?}",
                self.cli_path
            )));
        }

        let args = self
            .build_teammate_cli_args(&config)
            .map_err(|e| AgentError::SpawnFailed(e))?;
        let team_env = Self::build_teammate_env_vars(&config);

        // Build command
        let mut cmd = tokio::process::Command::new(&self.cli_path);
        cmd.args(&args)
            .current_dir(&config.working_directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()) // Piped and drained in background at call site to capture crash messages
            .stdin(Stdio::piped()); // Piped for message injection (NOT null)

        // Apply common RalphX spawn env vars
        apply_common_spawn_env(&mut cmd);

        // Plugin root env var
        if let Some(plugin_dir) = &config.plugin_dir {
            cmd.env("CLAUDE_PLUGIN_ROOT", plugin_dir);
        }

        // Team-specific env vars
        for (key, value) in &team_env {
            cmd.env(key, value);
        }

        // Spawn the process
        tracing::info!(
            program = %command_log_program(&cmd),
            arg_count = command_log_arg_count(&cmd),
            env_keys = ?command_log_env_keys(&cmd),
            teammate = %config.name,
            team = %config.team_name,
            model = %config.model,
            agent_type = %config.agent_type,
            parent_session_id = %config.parent_session_id,
            "[TEAM_SPAWN] Spawning teammate (interactive) with --parent-session-id"
        );

        let mut child = cmd
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AgentError::SpawnFailed(e.to_string()))?;

        // Take stdin pipe before returning
        let mut stdin = child.stdin.take().ok_or_else(|| {
            AgentError::SpawnFailed("Failed to capture stdin pipe for teammate".to_string())
        })?;

        // Write initial prompt to stdin to activate print mode output.
        // With -p - --input-format stream-json, Claude Code waits for the first stdin
        // message before producing stream-json on stdout. Team inbox messages (SendMessage)
        // are processed but don't generate stdout output until the first stdin turn activates it.
        if !config.prompt.is_empty() && config.print_mode_prompt.is_none() {
            use tokio::io::AsyncWriteExt;
            let formatted = super::format_stream_json_input(&config.prompt);
            stdin.write_all(formatted.as_bytes()).await.map_err(|e| {
                AgentError::SpawnFailed(format!(
                    "Failed to write initial prompt to teammate stdin: {e}"
                ))
            })?;
            stdin.write_all(b"\n").await.map_err(|e| {
                AgentError::SpawnFailed(format!("Failed to write newline to teammate stdin: {e}"))
            })?;
            stdin.flush().await.map_err(|e| {
                AgentError::SpawnFailed(format!("Failed to flush teammate stdin: {e}"))
            })?;
        }

        let handle = AgentHandle::new(
            ClientType::ClaudeCode,
            AgentRole::Custom(format!("teammate:{}", config.name)),
        );

        Ok(TeammateSpawnResult {
            handle,
            child,
            stdin,
        })
    }
}

#[cfg(test)]
#[path = "claude_code_client_tests.rs"]
mod tests;
