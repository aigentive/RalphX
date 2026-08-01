pub(crate) mod app_server_mcp_catalog;
mod codex_cli_client;
pub(crate) mod mcp_catalog;
mod security_policy;
pub mod stream_processor;

#[cfg(test)]
mod app_server_mcp_catalog_tests;

#[cfg(test)]
mod mcp_catalog_tests;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tracing::warn;

use crate::domain::agents::{
    LogicalEffort, CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE,
};
use crate::infrastructure::agents::claude::{
    agent_names, claude_runtime_config, external_mcp_config, filter_interactive_tools,
    format_allowed_tools_arg_value, get_agent_config_for_profile, mcp_agent_type, node_utils,
    validate_mcp_tool_name,
};
use crate::infrastructure::agents::claude::{SpawnableCommand, SpawnableStdinTransport};
use crate::infrastructure::agents::harness_agent_catalog::{
    internal_mcp_server_name, load_harness_agent_prompt_for_profile,
    render_agent_runtime_profile_context, resolve_project_root_from_plugin_dir,
    try_load_canonical_codex_metadata_for_profile, AgentPromptHarness, CanonicalCodexAgentMetadata,
};
use crate::infrastructure::agents::internal_skills::inject_internal_skills_into_system_prompt_for_profile;
use crate::infrastructure::agents::mcp_runtime_context::{
    append_mcp_runtime_args, append_mcp_runtime_query, McpRuntimeContext,
};
use crate::infrastructure::external_mcp_supervisor::{
    ensure_tauri_mcp_bypass_token, TAURI_MCP_BYPASS_TOKEN_ENV,
};
pub use codex_cli_client::{kill_all_tracked_processes, CodexCliClient};
pub(crate) use security_policy::CodexLaunchSecurityPolicy;

const CODEX_PLAN_AGENT_PROFILE: &str = "plan";
const CODEX_APPLY_PATCH_DISABLED_CONFIG_OVERRIDES: &[&str] = &[
    "features.apply_patch_freeform=false",
    "features.apply_patch_streaming_events=false",
    "include_apply_patch_tool=false",
];

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
    pub supports_fast_mode_feature: bool,
    pub fast_mode_supported_models: Vec<String>,
    pub supported_model_aliases: Vec<String>,
    pub supported_efforts: Vec<String>,
    pub model_supported_efforts: BTreeMap<String, Vec<String>>,
    pub ultra_supported_models: Vec<String>,
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

    pub fn supports_fast_mode(&self) -> bool {
        self.supports_fast_mode_feature && !self.fast_mode_supported_models.is_empty()
    }

    pub fn fast_mode_supported_models(&self) -> Vec<String> {
        if self.supports_fast_mode_feature {
            self.fast_mode_supported_models.clone()
        } else {
            Vec::new()
        }
    }

    pub fn supported_effort_labels(&self) -> Vec<String> {
        self.supported_efforts.clone()
    }

    pub fn supports_ultra_for_model(&self, model: &str) -> bool {
        self.ultra_supported_models
            .iter()
            .any(|supported| supported == model)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodexModelCatalogCapabilities {
    pub supported_model_aliases: Vec<String>,
    pub supported_efforts: Vec<String>,
    pub model_supported_efforts: BTreeMap<String, Vec<String>>,
    pub ultra_supported_models: Vec<String>,
}

impl CodexModelCatalogCapabilities {
    fn is_empty(&self) -> bool {
        self.supported_model_aliases.is_empty()
            && self.supported_efforts.is_empty()
            && self.model_supported_efforts.is_empty()
            && self.ultra_supported_models.is_empty()
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
    pub ultra_mode: bool,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub service_tier: Option<String>,
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
            ultra_mode: false,
            approval_policy: Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string()),
            sandbox_mode: Some(CODEX_DEFAULT_SANDBOX_MODE.to_string()),
            service_tier: None,
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

fn effective_codex_approval_policy(policy: CodexLaunchSecurityPolicy) -> &'static str {
    policy.approval_policy()
}

fn effective_codex_sandbox_mode(policy: CodexLaunchSecurityPolicy) -> &'static str {
    policy.sandbox_mode()
}

