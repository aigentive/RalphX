mod codex_cli_client;
pub mod stream_processor;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tracing::warn;

use crate::domain::agents::LogicalEffort;
use crate::infrastructure::agents::claude::SpawnableCommand;
use crate::infrastructure::agents::claude::{
    claude_runtime_config, external_mcp_config, filter_interactive_tools,
    format_allowed_tools_arg_value, get_agent_config, mcp_agent_type, node_utils,
    validate_mcp_tool_name,
};
use crate::infrastructure::agents::harness_agent_catalog::{
    load_canonical_codex_metadata, load_harness_agent_prompt, resolve_project_root_from_plugin_dir,
    AgentPromptHarness, CanonicalCodexAgentMetadata,
};
use crate::infrastructure::agents::internal_skills::inject_internal_skills_into_system_prompt;
use crate::infrastructure::agents::mcp_runtime_context::{
    append_mcp_runtime_query, McpRuntimeContext,
};
use crate::infrastructure::external_mcp_supervisor::{
    ensure_tauri_mcp_bypass_token, TAURI_MCP_BYPASS_TOKEN_ENV,
};
pub use codex_cli_client::CodexCliClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexCliCapabilities {
    pub version: Option<String>,
    pub supports_exec_subcommand: bool,
    pub supports_json_output: bool,
    pub supports_model_flag: bool,
    pub supports_config_override: bool,
    pub supports_sandbox_flag: bool,
    pub supports_add_dir: bool,
    pub supports_search_flag: bool,
    pub supports_resume_subcommand: bool,
    pub supports_mcp_subcommand: bool,
}

impl CodexCliCapabilities {
    pub fn has_core_exec_support(&self) -> bool {
        self.missing_core_exec_features().is_empty()
    }