fn codex_service_tier_overrides(config: &CodexExecCliConfig) -> Result<Vec<String>, String> {
    let Some(service_tier) = config.service_tier.as_deref().map(str::trim) else {
        return Ok(Vec::new());
    };
    if service_tier.is_empty() {
        return Ok(Vec::new());
    }
    if service_tier.eq_ignore_ascii_case("standard") {
        return Ok(Vec::new());
    }

    let mut overrides = vec![format!(
        "service_tier={}",
        encode_codex_string_literal(service_tier)?
    )];
    if service_tier.eq_ignore_ascii_case("fast") {
        overrides.push("features.fast_mode=true".to_string());
    }
    Ok(overrides)
}

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
    build_codex_mcp_overrides_for_profile(
        plugin_dir,
        agent_name,
        None,
        is_external_mcp,
        runtime_context,
    )
}

pub fn build_codex_mcp_overrides_for_profile(
    plugin_dir: &Path,
    agent_name: &str,
    agent_profile: Option<&str>,
    is_external_mcp: bool,
    runtime_context: Option<&CodexMcpRuntimeContext>,
) -> Result<Vec<String>, String> {
    let mcp_server_name = claude_runtime_config().mcp_server_name.clone();
    let short_name = mcp_agent_type(agent_name);
    let project_root = resolve_project_root_from_plugin_dir(plugin_dir);
    let codex_metadata =
        try_load_canonical_codex_metadata_for_profile(&project_root, short_name, agent_profile)?;
    let shell_tool_disabled = codex_metadata.runtime_features.get("shell_tool") == Some(&false);
    if codex_metadata.mcp_transport.as_deref() == Some("external") {
        let mut overrides = build_codex_external_mcp_overrides(
            &mcp_server_name,
            codex_metadata.clone(),
            runtime_context,
        )?;
        if !codex_metadata.internal_mcp_tools.is_empty() {
            overrides.extend(build_codex_internal_mcp_overrides(
                &internal_mcp_server_name(&mcp_server_name),
                plugin_dir,
                short_name,
                agent_profile,
                is_external_mcp,
                runtime_context,
                Some(&codex_metadata.internal_mcp_tools),
            )?);
        }
        append_codex_apply_patch_disable_overrides(
            &mut overrides,
            short_name,
            agent_profile,
            shell_tool_disabled,
        );
        return Ok(overrides);
    }

    let mut overrides = build_codex_internal_mcp_overrides(
        &mcp_server_name,
        plugin_dir,
        short_name,
        agent_profile,
        is_external_mcp,
        runtime_context,
        None,
    )?;

    for (feature_name, enabled) in codex_metadata.runtime_features {
        overrides.push(format!("features.{feature_name}={enabled}"));
    }
    append_codex_apply_patch_disable_overrides(
        &mut overrides,
        short_name,
        agent_profile,
        shell_tool_disabled,
    );

    Ok(overrides)
}

fn append_codex_apply_patch_disable_overrides(
    overrides: &mut Vec<String>,
    agent_name: &str,
    agent_profile: Option<&str>,
    shell_tool_disabled: bool,
) {
    if !shell_tool_disabled
        && agent_profile != Some(CODEX_PLAN_AGENT_PROFILE)
        && agent_name != mcp_agent_type(agent_names::AGENT_PERSONA_EXTRACTOR)
    {
        return;
    }
    // A canonical agent without shell access is read-only across native Codex tools too.
    // Verified on Codex CLI 0.142.5: features list exposes both apply_patch feature
    // gates, while -c accepts the top-level include_apply_patch_tool override.
    overrides.extend(
        CODEX_APPLY_PATCH_DISABLED_CONFIG_OVERRIDES
            .iter()
            .map(|entry| (*entry).to_string()),
    );
}