    pub fn missing_core_exec_features(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.supports_exec_subcommand {
            missing.push("exec_subcommand");
        }
        if !self.supports_json_output {
            missing.push("json_output");
        }
        if !self.supports_model_flag {
            missing.push("model_flag");
        }
        if !self.supports_config_override {
            missing.push("config_override");
        }
        if !self.supports_sandbox_flag {
            missing.push("sandbox_flag");
        }
        if !self.supports_add_dir {
            missing.push("add_dir");
        }
        missing
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCodexCli {
    pub path: PathBuf,
    pub capabilities: CodexCliCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecCliConfig {
    pub model: Option<String>,
    pub reasoning_effort: Option<LogicalEffort>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub config_overrides: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub add_dirs: Vec<PathBuf>,
    pub skip_git_repo_check: bool,
    pub json_output: bool,
    pub search: bool,
}

impl Default for CodexExecCliConfig {
    fn default() -> Self {
        Self {
            model: None,
            reasoning_effort: None,
            approval_policy: None,
            sandbox_mode: None,
            config_overrides: Vec::new(),
            cwd: None,
            add_dirs: Vec::new(),
            skip_git_repo_check: false,
            json_output: true,
            search: false,
        }
    }
}

pub type CodexMcpRuntimeContext = McpRuntimeContext;

fn encode_codex_string_literal(value: &str) -> Result<String, String> {
    serde_json::to_string(value)
        .map_err(|error| format!("Failed to encode Codex string literal: {error}"))
}

fn encode_codex_string_array(values: &[String]) -> Result<String, String> {
    serde_json::to_string(values)
        .map_err(|error| format!("Failed to encode Codex array literal: {error}"))
}

pub fn build_codex_mcp_overrides(
    plugin_dir: &Path,
    agent_name: &str,
    is_external_mcp: bool,
    runtime_context: Option<&CodexMcpRuntimeContext>,
) -> Result<Vec<String>, String> {
    let mcp_server_name = claude_runtime_config().mcp_server_name.clone();
    let short_name = mcp_agent_type(agent_name);
    let project_root = resolve_project_root_from_plugin_dir(plugin_dir);
    let codex_metadata = load_canonical_codex_metadata(&project_root, short_name);
    if codex_metadata.mcp_transport.as_deref() == Some("external") {
        return build_codex_external_mcp_overrides(
            &mcp_server_name,
            codex_metadata,
            runtime_context,
        );
    }

    let mcp_server_path = plugin_dir.join("ralphx-mcp-server/build/index.js");

    let node_command = node_utils::find_node_binary()
        .to_string_lossy()
        .into_owned();

    let mut mcp_args = vec![
        mcp_server_path.to_string_lossy().into_owned(),
        "--agent-type".to_string(),
        short_name.to_string(),
        "--tauri-api-url".to_string(),
        crate::utils::backend_endpoint::backend_http_base_url(),
    ];

    if let Some(runtime_context) = runtime_context {
        if let Some(context_type) = runtime_context.context_type.as_deref() {
            mcp_args.push("--context-type".to_string());
            mcp_args.push(context_type.to_string());
        }
        if let Some(context_id) = runtime_context.context_id.as_deref() {
            mcp_args.push("--context-id".to_string());
            mcp_args.push(context_id.to_string());
        }
        if let Some(task_id) = runtime_context.task_id.as_deref() {
            mcp_args.push("--task-id".to_string());
            mcp_args.push(task_id.to_string());
        }
        if let Some(project_id) = runtime_context.project_id.as_deref() {
            mcp_args.push("--project-id".to_string());
            mcp_args.push(project_id.to_string());
        }
        if let Some(working_directory) = runtime_context.working_directory.as_ref() {
            mcp_args.push("--working-directory".to_string());
            mcp_args.push(working_directory.to_string_lossy().into_owned());
        }
        if let Some(lead_session_id) = runtime_context.lead_session_id.as_deref() {
            mcp_args.push("--lead-session-id".to_string());
            mcp_args.push(lead_session_id.to_string());
        }
    }

    let enabled_tools = get_agent_config(short_name).map(|config| {
        let tools: Vec<String> = config
            .allowed_mcp_tools
            .iter()
            .filter(|name| validate_mcp_tool_name(name))
            .cloned()
            .collect();
        if is_external_mcp {
            filter_interactive_tools(&tools)
        } else {
            tools
        }
    });

    if let Some(arg_value) = format_allowed_tools_arg_value(enabled_tools.as_deref()) {
        mcp_args.push(format!("--allowed-tools={arg_value}"));
    }

    let mut overrides = vec![
        format!(
            "mcp_servers.{mcp_server_name}.command={}",
            encode_codex_string_literal(&node_command)?
        ),
        format!(
            "mcp_servers.{mcp_server_name}.args={}",
            encode_codex_string_array(&mcp_args)?
        ),
        format!("mcp_servers.{mcp_server_name}.enabled=true"),
    ];

    if let Some(tools) = enabled_tools {
        overrides.push(format!(
            "mcp_servers.{mcp_server_name}.enabled_tools={}",
            encode_codex_string_array(&tools)?
        ));
    }

    for (feature_name, enabled) in codex_metadata.runtime_features {
        overrides.push(format!("features.{feature_name}={enabled}"));
    }

    Ok(overrides)
}

fn build_codex_external_mcp_overrides(
    mcp_server_name: &str,
    codex_metadata: CanonicalCodexAgentMetadata,
    runtime_context: Option<&CodexMcpRuntimeContext>,
) -> Result<Vec<String>, String> {
    let cfg = external_mcp_config();
    let _token = ensure_tauri_mcp_bypass_token();
    let mut url = format!("http://{}:{}/mcp", cfg.host, cfg.port);
    append_mcp_runtime_query(&mut url, runtime_context);
    let mut overrides = vec![
        format!(
            "mcp_servers.{mcp_server_name}.url={}",
            encode_codex_string_literal(&url)?
        ),
        format!(
            "mcp_servers.{mcp_server_name}.bearer_token_env_var={}",
            encode_codex_string_literal(TAURI_MCP_BYPASS_TOKEN_ENV)?
        ),
        format!("mcp_servers.{mcp_server_name}.enabled=true"),
    ];

    if !codex_metadata.mcp_tools.is_empty() {
        overrides.push(format!(
            "mcp_servers.{mcp_server_name}.enabled_tools={}",
            encode_codex_string_array(&codex_metadata.mcp_tools)?
        ));
    }

    for (feature_name, enabled) in codex_metadata.runtime_features {
        overrides.push(format!("features.{feature_name}={enabled}"));
    }

    Ok(overrides)
}

pub fn compose_codex_prompt(
    prompt: &str,
    plugin_dir: Option<&Path>,
    agent_name: Option<&str>,
) -> String {
    let Some(plugin_dir) = plugin_dir else {
        return prompt.to_string();
    };
    let Some(agent_name) = agent_name else {
        return prompt.to_string();
    };

    let project_root = resolve_project_root_from_plugin_dir(plugin_dir);
    let system_prompt =
        load_harness_agent_prompt(&project_root, agent_name, AgentPromptHarness::Codex);
    let Some(system_prompt) = system_prompt else {
        return prompt.to_string();
    };
    let system_prompt = match inject_internal_skills_into_system_prompt(
        &project_root,
        agent_name,
        &system_prompt,
        prompt,
    ) {
        Ok(injection) => injection.system_prompt,
        Err(error) => {
            warn!(
                agent = agent_name,
                error = %error,
                "Failed to inject internal skills into Codex prompt"
            );
            system_prompt
        }
    };

    format!(
        "<ralphx_agent_instructions>\n{system_prompt}\n</ralphx_agent_instructions>\n\n{prompt}"
    )
}

pub fn normalize_codex_exec_output(raw_stdout: &str) -> String {
    let mut parsed_any = false;
    let mut messages = Vec::new();
    let mut errors = Vec::new();

    for line in raw_stdout.lines() {
        let Some(event) = stream_processor::parse_codex_event_line(line) else {
            continue;
        };
        parsed_any = true;

        if let Some(text) = stream_processor::extract_codex_agent_message(&event) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                messages.push(trimmed.to_string());
            }
        }

        if let Some(command_execution) = stream_processor::extract_codex_command_execution(&event) {
            if let Some(exit_code) = command_execution.exit_code {
                if exit_code != 0 {
                    let error = command_execution
                        .aggregated_output
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            format!("Codex command_execution failed with exit code {exit_code}")
                        });
                    errors.push(error);
                }
            }
        }