fn build_codex_internal_mcp_overrides(
    mcp_server_name: &str,
    plugin_dir: &Path,
    short_name: &str,
    agent_profile: Option<&str>,
    is_external_mcp: bool,
    runtime_context: Option<&CodexMcpRuntimeContext>,
    explicit_allowed_tools: Option<&[String]>,
) -> Result<Vec<String>, String> {
    let startup_timeout_secs = external_mcp_config().startup_timeout_secs;
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
        "--trace-dir".to_string(),
        crate::utils::runtime_log_paths::ensure_mcp_proxy_trace_dir()
            .to_string_lossy()
            .into_owned(),
    ];

    if let Some(agent_profile) = agent_profile {
        mcp_args.push("--agent-profile".to_string());
        mcp_args.push(agent_profile.to_string());
    }

    append_mcp_runtime_args(&mut mcp_args, runtime_context);

    let enabled_tools = if let Some(tools) = explicit_allowed_tools {
        Some(valid_codex_mcp_tools(tools, is_external_mcp))
    } else {
        match get_agent_config_for_profile(short_name, agent_profile) {
            Some(config) => Some(valid_codex_mcp_tools(
                &config.allowed_mcp_tools,
                is_external_mcp,
            )),
            None if agent_profile.is_some() => Some(Vec::new()),
            None => None,
        }
    };

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
        format!("mcp_servers.{mcp_server_name}.required=true"),
        format!("mcp_servers.{mcp_server_name}.startup_timeout_sec={startup_timeout_secs}"),
    ];

    if let Some(tools) = enabled_tools {
        overrides.push(format!(
            "mcp_servers.{mcp_server_name}.enabled_tools={}",
            encode_codex_string_array(&tools)?
        ));
    }

    Ok(overrides)
}

fn valid_codex_mcp_tools(tools: &[String], is_external_mcp: bool) -> Vec<String> {
    let valid_tools = tools
        .iter()
        .filter(|name| validate_mcp_tool_name(name))
        .cloned()
        .collect::<Vec<_>>();
    if is_external_mcp {
        filter_interactive_tools(&valid_tools)
    } else {
        valid_tools
    }
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
        format!("mcp_servers.{mcp_server_name}.required=true"),
        format!(
            "mcp_servers.{mcp_server_name}.startup_timeout_sec={}",
            cfg.startup_timeout_secs
        ),
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
    compose_codex_prompt_for_profile(prompt, plugin_dir, agent_name, None, None)
}

pub fn compose_codex_prompt_for_profile(
    prompt: &str,
    plugin_dir: Option<&Path>,
    agent_name: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
) -> String {
    compose_codex_prompt_for_profile_with_outcome(
        prompt,
        plugin_dir,
        agent_name,
        agent_profile,
        persona_block,
    )
    .prompt
}

/// Body-free attribution outcome paired with the composed Codex prompt.
pub struct CodexPromptComposition {
    /// Prompt delivered to the Codex CLI.
    pub prompt: String,
    /// Whether the resolved persona overlay is present in `prompt`.
    pub persona_injected: bool,
    /// Body-free reason when a requested persona overlay could not be composed.
    pub persona_injection_skipped_reason: Option<&'static str>,
}

/// Compose a Codex prompt and report the actual persona overlay outcome.
pub fn compose_codex_prompt_for_profile_with_outcome(
    prompt: &str,
    plugin_dir: Option<&Path>,
    agent_name: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
) -> CodexPromptComposition {
    let Some(plugin_dir) = plugin_dir else {
        return CodexPromptComposition {
            prompt: prompt.to_string(),
            persona_injected: false,
            persona_injection_skipped_reason: persona_block.map(|_| "codex_plugin_dir_unavailable"),
        };
    };
    let Some(agent_name) = agent_name else {
        return CodexPromptComposition {
            prompt: prompt.to_string(),
            persona_injected: false,
            persona_injection_skipped_reason: persona_block.map(|_| "codex_agent_unavailable"),
        };
    };

    let project_root = resolve_project_root_from_plugin_dir(plugin_dir);
    let system_prompt = load_harness_agent_prompt_for_profile(
        &project_root,
        agent_name,
        AgentPromptHarness::Codex,
        agent_profile,
    );
    let Some(system_prompt) = system_prompt else {
        return CodexPromptComposition {
            prompt: prompt.to_string(),
            persona_injected: false,
            persona_injection_skipped_reason: persona_block
                .map(|_| "codex_agent_prompt_unavailable"),
        };
    };
    let persona_injected = persona_block.is_some();
    let system_prompt = super::persona_overlay::apply_persona_overlay(system_prompt, persona_block);
    let runtime_profile_context =
        render_agent_runtime_profile_context(&project_root, agent_name, agent_profile);
    let system_prompt = match inject_internal_skills_into_system_prompt_for_profile(
        &project_root,
        agent_name,
        agent_profile,
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
    let system_prompt = match runtime_profile_context {
        Some(context) => format!("{system_prompt}\n\n{context}"),
        None => system_prompt,
    };

    CodexPromptComposition {
        prompt: format!(
            "<ralphx_agent_instructions>\n{system_prompt}\n</ralphx_agent_instructions>\n\n{prompt}"
        ),
        persona_injected,
        persona_injection_skipped_reason: None,
    }
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
    features_output: Option<&str>,
    refreshed_model_catalog_output: Option<&str>,
    bundled_model_catalog_output: Option<&str>,
) -> CodexCliCapabilities {
    let supports_fast_mode_feature = features_output
        .map(parse_codex_fast_mode_feature)
        .unwrap_or(false);
    let fast_mode_supported_models = if supports_fast_mode_feature {
        parse_codex_fast_mode_supported_models_from_catalogs(
            refreshed_model_catalog_output,
            bundled_model_catalog_output,
        )
    } else {
        Vec::new()
    };
    let model_catalog_capabilities = parse_best_codex_model_catalog(
        refreshed_model_catalog_output,
        bundled_model_catalog_output,
    );

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
        supports_fast_mode_feature,
        fast_mode_supported_models,
        supported_model_aliases: model_catalog_capabilities.supported_model_aliases,
        supported_efforts: model_catalog_capabilities.supported_efforts,
        model_supported_efforts: model_catalog_capabilities.model_supported_efforts,
        ultra_supported_models: model_catalog_capabilities.ultra_supported_models,
    }
}

fn parse_best_codex_model_catalog(
    refreshed_model_catalog_output: Option<&str>,
    bundled_model_catalog_output: Option<&str>,
) -> CodexModelCatalogCapabilities {
    let refreshed = refreshed_model_catalog_output
        .map(parse_codex_model_catalog_capabilities)
        .unwrap_or_default();
    if !refreshed.is_empty() {
        return refreshed;
    }

    bundled_model_catalog_output
        .map(parse_codex_model_catalog_capabilities)
        .unwrap_or_default()
}

fn parse_codex_fast_mode_supported_models_from_catalogs(
    refreshed_model_catalog_output: Option<&str>,
    bundled_model_catalog_output: Option<&str>,
) -> Vec<String> {
    let mut supported_models = Vec::new();
    for output in [refreshed_model_catalog_output, bundled_model_catalog_output]
        .into_iter()
        .flatten()
    {
        supported_models.extend(parse_codex_fast_mode_supported_models(output));
    }
    supported_models.sort();
    supported_models.dedup();
    supported_models
}