        if let Some(error) = stream_processor::extract_codex_error_message(&event) {
            if stream_processor::is_non_fatal_mcp_resource_probe_error(&event, &error) {
                continue;
            }
            let trimmed = error.trim();
            if !trimmed.is_empty() {
                errors.push(trimmed.to_string());
            }
        }
    }

    if !messages.is_empty() {
        return messages.join("\n\n");
    }

    if !errors.is_empty() {
        return errors.join("\n\n");
    }

    if parsed_any {
        return raw_stdout.trim().to_string();
    }

    raw_stdout.to_string()
}

pub fn find_codex_cli() -> Option<PathBuf> {
    crate::infrastructure::tool_paths::find_codex_cli_path()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub fn parse_codex_version(output: &str) -> Option<String> {
    let mut parts = output.split_whitespace();
    let binary = parts.next()?;
    let version = parts.next()?;
    if binary == "codex-cli" {
        Some(version.to_string())
    } else {
        None
    }
}

pub fn parse_codex_cli_capabilities(
    root_help: &str,
    exec_help: &str,
    version_output: Option<&str>,
) -> CodexCliCapabilities {
    CodexCliCapabilities {
        version: version_output.and_then(parse_codex_version),
        supports_exec_subcommand: root_help.contains("exec"),
        supports_json_output: exec_help.contains("--json"),
        supports_model_flag: root_help.contains("--model") && exec_help.contains("--model"),
        supports_config_override: root_help.contains("--config") && exec_help.contains("--config"),
        supports_sandbox_flag: root_help.contains("--sandbox") && exec_help.contains("--sandbox"),
        supports_add_dir: root_help.contains("--add-dir") && exec_help.contains("--add-dir"),
        supports_search_flag: root_help.contains("--search"),
        supports_resume_subcommand: root_help.contains("resume"),
        supports_mcp_subcommand: root_help.contains("mcp"),
    }
}

pub fn probe_codex_cli(cli_path: &Path) -> Result<CodexCliCapabilities, String> {
    let version_output = run_codex_command(cli_path, &["--version"])?;
    let root_help = run_codex_command(cli_path, &["--help"])?;
    let exec_help = run_codex_command(cli_path, &["exec", "--help"])?;
    Ok(parse_codex_cli_capabilities(
        &root_help,
        &exec_help,
        Some(&version_output),
    ))
}

pub fn resolve_codex_cli() -> Result<ResolvedCodexCli, String> {
    let path = find_codex_cli().ok_or_else(|| "Codex CLI not found".to_string())?;
    let capabilities = probe_codex_cli(&path)?;
    Ok(ResolvedCodexCli { path, capabilities })
}

pub fn build_codex_exec_args(
    capabilities: &CodexCliCapabilities,
    config: &CodexExecCliConfig,
) -> Result<Vec<String>, String> {
    if !capabilities.supports_exec_subcommand {
        return Err("Codex CLI does not advertise the exec subcommand".to_string());
    }

    let mut args = vec!["exec".to_string()];

    if config.json_output {
        require_capability(capabilities.supports_json_output, "json_output")?;
        args.push("--json".to_string());
    }

    if let Some(model) = config.model.as_deref() {
        require_capability(capabilities.supports_model_flag, "model_flag")?;
        args.push("-m".to_string());
        args.push(model.to_string());
    }

    if let Some(sandbox_mode) = config.sandbox_mode.as_deref() {
        require_capability(capabilities.supports_sandbox_flag, "sandbox_flag")?;
        args.push("-s".to_string());
        args.push(normalize_cli_token(sandbox_mode));
    }

    if let Some(cwd) = config.cwd.as_ref() {
        args.push("-C".to_string());
        args.push(cwd.to_string_lossy().into_owned());
    }

    for add_dir in &config.add_dirs {
        require_capability(capabilities.supports_add_dir, "add_dir")?;
        args.push("--add-dir".to_string());
        args.push(add_dir.to_string_lossy().into_owned());
    }

    if config.skip_git_repo_check {
        args.push("--skip-git-repo-check".to_string());
    }

    if config.search {
        require_capability(capabilities.supports_search_flag, "search_flag")?;
        args.push("--search".to_string());
    }

    for override_value in &config.config_overrides {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(override_value.clone());
    }

    if let Some(reasoning_effort) = config.reasoning_effort {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort=\"{}\"", reasoning_effort));
    }

    if let Some(approval_policy) = config.approval_policy.as_deref() {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(format!(
            "approval_policy=\"{}\"",
            normalize_cli_token(approval_policy)
        ));
    }

    Ok(args)
}