pub fn parse_codex_model_catalog_capabilities(output: &str) -> CodexModelCatalogCapabilities {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(output) else {
        return CodexModelCatalogCapabilities::default();
    };
    let Some(models) = root.get("models").and_then(serde_json::Value::as_array) else {
        return CodexModelCatalogCapabilities::default();
    };

    let mut supported_model_aliases = Vec::new();
    let mut supported_efforts = Vec::new();
    let mut model_supported_efforts = BTreeMap::new();
    let mut ultra_supported_models = Vec::new();

    for model in models {
        if !codex_model_is_visible_list_entry(model) {
            continue;
        }
        let Some(slug) = model
            .get("slug")
            .or_else(|| model.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
        else {
            continue;
        };

        supported_model_aliases.push(slug.to_string());
        if let Some(aliases) = model.get("aliases").and_then(serde_json::Value::as_array) {
            supported_model_aliases.extend(aliases.iter().filter_map(|alias| {
                alias
                    .as_str()
                    .map(str::trim)
                    .filter(|alias| !alias.is_empty())
                    .map(str::to_string)
            }));
        }

        let mut model_efforts = model
            .get("supported_reasoning_levels")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|level| {
                let effort = level.get("effort").and_then(serde_json::Value::as_str)?;
                normalize_codex_reasoning_effort(effort)
            })
            .collect::<Vec<_>>();
        if model_efforts.iter().any(|effort| effort == "ultra") {
            ultra_supported_models.push(slug.to_string());
            model_efforts.retain(|effort| effort != "ultra");
        }
        sort_codex_reasoning_efforts(&mut model_efforts);
        supported_efforts.extend(model_efforts.iter().cloned());
        model_supported_efforts.insert(slug.to_string(), model_efforts);
    }

    supported_model_aliases.sort();
    supported_model_aliases.dedup();
    sort_codex_reasoning_efforts(&mut supported_efforts);
    ultra_supported_models.sort();
    ultra_supported_models.dedup();

    CodexModelCatalogCapabilities {
        supported_model_aliases,
        supported_efforts,
        model_supported_efforts,
        ultra_supported_models,
    }
}

fn codex_model_is_visible_list_entry(model: &serde_json::Value) -> bool {
    model
        .get("visibility")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|visibility| visibility.eq_ignore_ascii_case("list"))
}

fn normalize_codex_reasoning_effort(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if codex_reasoning_effort_order(&normalized).is_some() {
        return Some(normalized);
    }
    warn!(
        effort = value,
        "Ignoring unknown Codex reasoning effort in model catalog"
    );
    None
}

fn sort_codex_reasoning_efforts(efforts: &mut Vec<String>) {
    efforts.sort_by_key(|effort| codex_reasoning_effort_order(effort).unwrap_or(u8::MAX));
    efforts.dedup();
}

fn codex_reasoning_effort_order(effort: &str) -> Option<u8> {
    match effort {
        "low" => Some(0),
        "medium" => Some(1),
        "high" => Some(2),
        "xhigh" => Some(3),
        "max" => Some(4),
        "ultra" => Some(5),
        _ => None,
    }
}

pub fn parse_codex_fast_mode_feature(output: &str) -> bool {
    output.lines().any(|line| {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            return false;
        };
        if name != "fast_mode" {
            return false;
        }
        parts
            .last()
            .is_some_and(|enabled| enabled.eq_ignore_ascii_case("true"))
    })
}

pub fn parse_codex_fast_mode_supported_models(output: &str) -> Vec<String> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(output) else {
        return Vec::new();
    };
    let Some(models) = root.get("models").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };

    let mut supported_models = models
        .iter()
        .filter_map(|model| {
            let slug = model
                .get("slug")
                .or_else(|| model.get("id"))
                .and_then(serde_json::Value::as_str)?;
            model_supports_codex_fast_mode(model).then(|| slug.to_string())
        })
        .collect::<Vec<_>>();
    supported_models.sort();
    supported_models.dedup();
    supported_models
}

fn model_supports_codex_fast_mode(model: &serde_json::Value) -> bool {
    model
        .get("additional_speed_tiers")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|tiers| {
            tiers.iter().any(|tier| {
                tier.as_str()
                    .is_some_and(|tier| tier.eq_ignore_ascii_case("fast"))
            })
        })
        || model
            .get("service_tiers")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|tiers| {
                tiers.iter().any(|tier| {
                    let id = tier
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let name = tier
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    id.eq_ignore_ascii_case("fast")
                        || id.eq_ignore_ascii_case("priority")
                        || name.eq_ignore_ascii_case("fast")
                })
            })
}