pub fn build_codex_exec_resume_args(
    capabilities: &CodexCliCapabilities,
    session_id: &str,
    config: &CodexExecCliConfig,
) -> Result<Vec<String>, String> {
    if !capabilities.supports_exec_subcommand {
        return Err("Codex CLI does not advertise the exec subcommand".to_string());
    }

    let mut args = vec![
        "exec".to_string(),
        "resume".to_string(),
        session_id.to_string(),
    ];

    if config.json_output {
        require_capability(capabilities.supports_json_output, "json_output")?;
        args.push("--json".to_string());
    }

    if let Some(model) = config.model.as_deref() {
        require_capability(capabilities.supports_model_flag, "model_flag")?;
        args.push("-m".to_string());
        args.push(model.to_string());
    }

    if config.skip_git_repo_check {
        args.push("--skip-git-repo-check".to_string());
    }

    for override_value in &config.config_overrides {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(override_value.clone());
    }

    if let Some(reasoning_effort) = config.reasoning_effort {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort=\"{}\"", reasoning_effort));
    }

    if let Some(approval_policy) = config.approval_policy.as_deref() {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(format!(
            "approval_policy=\"{}\"",
            normalize_cli_token(approval_policy)
        ));
    }

    if let Some(sandbox_mode) = config.sandbox_mode.as_deref() {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(format!(
            "sandbox_mode=\"{}\"",
            normalize_cli_token(sandbox_mode)
        ));
    }

    Ok(args)
}