pub fn probe_codex_cli(cli_path: &Path) -> Result<CodexCliCapabilities, String> {
    let version_output = run_codex_command(cli_path, &["--version"])?;
    let root_help = run_codex_command(cli_path, &["--help"])?;
    let exec_help = run_codex_optional_command(cli_path, &["exec", "--help"]);
    let features_output = run_codex_optional_command(cli_path, &["features", "list"]);
    let refreshed_model_catalog_output = run_codex_optional_command(cli_path, &["debug", "models"]);
    let bundled_model_catalog_output =
        run_codex_optional_command(cli_path, &["debug", "models", "--bundled"]);
    Ok(parse_codex_cli_capabilities(
        &root_help,
        &exec_help,
        Some(&version_output),
        Some(&features_output),
        Some(&refreshed_model_catalog_output),
        Some(&bundled_model_catalog_output),
    ))
}

pub fn resolve_codex_cli() -> Result<ResolvedCodexCli, String> {
    resolve_codex_cli_from_candidates(
        crate::infrastructure::tool_paths::find_codex_cli_candidates(),
    )
}

fn resolve_codex_cli_from_candidates<I>(candidates: I) -> Result<ResolvedCodexCli, String>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut first_incompatible: Option<ResolvedCodexCli> = None;
    let mut errors = Vec::new();

    for path in candidates {
        match probe_codex_cli(&path) {
            Ok(capabilities) if capabilities.has_core_exec_support() => {
                return Ok(ResolvedCodexCli { path, capabilities });
            }
            Ok(capabilities) => {
                if first_incompatible.is_none() {
                    first_incompatible = Some(ResolvedCodexCli { path, capabilities });
                }
            }
            Err(error) => {
                errors.push(format!("{}: {}", path.display(), error));
            }
        }
    }

    if let Some(resolved) = first_incompatible {
        return Ok(resolved);
    }

    if errors.is_empty() {
        Err("Codex CLI not found".to_string())
    } else {
        Err(format!(
            "No launchable Codex CLI could be probed: {}",
            errors.join("; ")
        ))
    }
}

pub fn build_codex_exec_args(
    capabilities: &CodexCliCapabilities,
    config: &CodexExecCliConfig,
) -> Result<Vec<String>, String> {
    build_codex_exec_args_with_security_policy(
        capabilities,
        config,
        CodexLaunchSecurityPolicy::McpCompatibility,
    )
}

fn build_codex_exec_args_with_security_policy(
    capabilities: &CodexCliCapabilities,
    config: &CodexExecCliConfig,
    security_policy: CodexLaunchSecurityPolicy,
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

    require_capability(capabilities.supports_sandbox_flag, "sandbox_flag")?;
    args.push("-s".to_string());
    args.push(normalize_cli_token(effective_codex_sandbox_mode(
        security_policy,
    )));

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

    for override_value in codex_service_tier_overrides(config)? {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(override_value);
    }

    if let Some(reasoning_effort) = effective_codex_reasoning_effort(config) {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort=\"{}\"", reasoning_effort));
    }

    require_capability(capabilities.supports_config_override, "config_override")?;
    args.push("-c".to_string());
    args.push(format!(
        "approval_policy=\"{}\"",
        normalize_cli_token(effective_codex_approval_policy(security_policy))
    ));

    Ok(args)
}

pub fn build_codex_exec_resume_args(
    capabilities: &CodexCliCapabilities,
    session_id: &str,
    config: &CodexExecCliConfig,
) -> Result<Vec<String>, String> {
    build_codex_exec_resume_args_with_security_policy(
        capabilities,
        session_id,
        config,
        CodexLaunchSecurityPolicy::McpCompatibility,
    )
}