pub fn build_spawnable_codex_exec_command(
    cli_path: &Path,
    prompt: &str,
    capabilities: &CodexCliCapabilities,
    config: &CodexExecCliConfig,
) -> Result<SpawnableCommand, String> {
    let args = build_codex_exec_args(capabilities, config)?;
    let mut cmd = tokio::process::Command::new(cli_path);
    cmd.args(args);
    cmd.arg("--");
    cmd.arg(prompt);
    let prompt_arg_index = cmd.as_std().get_args().count().saturating_sub(1);
    configure_spawn(&mut cmd, config.cwd.as_deref());
    Ok(attach_codex_prompt_debug_artifact(
        SpawnableCommand::new(cmd, None),
        prompt,
        prompt_arg_index,
        config.cwd.as_deref(),
        "exec",
    ))
}

pub fn build_spawnable_codex_resume_command(
    cli_path: &Path,
    session_id: &str,
    prompt: &str,
    capabilities: &CodexCliCapabilities,
    config: &CodexExecCliConfig,
) -> Result<SpawnableCommand, String> {
    let args = build_codex_exec_resume_args(capabilities, session_id, config)?;
    let mut cmd = tokio::process::Command::new(cli_path);
    cmd.args(args);
    cmd.arg("--");
    cmd.arg(prompt);
    let prompt_arg_index = cmd.as_std().get_args().count().saturating_sub(1);
    configure_spawn(&mut cmd, config.cwd.as_deref());
    Ok(attach_codex_prompt_debug_artifact(
        SpawnableCommand::new(cmd, None),
        prompt,
        prompt_arg_index,
        config.cwd.as_deref(),
        "resume",
    ))
}

fn attach_codex_prompt_debug_artifact(
    spawnable: SpawnableCommand,
    prompt: &str,
    prompt_arg_index: usize,
    cwd: Option<&Path>,
    mode: &str,
) -> SpawnableCommand {
    match write_codex_prompt_debug_artifact(prompt, cwd, mode) {
        Ok(path) => spawnable.with_prompt_arg_debug_redaction(prompt_arg_index, path),
        Err(error) => {
            warn!(%error, "Failed to persist Codex prompt debug artifact");
            spawnable
        }
    }
}

fn write_codex_prompt_debug_artifact(
    prompt: &str,
    _cwd: Option<&Path>,
    mode: &str,
) -> Result<PathBuf, String> {
    let prompt_dir = crate::utils::runtime_log_paths::codex_prompt_debug_dir();
    fs::create_dir_all(&prompt_dir).map_err(|error| {
        format!(
            "Failed to create Codex prompt log directory {}: {error}",
            prompt_dir.display()
        )
    })?;

    let path = crate::utils::runtime_log_paths::codex_prompt_debug_file(mode);
    fs::write(&path, prompt).map_err(|error| {
        format!(
            "Failed to write Codex prompt log artifact {}: {error}",
            path.display()
        )
    })?;
    Ok(path)
}

fn run_codex_command(cli_path: &Path, args: &[&str]) -> Result<String, String> {
    let output = StdCommand::new(cli_path)
        .args(args)
        .output()
        .map_err(|error| format!("Failed to run {} {:?}: {}", cli_path.display(), args, error))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "Command {} {:?} exited with status {}: {}",
            cli_path.display(),
            args,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn configure_spawn(cmd: &mut tokio::process::Command, cwd: Option<&Path>) {
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.env(
        "PATH",
        crate::infrastructure::tool_paths::agent_subprocess_env_path(),
    );
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::piped());
    crate::infrastructure::tool_paths::prepend_resolved_node_bin_to_path(cmd.as_std_mut());
}

fn require_capability(supported: bool, capability: &str) -> Result<(), String> {
    if supported {
        Ok(())
    } else {
        Err(format!(
            "Codex CLI is missing required capability: {capability}"
        ))
    }
}

fn normalize_cli_token(value: &str) -> String {
    value.trim().replace('_', "-")
}