fn build_codex_exec_resume_args_with_security_policy(
    capabilities: &CodexCliCapabilities,
    session_id: &str,
    config: &CodexExecCliConfig,
    security_policy: CodexLaunchSecurityPolicy,
) -> Result<Vec<String>, String> {
    if !capabilities.supports_exec_subcommand {
        return Err("Codex CLI does not advertise the exec subcommand".to_string());
    }
    if !capabilities.supports_resume_subcommand {
        return Err("Codex CLI does not advertise the resume subcommand".to_string());
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

    for override_value in codex_service_tier_overrides(config)? {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(override_value);
    }

    if let Some(reasoning_effort) = effective_codex_reasoning_effort(config) {
        require_capability(capabilities.supports_config_override, "config_override")?;
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort=\"{}\"", reasoning_effort));
    }

    require_capability(capabilities.supports_config_override, "config_override")?;
    args.push("-c".to_string());
    args.push(format!(
        "approval_policy=\"{}\"",
        normalize_cli_token(effective_codex_approval_policy(security_policy))
    ));

    require_capability(capabilities.supports_config_override, "config_override")?;
    args.push("-c".to_string());
    args.push(format!(
        "sandbox_mode=\"{}\"",
        normalize_cli_token(effective_codex_sandbox_mode(security_policy))
    ));

    Ok(args)
}

fn effective_codex_reasoning_effort(config: &CodexExecCliConfig) -> Option<LogicalEffort> {
    if config.ultra_mode {
        return Some(LogicalEffort::Ultra);
    }
    config.reasoning_effort.map(|effort| match effort {
        LogicalEffort::Ultra => LogicalEffort::Max,
        ordinary => ordinary,
    })
}

pub fn build_spawnable_codex_exec_command(
    cli_path: &Path,
    prompt: &str,
    capabilities: &CodexCliCapabilities,
    config: &CodexExecCliConfig,
) -> Result<SpawnableCommand, String> {
    build_spawnable_codex_exec_command_with_security_policy(
        cli_path,
        prompt,
        capabilities,
        config,
        CodexLaunchSecurityPolicy::McpCompatibility,
    )
}

pub(crate) fn build_spawnable_codex_exec_command_with_security_policy(
    cli_path: &Path,
    prompt: &str,
    capabilities: &CodexCliCapabilities,
    config: &CodexExecCliConfig,
    security_policy: CodexLaunchSecurityPolicy,
) -> Result<SpawnableCommand, String> {
    let args = build_codex_exec_args_with_security_policy(capabilities, config, security_policy)?;
    let mut cmd = tokio::process::Command::new(cli_path);
    cmd.args(args);
    cmd.arg("--");
    cmd.arg(prompt);
    let prompt_arg_index = cmd.as_std().get_args().count().saturating_sub(1);
    let stdin_transport = configure_spawn(
        &mut cmd,
        config.cwd.as_deref(),
        CodexPromptTransport::PositionalArg,
    );
    Ok(attach_codex_prompt_debug_artifact(
        SpawnableCommand::new_with_stdin_transport(cmd, None, stdin_transport),
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
    build_spawnable_codex_resume_command_with_security_policy(
        cli_path,
        session_id,
        prompt,
        capabilities,
        config,
        CodexLaunchSecurityPolicy::McpCompatibility,
    )
}

pub(crate) fn build_spawnable_codex_resume_command_with_security_policy(
    cli_path: &Path,
    session_id: &str,
    prompt: &str,
    capabilities: &CodexCliCapabilities,
    config: &CodexExecCliConfig,
    security_policy: CodexLaunchSecurityPolicy,
) -> Result<SpawnableCommand, String> {
    let args = build_codex_exec_resume_args_with_security_policy(
        capabilities,
        session_id,
        config,
        security_policy,
    )?;
    let mut cmd = tokio::process::Command::new(cli_path);
    cmd.args(args);
    cmd.arg("--");
    cmd.arg(prompt);
    let prompt_arg_index = cmd.as_std().get_args().count().saturating_sub(1);
    let stdin_transport = configure_spawn(
        &mut cmd,
        config.cwd.as_deref(),
        CodexPromptTransport::PositionalArg,
    );
    Ok(attach_codex_prompt_debug_artifact(
        SpawnableCommand::new_with_stdin_transport(cmd, None, stdin_transport),
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
    fs::write(&path, redact_persona_from_codex_prompt(prompt)).map_err(|error| {
        format!(
            "Failed to write Codex prompt log artifact {}: {error}",
            path.display()
        )
    })?;
    Ok(path)
}

fn redact_persona_from_codex_prompt(prompt: &str) -> String {
    const OPEN: &str = "<ralphx_agent_persona>";
    const CLOSE: &str = "</ralphx_agent_persona>";
    const REDACTED: &str = "<ralphx_agent_persona>[redacted]</ralphx_agent_persona>";

    let mut remaining = prompt;
    let mut redacted = String::with_capacity(prompt.len());
    while let Some(start) = remaining.find(OPEN) {
        redacted.push_str(&remaining[..start]);
        let persona_and_rest = &remaining[start + OPEN.len()..];
        let Some(end) = persona_and_rest.find(CLOSE) else {
            redacted.push_str(REDACTED);
            return redacted;
        };
        redacted.push_str(REDACTED);
        remaining = &persona_and_rest[end + CLOSE.len()..];
    }
    redacted.push_str(remaining);
    redacted
}

fn run_codex_command(cli_path: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = StdCommand::new(cli_path);
    command.args(args);
    command.env(
        "PATH",
        crate::infrastructure::tool_paths::agent_subprocess_env_path(),
    );
    crate::infrastructure::tool_paths::ensure_resolved_node_bin_in_path(&mut command);
    crate::infrastructure::subprocess_env_policy::github_cli_env_policy()
        .apply_to_std_command(&mut command);
    let output = command
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

fn run_codex_optional_command(cli_path: &Path, args: &[&str]) -> String {
    run_codex_command(cli_path, args).unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexPromptTransport {
    PositionalArg,
    #[cfg(test)]
    Stdin,
}

fn configure_spawn(
    cmd: &mut tokio::process::Command,
    cwd: Option<&Path>,
    prompt_transport: CodexPromptTransport,
) -> SpawnableStdinTransport {
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    // Inject the user's login-shell env FIRST so provider auth exports
    // (`OPENAI_API_KEY`, `CODEX_HOME`, ...) reach the spawned CLI. The
    // RalphX-managed `PATH` override below remains authoritative because
    // `login_shell_env::should_forward` filters PATH out of the captured map.
    crate::infrastructure::login_shell_env::apply_to(cmd);
    cmd.env(
        "PATH",
        crate::infrastructure::tool_paths::agent_subprocess_env_path(),
    );
    cmd.env(
        "RALPHX_AGENT_SCREENSHOT_DIR",
        crate::utils::runtime_log_paths::agent_screenshot_dir(),
    );
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let stdin_transport = match prompt_transport {
        CodexPromptTransport::PositionalArg => {
            cmd.stdin(std::process::Stdio::null());
            SpawnableStdinTransport::Null
        }
        #[cfg(test)]
        CodexPromptTransport::Stdin => {
            cmd.stdin(std::process::Stdio::piped());
            SpawnableStdinTransport::Piped
        }
    };
    crate::infrastructure::tool_paths::ensure_resolved_node_bin_in_path(cmd.as_std_mut());
    // Put Codex (and its descendants — MCP server, any subprocesses it
    // spawns) into their own process group so the Tauri exit handler can
    // SIGTERM the whole tree without risking the app itself. See
    // `crate::infrastructure::agents::spawn_isolation`.
    crate::infrastructure::agents::spawn_isolation::install_setsid_pre_exec_tokio(cmd);
    stdin_transport
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
