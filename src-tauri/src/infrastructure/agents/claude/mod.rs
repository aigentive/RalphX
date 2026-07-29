// Claude Code agent implementations
// Uses the claude CLI for agent interactions

mod agent_config;
pub mod agent_names;
mod claude_code_client;
pub mod cli_capabilities;
pub mod effort_resolver;
mod generated_plugin;
pub(crate) mod mcp_catalog;
pub(crate) mod mcp_registration_repair;
pub mod model_labels;
pub mod model_resolver;
pub mod node_utils;
mod security_policy;
mod stream_processor;

#[cfg(test)]
mod mcp_catalog_tests;
#[cfg(test)]
mod mcp_registration_repair_tests;

pub(crate) use agent_config::configure_runtime_config_dir;
pub use agent_config::live_flags::{
    reset_agent_personas_override_for_test, reset_standalone_conversations_override_for_test,
    set_agent_personas_override, set_standalone_conversations_override,
};
pub use agent_config::process_config::{resolve_process_agent, ProcessMapping, ProcessSlot};
pub use agent_config::{
    agent_configs, agent_harness_defaults_config, agent_personas_enabled, automations_config,
    claude_runtime_config, config_path, defer_merge_enabled,
    execution_defaults_config, external_mcp_config, external_mcp_config_path, file_logging_enabled,
    get_agent_config, get_agent_config_for_profile, get_allowed_tools,
    get_allowed_tools_for_profile, get_effective_settings, get_effective_settings_profile,
    get_preapproved_tools, get_preapproved_tools_for_profile, git_runtime_config,
    ideation_activity_threshold_secs, limits_config, process_mapping, reconciliation_config,
    resolve_file_logging_early, scheduler_config, standalone_conversations_enabled,
    stream_timeouts, supervisor_runtime_config, ui_feature_flags_config,
    validate_external_mcp_config, verification_config, AgentConfig, AgentHarnessDefaultsConfig,
    AllRuntimeConfig, AutomationsRuntimeConfig, ExecutionDefaultsConfig, ExternalMcpConfig,
    GitRuntimeConfig, LimitsConfig, ReconciliationConfig, SchedulerConfig, SpecialistEntry,
    StreamTimeoutsConfig, SupervisorRuntimeConfig, UiFeatureFlagsConfig, VerificationConfig,
};
pub use claude_code_client::kill_all_tracked_processes;
pub use claude_code_client::ClaudeCodeClient;
pub use claude_code_client::{StreamEvent as ClientStreamEvent, StreamingSpawnResult};
pub use cli_capabilities::{
    clear_claude_cli_capability_cache, is_claude_sonnet_5_model,
    normalize_claude_effort_for_cli_path, parse_claude_cli_capabilities, parse_claude_version,
    probe_claude_cli, probe_claude_cli_cached, validate_claude_model_for_cli_path,
    ClaudeCliCapabilities, CLAUDE_SONNET_5_API_MODEL_ID, CLAUDE_SONNET_5_MIN_VERSION,
};

// Re-export stream processor types for use by services
pub use stream_processor::{
    AssistantContent, AssistantMessage, ContentBlock, ContentBlockItem, ContentDelta, DiffContext,
    ParsedLine, StreamEvent, StreamMessage, StreamProcessor, StreamResult, ToolCall, ToolCallStats,
};

// Re-export effort resolver helpers for use by services
pub use effort_resolver::{
    effort_bucket_for_agent, resolve_effort_with_source, resolve_ideation_effort,
};

// Re-export model resolver helpers for use by services
#[allow(unused_imports)]
pub(crate) use generated_plugin::{
    materialize_generated_plugin_dir, materialize_generated_plugin_dir_with_runtime_source,
};
pub use model_resolver::{
    resolve_ideation_model, resolve_ideation_subagent_model_with_source, resolve_model_with_source,
    resolve_verifier_subagent_model_with_source, ResolvedModel,
};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::process::Command;
use tracing::warn;

use crate::domain::agents::{AgentProviderSettings, CLAUDE_DEFAULT_PERMISSION_MODE};
use crate::infrastructure::agents::harness_agent_catalog::{
    internal_mcp_server_name, load_harness_agent_prompt_for_profile,
    render_agent_runtime_profile_context, resolve_harness_agent_prompt_path,
    resolve_project_root_from_plugin_dir, try_load_canonical_claude_metadata_for_profile,
    AgentPromptHarness,
};
use crate::infrastructure::agents::internal_skills::{
    inject_internal_skills_into_system_prompt_for_profile,
    inject_pre_execution_learned_skills_into_existing_injection,
    InternalSkillInjection, PreExecutionLearnedSkillContext,
};
use crate::infrastructure::agents::mcp_runtime_context::{
    append_mcp_runtime_args, append_mcp_runtime_query, McpRuntimeContext,
};
use crate::infrastructure::external_mcp_supervisor::ensure_tauri_mcp_bypass_token;

pub(crate) const PRIMARY_PLUGIN_DIR_REL: &str = "plugins/app";
pub(crate) const LEGACY_PLUGIN_DIR_REL: &str = "ralphx-plugin";
pub(crate) use security_policy::ClaudePermissionPolicy;
#[cfg(test)]
pub(crate) use security_policy::CLAUDE_PROMPT_PERMISSION_MODE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudePromptDelivery {
    NonInteractive,
    Interactive,
}

impl ClaudePromptDelivery {
    const fn is_interactive(self) -> bool {
        matches!(self, Self::Interactive)
    }
}

fn base_plugin_dir_override() -> &'static Mutex<Option<PathBuf>> {
    static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

fn replace_base_plugin_dir_override(next: Option<PathBuf>) -> Option<PathBuf> {
    let mut guard = base_plugin_dir_override()
        .lock()
        .expect("base plugin dir override lock poisoned");
    std::mem::replace(&mut *guard, next)
}

fn configured_base_plugin_dir() -> Option<PathBuf> {
    base_plugin_dir_override()
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

pub fn configure_runtime_plugin_dirs(plugin_dir: PathBuf, generated_plugin_dir: PathBuf) {
    replace_base_plugin_dir_override(Some(plugin_dir));
    generated_plugin::replace_generated_plugin_dir_override(Some(generated_plugin_dir));
}

#[doc(hidden)]
pub struct RuntimePluginDirsOverrideGuard {
    _lock: RuntimePluginDirsOverrideLock,
    previous_plugin_dir: Option<PathBuf>,
    previous_generated_plugin_dir: Option<PathBuf>,
}

#[doc(hidden)]
pub struct RuntimePluginDirsOverrideLock;

impl Drop for RuntimePluginDirsOverrideLock {
    fn drop(&mut self) {
        runtime_plugin_dirs_override_in_use().store(false, Ordering::Release);
    }
}

fn runtime_plugin_dirs_override_in_use() -> &'static AtomicBool {
    static IN_USE: AtomicBool = AtomicBool::new(false);
    &IN_USE
}

fn acquire_runtime_plugin_dirs_override_lock() -> RuntimePluginDirsOverrideLock {
    while runtime_plugin_dirs_override_in_use()
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::thread::yield_now();
    }
    RuntimePluginDirsOverrideLock
}

#[doc(hidden)]
pub fn lock_runtime_plugin_dirs_for_tests() -> RuntimePluginDirsOverrideLock {
    acquire_runtime_plugin_dirs_override_lock()
}

#[doc(hidden)]
pub fn override_runtime_plugin_dirs_for_tests(
    plugin_dir: PathBuf,
    generated_plugin_dir: PathBuf,
) -> RuntimePluginDirsOverrideGuard {
    let lock = acquire_runtime_plugin_dirs_override_lock();
    RuntimePluginDirsOverrideGuard {
        _lock: lock,
        previous_plugin_dir: replace_base_plugin_dir_override(Some(plugin_dir)),
        previous_generated_plugin_dir: generated_plugin::replace_generated_plugin_dir_override(
            Some(generated_plugin_dir),
        ),
    }
}

impl Drop for RuntimePluginDirsOverrideGuard {
    fn drop(&mut self) {
        replace_base_plugin_dir_override(self.previous_plugin_dir.take());
        generated_plugin::replace_generated_plugin_dir_override(
            self.previous_generated_plugin_dir.take(),
        );
    }
}

#[allow(clippy::manual_find)]
fn first_existing_plugin_dir(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| candidate.is_dir())
}

pub(crate) fn find_base_plugin_dir() -> Option<PathBuf> {
    if let Some(plugin_dir) = configured_base_plugin_dir() {
        return Some(plugin_dir);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(manifest_dir);
    if let Some(candidate) = first_existing_plugin_dir([
        repo_root.join(PRIMARY_PLUGIN_DIR_REL),
        repo_root.join(LEGACY_PLUGIN_DIR_REL),
    ]) {
        return Some(candidate);
    }

    // Try relative to executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let candidates = [
                parent.join(PRIMARY_PLUGIN_DIR_REL),
                parent.join(LEGACY_PLUGIN_DIR_REL),
                parent.join(format!("../{PRIMARY_PLUGIN_DIR_REL}")),
                parent.join(format!("../{LEGACY_PLUGIN_DIR_REL}")),
                parent.join(format!("../../{PRIMARY_PLUGIN_DIR_REL}")),
                parent.join(format!("../../{LEGACY_PLUGIN_DIR_REL}")),
                parent.join(format!("../../../{PRIMARY_PLUGIN_DIR_REL}")),
                parent.join(format!("../../../{LEGACY_PLUGIN_DIR_REL}")),
            ];

            if let Some(candidate) = first_existing_plugin_dir(candidates) {
                return Some(candidate);
            }
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if let Some(candidate) = first_existing_plugin_dir([
            PathBuf::from(&home).join(format!(
                "Library/Application Support/com.ralphx.app/{PRIMARY_PLUGIN_DIR_REL}"
            )),
            PathBuf::from(home).join("Library/Application Support/com.ralphx.app/ralphx-plugin"),
        ]) {
            return Some(candidate);
        }
    }

    None
}

/// Apply common Claude CLI environment flags for RalphX-managed spawns.
pub fn apply_common_spawn_env(cmd: &mut Command) {
    apply_common_spawn_env_to_std(cmd.as_std_mut());
}

fn apply_common_spawn_env_to_std(cmd: &mut std::process::Command) {
    // Inject the user's login-shell env FIRST so things like `ANTHROPIC_API_KEY`
    // and other auth exports the user set in `~/.zshrc` / `~/.zprofile` reach
    // the spawned CLI. The RalphX-managed overrides below stay authoritative
    // because they apply after — see `login_shell_env::should_forward`.
    crate::infrastructure::login_shell_env::apply_to_std(cmd);
    cmd.env(
        "PATH",
        crate::infrastructure::tool_paths::agent_subprocess_env_path(),
    );
    cmd.env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
    cmd.env("CLAUDE_CODE_ENABLE_TASKS", "1");
    cmd.env("DEBUG", "true");
    cmd.env(
        "TAURI_API_URL",
        crate::utils::backend_endpoint::backend_http_base_url(),
    );
    cmd.env(
        "RALPHX_AGENT_SCREENSHOT_DIR",
        crate::utils::runtime_log_paths::agent_screenshot_dir(),
    );
    crate::infrastructure::tool_paths::ensure_resolved_node_bin_in_path(cmd);
    // Provider-neutral setsid wrapper — same helper Codex uses. See
    // `crate::infrastructure::agents::spawn_isolation` for rationale.
    crate::infrastructure::agents::spawn_isolation::install_setsid_pre_exec(cmd);
}

/// Normalize legacy short agent ids to the current canonical ids.
pub fn canonical_short_agent_name(name: &str) -> &str {
    let short_name = name.strip_prefix("ralphx:").unwrap_or(name);
    match short_name {
        "orchestrator-ideation" => "ralphx-ideation",
        "orchestrator-ideation-readonly" => "ralphx-ideation-readonly",
        "ideation-advocate" => "ralphx-ideation-advocate",
        "ideation-critic" => "ralphx-ideation-critic",
        "ideation-specialist-backend" => "ralphx-ideation-specialist-backend",
        "ideation-specialist-frontend" => "ralphx-ideation-specialist-frontend",
        "ideation-specialist-infra" => "ralphx-ideation-specialist-infra",
        "chat-task" => "ralphx-chat-task",
        "chat-project" => "ralphx-chat-project",
        "ralphx-worker" => "ralphx-execution-worker",
        "ralphx-coder" => "ralphx-execution-coder",
        "ralphx-reviewer" => "ralphx-execution-reviewer",
        "ralphx-merger" => "ralphx-execution-merger",
        "ralphx-orchestrator" => "ralphx-execution-orchestrator",
        "ralphx-deep-researcher" => "ralphx-research-deep-researcher",
        "project-analyzer" => "ralphx-project-analyzer",
        "memory-capture" => "ralphx-memory-capture",
        "memory-maintainer" => "ralphx-memory-maintainer",
        "session-namer" => "ralphx-utility-session-namer",
        "pr-describer" => "ralphx-utility-pr-describer",
        "plan-complexity" => "ralphx-utility-plan-complexity",
        _ => short_name,
    }
}

/// Qualify a short agent name with the `ralphx:` plugin prefix.
/// If the name already contains `:`, it's assumed to be fully qualified.
pub fn qualify_agent_name(name: &str) -> String {
    if let Some(short_name) = name.strip_prefix("ralphx:") {
        format!("ralphx:{}", canonical_short_agent_name(short_name))
    } else if name.contains(':') {
        name.to_string()
    } else {
        format!("ralphx:{}", canonical_short_agent_name(name))
    }
}

/// Strip the `ralphx:` plugin prefix from an agent name.
/// Used when passing agent type to the MCP server or looking up agent configs
/// (both use short/unprefixed names).
pub fn mcp_agent_type(name: &str) -> &str {
    canonical_short_agent_name(name)
}

/// Build base CLI arguments for Claude Code
/// These are the common args needed for all Claude CLI invocations with streaming output
///
/// When `agent_type` is provided, creates a dynamic MCP config that passes
/// the agent type as a CLI argument to the MCP server. This is necessary because
/// Claude CLI does NOT pass its environment variables to MCP servers it spawns.
fn is_test_environment() -> bool {
    if cfg!(test) {
        return true;
    }

    if std::env::var("RUST_TEST_THREADS").is_ok() {
        return true;
    }

    if let Ok(value) = std::env::var("RALPHX_TEST_MODE") {
        return value == "1" || value.eq_ignore_ascii_case("true");
    }

    false
}

pub fn ensure_claude_spawn_allowed() -> Result<(), String> {
    if let Ok(value) = std::env::var("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS") {
        if value == "1" || value.eq_ignore_ascii_case("true") {
            return Ok(());
        }
    }

    if is_test_environment() {
        return Err("Claude spawn disabled in tests".to_string());
    }

    if let Ok(value) = std::env::var("RALPHX_DISABLE_CLAUDE_SPAWN") {
        if value == "1" || value.eq_ignore_ascii_case("true") {
            return Err("Claude spawn disabled by RALPHX_DISABLE_CLAUDE_SPAWN".to_string());
        }
    }

    Ok(())
}

/// Resolve the `--effort` level for a given agent type.
///
/// Priority: `AgentConfig.effort` > `ClaudeRuntimeConfig.default_effort`
pub fn resolve_effort(agent_type: Option<&str>) -> String {
    let default = claude_runtime_config().default_effort.clone();
    match agent_type {
        Some(name) => get_agent_config(name)
            .and_then(|c| c.effort.clone())
            .unwrap_or(default),
        None => default,
    }
}

/// Resolve the `--model` value for a given agent type.
///
/// Priority: `AgentConfig.model` > hardcoded default `"sonnet"`.
/// Used as the YAML fallback layer (levels 3–4) in the ideation model resolution chain.
pub fn resolve_model(agent_type: Option<&str>) -> String {
    match agent_type {
        Some(name) => get_agent_config(name)
            .and_then(|c| c.model.clone())
            .unwrap_or_else(|| "sonnet".to_string()),
        None => "sonnet".to_string(),
    }
}

/// Resolve the `--permission-mode` for a given agent type.
///
/// Priority: `AgentConfig.permission_mode` > `ClaudeRuntimeConfig.permission_mode`
pub fn resolve_permission_mode(agent_type: Option<&str>) -> String {
    let default = claude_permission_runtime_override()
        .lock()
        .ok()
        .and_then(|guard| {
            guard
                .as_ref()
                .and_then(|value| value.permission_mode.clone())
        })
        .unwrap_or_else(|| claude_runtime_config().permission_mode.clone());
    match agent_type {
        Some(name) => get_agent_config(name)
            .and_then(|c| c.permission_mode.clone())
            .unwrap_or(default),
        None => default,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudePermissionRuntimeOverride {
    pub permission_mode: Option<String>,
    pub dangerously_skip_permissions: bool,
    pub allow_dangerously_skip_permissions: bool,
}

fn claude_permission_runtime_override() -> &'static Mutex<Option<ClaudePermissionRuntimeOverride>> {
    static OVERRIDE: OnceLock<Mutex<Option<ClaudePermissionRuntimeOverride>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

pub(crate) fn set_claude_permission_runtime_override(
    next: Option<ClaudePermissionRuntimeOverride>,
) -> Option<ClaudePermissionRuntimeOverride> {
    claude_permission_runtime_override()
        .lock()
        .ok()
        .and_then(|mut guard| std::mem::replace(&mut *guard, next))
}

pub(crate) fn claude_permission_override_from_provider_settings(
    settings: &AgentProviderSettings,
) -> ClaudePermissionRuntimeOverride {
    ClaudePermissionRuntimeOverride {
        permission_mode: settings
            .claude_permission_mode
            .clone()
            .or_else(|| Some(CLAUDE_DEFAULT_PERMISSION_MODE.to_string())),
        dangerously_skip_permissions: settings.claude_dangerously_skip_permissions,
        allow_dangerously_skip_permissions: settings.claude_allow_dangerously_skip_permissions,
    }
}

pub(crate) fn apply_claude_provider_permission_settings(settings: &AgentProviderSettings) {
    set_claude_permission_runtime_override(Some(
        claude_permission_override_from_provider_settings(settings),
    ));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudePermissionCliOptions {
    pub permission_prompt_tool: String,
    pub permission_mode: String,
    pub dangerously_skip_permissions: bool,
    pub allow_dangerously_skip_permissions: bool,
}

pub(crate) fn resolve_claude_permission_cli_options(
    agent_type: Option<&str>,
    agent_profile: Option<&str>,
) -> ClaudePermissionCliOptions {
    let runtime = claude_runtime_config();
    let override_settings = claude_permission_runtime_override()
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    ClaudePermissionCliOptions {
        // Transport-aware (and profile-aware): external-transport agents whose private
        // tools live on the internal sidecar must point `--permission-prompt-tool` at
        // that sidecar server so the flag matches the injected `--allowed-tools`
        // permission entry resolved under the same profile.
        permission_prompt_tool: agent_config::resolve_permission_prompt_tool(
            agent_type,
            agent_profile,
            &runtime.permission_prompt_tool,
        ),
        permission_mode: resolve_permission_mode(agent_type),
        dangerously_skip_permissions: override_settings
            .as_ref()
            .map(|settings| settings.dangerously_skip_permissions)
            .unwrap_or(runtime.dangerously_skip_permissions),
        allow_dangerously_skip_permissions: override_settings
            .as_ref()
            .map(|settings| settings.allow_dangerously_skip_permissions)
            .unwrap_or(runtime.allow_dangerously_skip_permissions),
    }
}

fn resolve_claude_permission_cli_options_for_policy(
    agent_type: Option<&str>,
    agent_profile: Option<&str>,
    policy: ClaudePermissionPolicy,
) -> ClaudePermissionCliOptions {
    policy.resolve_cli_options(resolve_claude_permission_cli_options(
        agent_type,
        agent_profile,
    ))
}

fn preapproved_tools_for_permission_policy(
    agent_name: &str,
    agent_profile: Option<&str>,
    policy: ClaudePermissionPolicy,
) -> Option<String> {
    let preapproved = get_preapproved_tools_for_profile(agent_name, agent_profile)?;
    policy.filter_preapproved_tools(preapproved)
}

pub(crate) fn append_claude_permission_args(
    args: &mut Vec<String>,
    agent_type: Option<&str>,
    agent_profile: Option<&str>,
) {
    let options = resolve_claude_permission_cli_options(agent_type, agent_profile);
    args.extend([
        "--permission-prompt-tool".to_string(),
        options.permission_prompt_tool,
        "--permission-mode".to_string(),
        options.permission_mode,
    ]);
    if options.allow_dangerously_skip_permissions {
        args.push("--allow-dangerously-skip-permissions".to_string());
    }
    if options.dangerously_skip_permissions {
        args.push("--dangerously-skip-permissions".to_string());
    }
}

fn apply_claude_permission_args(
    cmd: &mut Command,
    agent_type: Option<&str>,
    agent_profile: Option<&str>,
    policy: ClaudePermissionPolicy,
) {
    let options =
        resolve_claude_permission_cli_options_for_policy(agent_type, agent_profile, policy);
    cmd.args([
        "--permission-prompt-tool",
        &options.permission_prompt_tool,
        "--permission-mode",
        &options.permission_mode,
    ]);
    if options.allow_dangerously_skip_permissions {
        cmd.arg("--allow-dangerously-skip-permissions");
    }
    if options.dangerously_skip_permissions {
        cmd.arg("--dangerously-skip-permissions");
    }
}

pub fn build_base_cli_command(
    cli_path: &Path,
    plugin_dir: &Path,
    agent_type: Option<&str>,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<Command, String> {
    build_base_cli_command_inner(
        cli_path,
        plugin_dir,
        agent_type,
        is_external_mcp,
        effort_override,
        model_override,
        true,
    )
}

#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub fn build_base_cli_command_for_test(
    cli_path: &Path,
    plugin_dir: &Path,
    agent_type: Option<&str>,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<Command, String> {
    build_base_cli_command_inner(
        cli_path,
        plugin_dir,
        agent_type,
        is_external_mcp,
        effort_override,
        model_override,
        false,
    )
}

fn build_base_cli_command_inner(
    cli_path: &Path,
    plugin_dir: &Path,
    agent_type: Option<&str>,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    enforce_spawn_guard: bool,
) -> Result<Command, String> {
    build_base_cli_command_inner_with_runtime_context(
        cli_path,
        plugin_dir,
        agent_type,
        is_external_mcp,
        effort_override,
        model_override,
        None,
        enforce_spawn_guard,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_base_cli_command_inner_with_runtime_context(
    cli_path: &Path,
    plugin_dir: &Path,
    agent_type: Option<&str>,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
    enforce_spawn_guard: bool,
) -> Result<Command, String> {
    build_base_cli_command_inner_with_runtime_context_and_profile(
        cli_path,
        plugin_dir,
        agent_type,
        None,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
        enforce_spawn_guard,
        ClaudePermissionPolicy::InheritConfigured,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_base_cli_command_inner_with_runtime_context_and_profile(
    cli_path: &Path,
    plugin_dir: &Path,
    agent_type: Option<&str>,
    agent_profile: Option<&str>,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
    enforce_spawn_guard: bool,
    permission_policy: ClaudePermissionPolicy,
) -> Result<Command, String> {
    if enforce_spawn_guard {
        ensure_claude_spawn_allowed()?;
    }
    let mut cmd = Command::new(cli_path);

    // Apply common environment hardening and debug flags for CLI spawns.
    apply_common_spawn_env(&mut cmd);
    cmd.env("CLAUDE_PLUGIN_ROOT", plugin_dir);

    // Propagate external trigger context so MCP server can set child session origin correctly.
    if is_external_mcp {
        cmd.env("RALPHX_IS_EXTERNAL_TRIGGER", "1");
    }

    // Optional setting-sources override from config/ralphx.yaml.
    if let Some(sources) = &claude_runtime_config().setting_sources {
        if !sources.is_empty() {
            cmd.args(["--setting-sources", &sources.join(",")]);
        }
    }

    // Temporary hardening: disable slash-command skill loading to avoid
    // startup JSON parse crashes in Claude's skill initialization path.
    cmd.arg("--disable-slash-commands");

    // Plugin directory for agent/skill discovery
    cmd.args([
        "--plugin-dir",
        plugin_dir.to_str().unwrap_or("./plugins/app"),
    ]);

    // Output format for streaming JSON
    cmd.args(["--output-format", "stream-json"]);

    // Required for stream-json with -p (print mode)
    cmd.arg("--verbose");

    // Capture Claude's internal debug log per spawn for post-mortem analysis.
    // This is critical when the process exits 0 with no stdout/stderr.
    let debug_path = crate::utils::runtime_log_paths::claude_debug_log_file();
    if let Some(path_str) = debug_path.to_str() {
        cmd.args(["--debug-file", path_str]);
        tracing::debug!(path = %debug_path.display(), "Enabled Claude debug file");
    }

    // Configure permission handling from config/harnesses/claude.yaml.
    apply_claude_permission_args(&mut cmd, agent_type, agent_profile, permission_policy);
    // Optional settings JSON passed to claude CLI via --settings.
    // Agent-specific profile overrides global profile when configured.
    if let Some(s) = get_effective_settings(agent_type) {
        if let Ok(json) = serde_json::to_string(s) {
            cmd.args(["--settings", &json]);
        }
    }

    let agent_profile_config =
        agent_type.and_then(|agent| get_agent_config_for_profile(agent, agent_profile));

    // Effort level for this agent — use explicit override when provided, otherwise resolve from config.
    let effort_resolved;
    let effort = match effort_override {
        Some(e) => e,
        None => {
            effort_resolved = agent_profile_config
                .as_ref()
                .and_then(|config| config.effort.clone())
                .unwrap_or_else(|| claude_runtime_config().default_effort.clone());
            &effort_resolved
        }
    };
    let normalized_effort = normalize_claude_effort_for_cli_path(cli_path, effort);
    if normalized_effort != effort {
        tracing::warn!(
            requested_effort = effort,
            effective_effort = %normalized_effort,
            cli_path = %cli_path.display(),
            "Normalized Claude CLI effort for installed CLI capability"
        );
    }
    cmd.args(["--effort", normalized_effort.as_str()]);

    // Model for this agent — use explicit override when provided, otherwise resolve from agent config.
    let model_resolved;
    let model = match model_override {
        Some(m) => Some(m),
        None => {
            model_resolved = agent_profile_config
                .as_ref()
                .and_then(|cfg| cfg.model.clone());
            model_resolved.as_deref()
        }
    };
    if let Some(m) = model {
        validate_claude_model_for_cli_path(cli_path, m)?;
        cmd.args(["--model", m]);
    }

    // If agent_type is provided, create a dynamic MCP config that passes it
    // to the MCP server via CLI args (since env vars don't propagate to MCP servers).
    // RalphX injects its required server while preserving provider-native servers.
    // Hard error on invalid config — MCP is critical infra, fail loud.
    if let Some(agent) = agent_type {
        let temp_path = create_mcp_config_with_runtime_context_for_profile(
            plugin_dir,
            agent,
            agent_profile,
            is_external_mcp,
            mcp_runtime_context,
        )
        .map_err(|e| {
            tracing::error!(
                error = %e,
                agent = %agent,
                agent_profile = ?agent_profile,
                "MCP config creation failed"
            );
            e
        })?;
        cmd.args(["--mcp-config", temp_path.to_str().unwrap_or("")]);
        tracing::debug!(
            path = %temp_path.display(),
            agent_type = agent,
            agent_profile = ?agent_profile,
            "Dynamic RalphX MCP config written alongside provider-native configuration"
        );
    }

    Ok(cmd)
}

pub(crate) fn resolve_agent_system_prompt_path(
    plugin_dir: &Path,
    agent_name: &str,
) -> Option<PathBuf> {
    let short = mcp_agent_type(agent_name);
    let project_root = resolve_project_root_from_plugin_dir(plugin_dir);
    resolve_harness_agent_prompt_path(&project_root, short, AgentPromptHarness::Claude)
}

fn load_agent_system_prompt_with_internal_skills(
    plugin_dir: &Path,
    agent_name: &str,
    agent_profile: Option<&str>,
    prompt: &str,
    persona_block: Option<&str>,
    pre_execution_learned_skills: Option<&PreExecutionLearnedSkillContext>,
) -> Option<(String, Vec<String>)> {
    let short = mcp_agent_type(agent_name);
    let project_root = resolve_project_root_from_plugin_dir(plugin_dir);
    let system_prompt = load_harness_agent_prompt_for_profile(
        &project_root,
        short,
        AgentPromptHarness::Claude,
        agent_profile,
    )?;
    let system_prompt = super::persona_overlay::apply_persona_overlay(system_prompt, persona_block);
    let runtime_profile_context =
        render_agent_runtime_profile_context(&project_root, short, agent_profile);
    match inject_internal_skills_into_system_prompt_for_profile(
        &project_root,
        short,
        agent_profile,
        &system_prompt,
        prompt,
    ) {
        Ok(injection) => {
            let injection = inject_pre_execution_learned_skills_into_existing_injection(
                injection,
                pre_execution_learned_skills,
            );
            Some((
                append_runtime_profile_context(injection.system_prompt, runtime_profile_context),
                injection.injected_skill_names,
            ))
        }
        Err(error) => {
            warn!(
                agent = agent_name,
                error = %error,
                "Failed to inject internal skills into Claude prompt"
            );
            let injection = inject_pre_execution_learned_skills_into_existing_injection(
                InternalSkillInjection {
                    system_prompt,
                    injected_skill_names: Vec::new(),
                },
                pre_execution_learned_skills,
            );
            Some((
                append_runtime_profile_context(injection.system_prompt, runtime_profile_context),
                injection.injected_skill_names,
            ))
        }
    }
}

fn append_runtime_profile_context(
    system_prompt: String,
    runtime_profile_context: Option<String>,
) -> String {
    match runtime_profile_context {
        Some(context) => format!("{system_prompt}\n\n{context}"),
        None => system_prompt,
    }
}

/// Validate a generated MCP config JSON value for required fields.
///
/// Checks that the config has `mcpServers` and that the named server is either
/// HTTP (`url`) or stdio (`command` + `args`). Returns an error message on failure.
pub(crate) fn validate_mcp_config_json(
    config: &serde_json::Value,
    server_name: &str,
) -> Result<(), String> {
    let mcp_servers = config
        .get("mcpServers")
        .ok_or_else(|| "missing 'mcpServers' key".to_string())?;

    let server = mcp_servers
        .get(server_name)
        .ok_or_else(|| format!("missing server entry '{server_name}' in mcpServers"))?;

    if server.get("url").is_some() {
        return Ok(());
    }

    if server.get("command").is_none() {
        return Err(format!(
            "server '{server_name}' missing required 'command' field"
        ));
    }
    if server.get("args").is_none() {
        return Err(format!(
            "server '{server_name}' missing required 'args' field"
        ));
    }

    Ok(())
}

/// MCP tools that require live human interaction and must be excluded when an agent
/// is spawned from an external (non-interactive) context such as an external MCP request.
/// Without this filter the agent would long-poll for human input that never arrives → deadlock.
pub const INTERACTIVE_TOOLS: &[&str] = &["ask_user_question"];

/// Remove `INTERACTIVE_TOOLS` entries from `tools`, returning a filtered list.
pub fn filter_interactive_tools(tools: &[String]) -> Vec<String> {
    tools
        .iter()
        .filter(|name| !INTERACTIVE_TOOLS.contains(&name.as_str()))
        .cloned()
        .collect()
}

/// Create a dynamic MCP config temp file for an agent.
///
/// Writes a JSON config that starts the configured MCP server with the agent's type
/// passed via `--agent-type` CLI arg (for tool filtering). Returns the temp file path.
/// Uses UUID in filename to avoid race conditions between parallel agent spawns.
///
/// When `is_external_mcp` is `true`, interactive-only tools (see `INTERACTIVE_TOOLS`) are
/// stripped from the `--allowed-tools` arg to prevent deadlocks in unattended contexts.
///
/// # Errors
///
/// Returns `Err` when the config JSON fails validation (missing required fields) or
/// when the temp file cannot be written. Errors propagate to agent spawn failure.
pub fn create_mcp_config(
    plugin_dir: &Path,
    agent_type: &str,
    is_external_mcp: bool,
) -> Result<PathBuf, String> {
    create_mcp_config_with_runtime_context_for_profile(
        plugin_dir,
        agent_type,
        None,
        is_external_mcp,
        None,
    )
}

pub fn create_mcp_config_with_runtime_context(
    plugin_dir: &Path,
    agent_type: &str,
    is_external_mcp: bool,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<PathBuf, String> {
    create_mcp_config_with_runtime_context_for_profile(
        plugin_dir,
        agent_type,
        None,
        is_external_mcp,
        mcp_runtime_context,
    )
}

pub fn create_mcp_config_with_runtime_context_for_profile(
    plugin_dir: &Path,
    agent_type: &str,
    agent_profile: Option<&str>,
    is_external_mcp: bool,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<PathBuf, String> {
    let mcp_config = build_mcp_config_with_runtime_context_for_profile(
        plugin_dir,
        agent_type,
        agent_profile,
        is_external_mcp,
        mcp_runtime_context,
    )?;
    write_mcp_config_temp(&mcp_config)
}

#[cfg(test)]
pub(crate) fn build_mcp_config_with_runtime_context(
    plugin_dir: &Path,
    agent_type: &str,
    is_external_mcp: bool,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<serde_json::Value, String> {
    build_mcp_config_with_runtime_context_for_profile(
        plugin_dir,
        agent_type,
        None,
        is_external_mcp,
        mcp_runtime_context,
    )
}

pub(crate) fn build_mcp_config_with_runtime_context_for_profile(
    plugin_dir: &Path,
    agent_type: &str,
    agent_profile: Option<&str>,
    is_external_mcp: bool,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<serde_json::Value, String> {
    let mcp_server_path = plugin_dir.join("ralphx-mcp-server/build/index.js");
    let mcp_server_path_str = mcp_server_path.to_string_lossy().to_string();
    // Resolve node path robustly — delegates to node_utils::find_node_binary() so
    // both stdio MCP registration and the external MCP supervisor use identical logic.
    let node_command = node_utils::find_node_binary()
        .to_string_lossy()
        .into_owned();

    // Strip plugin prefix for MCP server's --agent-type param
    let short_name = mcp_agent_type(agent_type);
    let mcp_server_name = &claude_runtime_config().mcp_server_name;
    let project_root = resolve_project_root_from_plugin_dir(plugin_dir);
    let claude_metadata =
        try_load_canonical_claude_metadata_for_profile(&project_root, short_name, agent_profile)?;
    let mut mcp_servers = serde_json::Map::new();
    let mut server_names = Vec::new();

    if claude_metadata.mcp_transport.as_deref() == Some("external") {
        mcp_servers.insert(
            mcp_server_name.to_string(),
            build_external_mcp_server_config(mcp_runtime_context),
        );
        server_names.push(mcp_server_name.to_string());

        if !claude_metadata.internal_mcp_tools.is_empty() {
            let internal_server_name = internal_mcp_server_name(mcp_server_name);
            let internal_server_cfg = build_internal_mcp_server_config(
                &mcp_server_path_str,
                &node_command,
                short_name,
                agent_type,
                agent_profile,
                is_external_mcp,
                mcp_runtime_context,
                Some(&claude_metadata.internal_mcp_tools),
            );
            mcp_servers.insert(internal_server_name.clone(), internal_server_cfg);
            server_names.push(internal_server_name);
        }
    } else {
        mcp_servers.insert(
            mcp_server_name.to_string(),
            build_internal_mcp_server_config(
                &mcp_server_path_str,
                &node_command,
                short_name,
                agent_type,
                agent_profile,
                is_external_mcp,
                mcp_runtime_context,
                None,
            ),
        );
        server_names.push(mcp_server_name.to_string());
    }

    let mcp_config = serde_json::json!({
        "mcpServers": serde_json::Value::Object(mcp_servers)
    });

    for server_name in server_names {
        validate_mcp_config_json(&mcp_config, &server_name)
            .map_err(|e| format!("Critical: MCP server config invalid — {e}"))?;
    }

    Ok(mcp_config)
}

fn build_internal_mcp_server_config(
    mcp_server_path_str: &str,
    node_command: &str,
    short_name: &str,
    agent_type: &str,
    agent_profile: Option<&str>,
    is_external_mcp: bool,
    mcp_runtime_context: Option<&McpRuntimeContext>,
    explicit_allowed_tools: Option<&[String]>,
) -> serde_json::Value {
    let mut args_vec = vec![mcp_server_path_str.to_string()];

    // Always pass --agent-type for MCP-side tool filtering.
    args_vec.push("--agent-type".to_string());
    args_vec.push(short_name.to_string());
    if let Some(agent_profile) = agent_profile {
        args_vec.push("--agent-profile".to_string());
        args_vec.push(agent_profile.to_string());
    }
    args_vec.push("--tauri-api-url".to_string());
    args_vec.push(crate::utils::backend_endpoint::backend_http_base_url());
    args_vec.push("--trace-dir".to_string());
    args_vec.push(
        crate::utils::runtime_log_paths::ensure_mcp_proxy_trace_dir()
            .to_string_lossy()
            .into_owned(),
    );

    // Inject --allowed-tools from agent metadata.
    // - Agent not in config and no explicit sidecar list → skip arg entirely.
    // - Empty list → inject __NONE__ sentinel (intentional zero tools).
    // - Non-empty list → validate names, join with commas, inject arg.
    // When is_external_mcp=true, strip interactive-only tools to prevent unattended deadlocks.
    let validated_tools: Option<Vec<String>> = if let Some(tools) = explicit_allowed_tools {
        Some(validated_mcp_tools(agent_type, tools, is_external_mcp))
    } else {
        match get_agent_config_for_profile(agent_type, agent_profile) {
            Some(cfg) => Some(validated_mcp_tools(
                agent_type,
                &cfg.allowed_mcp_tools,
                is_external_mcp,
            )),
            None if agent_profile.is_some() => Some(Vec::new()),
            None => None,
        }
    };
    if let Some(arg_value) = format_allowed_tools_arg_value(validated_tools.as_deref()) {
        args_vec.push(format!("--allowed-tools={}", arg_value));
    }
    append_mcp_runtime_args(&mut args_vec, mcp_runtime_context);

    serde_json::json!({
        "type": "stdio",
        "command": node_command,
        "args": args_vec,
    })
}

fn validated_mcp_tools(agent_type: &str, tools: &[String], is_external_mcp: bool) -> Vec<String> {
    let valid_tools: Vec<String> = tools
        .iter()
        .filter(|name| {
            if validate_mcp_tool_name(name) {
                true
            } else {
                tracing::error!(
                    "[RalphX] Invalid MCP tool name {:?} for agent {:?} (skipped from --allowed-tools)",
                    name,
                    agent_type
                );
                false
            }
        })
        .cloned()
        .collect();
    if is_external_mcp {
        filter_interactive_tools(&valid_tools)
    } else {
        valid_tools
    }
}

fn build_external_mcp_server_config(
    runtime_context: Option<&McpRuntimeContext>,
) -> serde_json::Value {
    let cfg = external_mcp_config();
    let token = ensure_tauri_mcp_bypass_token();
    let mut url = format!("http://{}:{}/mcp", cfg.host, cfg.port);
    append_mcp_runtime_query(&mut url, runtime_context);
    serde_json::json!({
        "type": "http",
        "url": url,
        "headers": {
            "Authorization": format!("Bearer {token}")
        }
    })
}

fn write_mcp_config_temp(mcp_config: &serde_json::Value) -> Result<PathBuf, String> {
    let config_json = serde_json::to_string(mcp_config)
        .map_err(|e| format!("Failed to serialize MCP config: {e}"))?;
    let mut temp_file = tempfile::Builder::new()
        .prefix("ralphx-mcp-")
        .suffix(".json")
        .tempfile()
        .map_err(|e| format!("Failed to create MCP config temp file: {e}"))?;
    {
        use std::io::Write as _;
        temp_file
            .as_file_mut()
            .write_all(config_json.as_bytes())
            .map_err(|e| format!("Failed to write MCP config temp file: {e}"))?;
    }
    let (_file, path) = temp_file
        .keep()
        .map_err(|e| format!("Failed to keep MCP config temp file: {e}"))?;
    Ok(path)
}

fn write_agent_system_prompt_temp(system_prompt: &str) -> Result<PathBuf, String> {
    let mut temp_file = tempfile::Builder::new()
        .prefix("ralphx-agent-prompt-")
        .suffix(".md")
        .tempfile()
        .map_err(|e| format!("Failed to create agent prompt temp file: {e}"))?;
    {
        use std::io::Write as _;
        temp_file
            .as_file_mut()
            .write_all(system_prompt.as_bytes())
            .map_err(|e| format!("Failed to write agent prompt temp file: {e}"))?;
    }
    let (_file, path) = temp_file
        .keep()
        .map_err(|e| format!("Failed to keep agent prompt temp file: {e}"))?;
    Ok(path)
}

fn append_system_prompt_args<F>(
    cmd: &mut Command,
    agent_name: &str,
    system_prompt: &str,
    use_file: bool,
    write_system_prompt_temp: F,
) where
    F: FnOnce(&str) -> Result<PathBuf, String>,
{
    if use_file {
        match write_system_prompt_temp(system_prompt) {
            Ok(prompt_file) => {
                if let Some(path_str) = prompt_file.to_str() {
                    cmd.args(["--append-system-prompt-file", path_str]);
                    tracing::debug!(
                        agent = agent_name,
                        path = path_str,
                        "Injected generated agent prompt via --append-system-prompt-file"
                    );
                } else {
                    cmd.args(["--append-system-prompt", system_prompt]);
                    tracing::debug!(
                        agent = agent_name,
                        "Injected generated agent prompt via --append-system-prompt"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(
                    agent = agent_name,
                    error = %error,
                    "Failed to write generated agent prompt file; falling back to --append-system-prompt"
                );
                cmd.args(["--append-system-prompt", system_prompt]);
            }
        }
    } else {
        cmd.args(["--append-system-prompt", system_prompt]);
        tracing::debug!(
            agent = agent_name,
            "Injected agent prompt via --append-system-prompt"
        );
    }
}

/// A ready-to-spawn CLI command that handles stdin piping automatically.
///
/// **CLI bug workaround (2.1.38):** `--agent` + `-p "text"` causes the CLI to
/// hang silently. Piping via stdin with `-p -` works correctly. `SpawnableCommand`
/// encapsulates this so callers just call `spawn()`.
pub struct SpawnableCommand {
    cmd: Command,
    stdin_prompt: Option<String>,
    stdin_transport: SpawnableStdinTransport,
    prompt_arg_debug_redaction: Option<PromptArgDebugRedaction>,
    injected_skill_names: Vec<String>,
    persona_injected: bool,
    persona_injection_skipped_reason: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpawnableStdinTransport {
    #[cfg(test)]
    Inherited,
    Null,
    Piped,
}

#[derive(Debug, Clone)]
struct PromptArgDebugRedaction {
    arg_index: usize,
    artifact_path: PathBuf,
}

struct DebugCommandView<'a> {
    cmd: &'a Command,
    prompt_arg_debug_redaction: Option<&'a PromptArgDebugRedaction>,
}

impl std::fmt::Debug for DebugCommandView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let std_cmd = self.cmd.as_std();
        let args_count = std_cmd.get_args().count();
        let prompt_arg_index = self
            .prompt_arg_debug_redaction
            .map(|redaction| redaction.arg_index);
        let has_prompt_artifact = self.prompt_arg_debug_redaction.is_some();
        let prompt_artifact = self
            .prompt_arg_debug_redaction
            .map(|redaction| redaction.artifact_path.display().to_string());

        let envs = std_cmd
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|_| (key.to_string_lossy().into_owned(), "<redacted>".to_string()))
            })
            .collect::<Vec<_>>();

        f.debug_struct("Command")
            .field(
                "program",
                &std_cmd.get_program().to_string_lossy().into_owned(),
            )
            .field(
                "current_dir",
                &std_cmd
                    .get_current_dir()
                    .map(|path| path.to_string_lossy().into_owned()),
            )
            .field("args_count", &args_count)
            .field("prompt_arg_index", &prompt_arg_index)
            .field("has_prompt_artifact", &has_prompt_artifact)
            .field("prompt_artifact", &prompt_artifact)
            .field("envs", &envs)
            .finish()
    }
}

impl std::fmt::Debug for SpawnableCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prompt_len = self.stdin_prompt.as_ref().map(|s| s.len());
        f.debug_struct("SpawnableCommand")
            .field(
                "cmd",
                &DebugCommandView {
                    cmd: &self.cmd,
                    prompt_arg_debug_redaction: self.prompt_arg_debug_redaction.as_ref(),
                },
            )
            .field("stdin_transport", &self.stdin_transport)
            .field("has_stdin_prompt", &self.stdin_prompt.is_some())
            .field("stdin_prompt_len", &prompt_len)
            .field("stdin_prompt_redacted", &self.stdin_prompt.is_some())
            .finish()
    }
}

impl SpawnableCommand {
    #[cfg(test)]
    pub(crate) fn new(cmd: Command, stdin_prompt: Option<String>) -> Self {
        Self::new_with_stdin_transport(cmd, stdin_prompt, SpawnableStdinTransport::Inherited)
    }

    pub(crate) fn new_with_stdin_transport(
        mut cmd: Command,
        stdin_prompt: Option<String>,
        stdin_transport: SpawnableStdinTransport,
    ) -> Self {
        crate::infrastructure::subprocess_env_policy::github_cli_env_policy()
            .apply_to_tokio_command(&mut cmd);
        Self {
            cmd,
            stdin_prompt,
            stdin_transport,
            prompt_arg_debug_redaction: None,
            injected_skill_names: Vec::new(),
            persona_injected: false,
            persona_injection_skipped_reason: None,
        }
    }

    pub(crate) fn with_persona_injection_outcome(
        mut self,
        persona_injected: bool,
        persona_injection_skipped_reason: Option<&'static str>,
    ) -> Self {
        self.persona_injected = persona_injected;
        self.persona_injection_skipped_reason = persona_injection_skipped_reason;
        self
    }

    pub(crate) fn with_injected_skill_names(mut self, injected_skill_names: Vec<String>) -> Self {
        self.injected_skill_names = injected_skill_names;
        self
    }

    pub(crate) fn injected_skill_names(&self) -> &[String] {
        &self.injected_skill_names
    }

    pub(crate) fn persona_injected(&self) -> bool {
        self.persona_injected
    }

    pub(crate) fn persona_injection_skipped_reason(&self) -> Option<&'static str> {
        self.persona_injection_skipped_reason
    }

    pub(crate) fn with_prompt_arg_debug_redaction(
        mut self,
        arg_index: usize,
        artifact_path: PathBuf,
    ) -> Self {
        self.prompt_arg_debug_redaction = Some(PromptArgDebugRedaction {
            arg_index,
            artifact_path,
        });
        self
    }

    /// Set an environment variable on the underlying command.
    pub fn env(&mut self, key: &str, val: &str) -> &mut Self {
        if crate::infrastructure::subprocess_env_policy::is_github_cli_token_env_var(key) {
            self.cmd.env_remove(key);
        } else {
            self.cmd.env(key, val);
        }
        self
    }

    /// Append a CLI argument to the underlying command.
    pub fn arg(&mut self, val: &str) -> &mut Self {
        self.cmd.arg(val);
        self
    }

    /// Returns environment variables explicitly set on this command.
    ///
    /// For use in tests only — verifies env-var injection without spawning the process.
    #[doc(hidden)]
    pub fn get_envs_for_test(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        self.cmd
            .as_std()
            .get_envs()
            .filter_map(|(k, v)| v.map(|val| (k.to_os_string(), val.to_os_string())))
            .collect()
    }

    /// Returns CLI arguments currently configured on this command.
    #[doc(hidden)]
    pub fn get_args_for_test(&self) -> Vec<String> {
        self.cmd
            .as_std()
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    /// Returns the stored stdin prompt for test assertions.
    #[doc(hidden)]
    pub fn get_stdin_prompt_for_test(&self) -> Option<&str> {
        self.stdin_prompt.as_deref()
    }

    /// Spawn in interactive mode: writes the stored prompt to stdin, then returns
    /// the stdin handle open for future multi-turn messages.
    ///
    /// Unlike `spawn()` (which drops stdin after writing, signaling EOF), this
    /// keeps stdin alive so the caller can write additional messages later.
    ///
    /// The command uses `-p - --input-format stream-json`, so each message
    /// (including the initial prompt) is a single-line JSON object. The CLI
    /// stays in print mode (required for `--output-format stream-json`) while
    /// reading new turns from stdin until EOF.
    pub async fn spawn_interactive(
        mut self,
    ) -> std::io::Result<(tokio::process::Child, tokio::process::ChildStdin)> {
        self.cmd.kill_on_drop(true);
        let mut child = self.cmd.spawn()?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            std::io::Error::other(
                "no stdin pipe — ensure Stdio::piped() was set before spawn_interactive",
            )
        })?;

        // Write the stored initial prompt (if any). No deadlock risk in interactive mode:
        // the process waits for stdin input before producing stdout, so the pipe
        // buffer cannot fill up from the other direction during this write.
        if let Some(prompt) = self.stdin_prompt.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.write_all(b"\n").await?; // CLI reads lines — newline signals end of input
            stdin.flush().await?; // Ensure bytes are delivered to the process

            // stdin is intentionally NOT dropped — kept open for future messages
        }

        Ok((child, stdin))
    }

    /// Spawn the command and pipe the prompt to stdin if needed.
    ///
    /// Stdin is written in a background task to avoid a pipe deadlock:
    /// the CLI writes to stdout during init (hooks, init event), and if we
    /// block here waiting for stdin write_all to complete, neither side
    /// makes progress once the pipe buffers fill up.
    pub async fn spawn(mut self) -> std::io::Result<tokio::process::Child> {
        self.cmd.kill_on_drop(true);
        let mut child = self.cmd.spawn()?;

        // Write prompt to stdin in background (avoids deadlock with stdout pipe)
        if let Some(prompt) = self.stdin_prompt.take() {
            if let Some(stdin) = child.stdin.take() {
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let mut stdin = stdin;
                    if let Err(e) = stdin.write_all(prompt.as_bytes()).await {
                        tracing::warn!("Failed to write prompt to stdin: {}", e);
                    }
                    // Drop closes stdin, signaling EOF to the CLI
                });
            }
        } else {
            // If stdin is piped but no prompt is provided via stdin, close it
            // immediately so the CLI doesn't wait for additional input.
            let _ = child.stdin.take();
        }

        Ok(child)
    }
}

/// Format a message as a stream-json input line for `--input-format stream-json`.
///
/// The Claude CLI with `-p - --input-format stream-json` reads one JSON message per
/// stdin line. Each message triggers a new turn in the same session.
pub fn format_stream_json_input(content: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": content
        }
    })
    .to_string()
}

/// Add prompt-related args to a CLI command.
///
/// Applies agent-specific tool restrictions via --tools flag (CLI tools)
/// and --allowedTools flag (MCP + CLI tool pre-approvals).
/// See `agent_config/` for the single source of truth on tool configurations.
///
/// When `interactive` is `true`, `-p -` + `--input-format stream-json` are added so the
/// CLI stays in print mode (required for `--output-format stream-json`) while reading
/// structured JSON messages from stdin for multi-turn conversations.
/// The returned outcome carries both stdin delivery and whether the persona block was
/// actually appended. Fallback agent-prompt paths deliberately report no injection.
struct PromptArgsOutcome {
    stdin_prompt: Option<String>,
    injected_skill_names: Vec<String>,
    persona_injected: bool,
    persona_injection_skipped_reason: Option<&'static str>,
}

fn add_prompt_args(
    cmd: &mut Command,
    plugin_dir: &Path,
    prompt: &str,
    persona_block: Option<&str>,
    agent: Option<&str>,
    agent_profile: Option<&str>,
    resume_session: Option<&str>,
    interactive: bool,
    _mcp_runtime_context: Option<&McpRuntimeContext>,
    pre_execution_learned_skills: Option<&PreExecutionLearnedSkillContext>,
    permission_policy: ClaudePermissionPolicy,
) -> PromptArgsOutcome {
    // Add resume if continuing an existing session
    if let Some(session_id) = resume_session {
        cmd.args(["--resume", session_id]);
    }

    // Default path: avoid Claude's `--agent` execution mode (currently unstable in
    // some worktree/headless scenarios) and inject the agent behavior via
    // `--append-system-prompt` loaded from our codebase agent markdown.
    // Set RALPHX_USE_NATIVE_AGENT_FLAG=1 to force native --agent mode.
    //
    let use_native_agent_flag = native_agent_flag_enabled();

    // Default to stdin mode for agent runs due to CLI instability with
    // `--agent` + `-p "<text>"` on some Claude Code versions.
    // Set RALPHX_CLAUDE_PROMPT_MODE=arg to force direct -p arg mode.
    let use_stdin = if agent.is_some() {
        !matches!(std::env::var("RALPHX_CLAUDE_PROMPT_MODE"), Ok(mode) if mode.eq_ignore_ascii_case("arg"))
    } else {
        false
    };
    let mut persona_injected = false;
    let mut persona_skip_reason = None;
    let mut injected_skill_names = Vec::new();
    if let Some(agent_name) = agent {
        if use_native_agent_flag {
            cmd.args(["--agent", agent_name]);
            persona_skip_reason =
                persona_injection_skipped_reason(use_native_agent_flag, persona_block.is_some());
        } else if let Some(prompt_path) = resolve_agent_system_prompt_path(plugin_dir, agent_name) {
            let runtime = claude_runtime_config();
            let prompt_with_internal_skills = load_agent_system_prompt_with_internal_skills(
                plugin_dir,
                agent_name,
                agent_profile,
                prompt,
                persona_block,
                pre_execution_learned_skills,
            );
            if let Some((system_prompt, composed_skill_names)) =
                prompt_with_internal_skills.as_ref()
            {
                if !composed_skill_names.is_empty() {
                    tracing::debug!(
                        agent = agent_name,
                        skills = ?composed_skill_names,
                        "Injected agent prompt with internal skills"
                    );
                }
                append_system_prompt_args(
                    cmd,
                    agent_name,
                    system_prompt,
                    runtime.use_append_system_prompt_file,
                    write_agent_system_prompt_temp,
                );
                persona_injected = persona_block.is_some();
                composed_skill_names.clone_into(&mut injected_skill_names);
            } else if runtime.use_append_system_prompt_file {
                if let Some(path_str) = prompt_path.to_str() {
                    cmd.args(["--append-system-prompt-file", path_str]);
                    tracing::debug!(
                        agent = agent_name,
                        path = path_str,
                        "Injected agent prompt via --append-system-prompt-file"
                    );
                    persona_skip_reason = persona_block
                        .is_some()
                        .then_some("prompt_composition_fallback_raw_prompt_file");
                } else {
                    tracing::warn!(
                        agent = agent_name,
                        "Agent prompt path was not valid UTF-8; falling back to native --agent"
                    );
                    cmd.args(["--agent", agent_name]);
                    persona_skip_reason = persona_block
                        .is_some()
                        .then_some("prompt_path_non_utf8_native_agent");
                }
            } else {
                tracing::warn!(
                    agent = agent_name,
                    "Failed to load prompt content; falling back to native --agent"
                );
                cmd.args(["--agent", agent_name]);
                persona_skip_reason = persona_block
                    .is_some()
                    .then_some("prompt_composition_fallback_native_agent");
            }
        } else {
            tracing::warn!(
                agent = agent_name,
                "Agent prompt not found in plugin; falling back to native --agent"
            );
            cmd.args(["--agent", agent_name]);
            persona_skip_reason = persona_block
                .is_some()
                .then_some("agent_prompt_not_found_native_agent");
        }

        // Apply CLI tool restrictions from agent_config
        // Frontmatter tools/disallowedTools only work for subagent spawning,
        // NOT for direct CLI invocations with --agent -p. Pass --tools only when
        // there are built-in CLI tools to allow; the Claude CLI treats an empty
        // value as disabling MCP tools too.
        if let Some(allowed_tools) = get_allowed_tools_for_profile(agent_name, agent_profile) {
            if allowed_tools.is_empty() {
                tracing::debug!(
                    agent = agent_name,
                    "Agent configured as MCP-only; omitting --tools because Claude CLI treats an empty value as disabling MCP tools"
                );
            } else {
                cmd.args(["--tools", &allowed_tools]);
                tracing::debug!(
                    agent = agent_name,
                    tools = allowed_tools.as_str(),
                    "Agent restricted to CLI tools"
                );
            }
        }

        // Pre-approve tools to bypass permission prompts (MCP + CLI permissions)
        if let Some(preapproved) =
            preapproved_tools_for_permission_policy(agent_name, agent_profile, permission_policy)
        {
            cmd.args(["--allowedTools", &preapproved]);
            tracing::debug!(agent = agent_name, preapproved = %preapproved, "Agent pre-approved tools");
        }
    }

    let stdin_prompt = if interactive {
        // --output-format stream-json only works with -p (print mode).
        // Use `-p -` to stay in print mode + `--input-format stream-json` so the CLI
        // reads structured JSON messages from stdin (one per line) for multi-turn.
        // The process stays alive until stdin EOF.
        cmd.args(["-p", "-", "--input-format", "stream-json"]);
        tracing::debug!("Claude prompt mode: interactive (-p - + stream-json input)");
        Some(format_stream_json_input(prompt))
    } else if use_stdin {
        // Workaround: pipe prompt via stdin to avoid --agent + -p arg hang (CLI 2.1.38)
        cmd.args(["-p", "-"]);
        tracing::debug!("Claude prompt mode: stdin");
        Some(prompt.to_string())
    } else {
        cmd.args(["-p", prompt]);
        tracing::debug!("Claude prompt mode: arg");
        None
    };

    PromptArgsOutcome {
        stdin_prompt,
        injected_skill_names,
        persona_injected,
        persona_injection_skipped_reason: persona_skip_reason,
    }
}

pub(crate) fn native_agent_flag_enabled() -> bool {
    std::env::var("RALPHX_USE_NATIVE_AGENT_FLAG")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub(crate) fn persona_injection_skipped_reason(
    use_native_agent_flag: bool,
    resolved: bool,
) -> Option<&'static str> {
    (use_native_agent_flag && resolved).then_some("native_agent_flag")
}

/// Configure command for spawning (working dir, stdout/stderr capture)
fn configure_spawn(cmd: &mut Command, working_dir: &Path, needs_stdin: bool) {
    // codeql[rust/path-injection]
    cmd.current_dir(working_dir);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    // Always provide a pipe for stdin.
    // In GUI/non-TTY environments, inheriting stdin can present as closed and
    // Claude may exit early before emitting stream-json output.
    let _ = needs_stdin;
    cmd.stdin(std::process::Stdio::piped());
}

/// Build a ready-to-spawn CLI command with all args configured.
///
/// Combines `build_base_cli_command`, `add_prompt_args`, and `configure_spawn`
/// into a single `SpawnableCommand` that handles stdin piping automatically.
pub fn build_spawnable_command(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    effort_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_command_with_mcp_runtime_context(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        resume_session,
        working_directory,
        effort_override,
        model_override,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_spawnable_command_with_mcp_runtime_context(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<SpawnableCommand, String> {
    let mut cmd = build_base_cli_command_inner_with_runtime_context(
        cli_path,
        plugin_dir,
        agent,
        false,
        effort_override,
        model_override,
        mcp_runtime_context,
        true,
    )?;
    let prompt_args = add_prompt_args(
        &mut cmd,
        plugin_dir,
        prompt,
        None,
        agent,
        None,
        resume_session,
        false,
        mcp_runtime_context,
        None,
        ClaudePermissionPolicy::InheritConfigured,
    );
    configure_spawn(
        &mut cmd,
        working_directory,
        prompt_args.stdin_prompt.is_some(),
    );
    Ok(SpawnableCommand::new_with_stdin_transport(
        cmd,
        prompt_args.stdin_prompt,
        SpawnableStdinTransport::Piped,
    )
    .with_persona_injection_outcome(
        prompt_args.persona_injected,
        prompt_args.persona_injection_skipped_reason,
    )
    .with_injected_skill_names(prompt_args.injected_skill_names))
}

#[allow(clippy::too_many_arguments)]
pub fn build_spawnable_command_with_mcp_runtime_context_and_profile(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_profile_command_with_permission_policy(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        agent_profile,
        persona_block,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
        None,
        ClaudePermissionPolicy::InheritConfigured,
        ClaudePromptDelivery::NonInteractive,
    )
}

#[allow(clippy::too_many_arguments)]
/// Canonical profile-aware Claude command seam for backend-selected launch security.
pub(crate) fn build_spawnable_profile_command_with_permission_policy(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
    pre_execution_learned_skills: Option<&PreExecutionLearnedSkillContext>,
    permission_policy: ClaudePermissionPolicy,
    prompt_delivery: ClaudePromptDelivery,
) -> Result<SpawnableCommand, String> {
    build_spawnable_profile_command_with_permission_policy_inner(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        agent_profile,
        persona_block,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
        pre_execution_learned_skills,
        permission_policy,
        prompt_delivery,
        true,
    )
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_spawnable_profile_command_with_permission_policy_for_test(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
    pre_execution_learned_skills: Option<&PreExecutionLearnedSkillContext>,
    permission_policy: ClaudePermissionPolicy,
    prompt_delivery: ClaudePromptDelivery,
) -> Result<SpawnableCommand, String> {
    build_spawnable_profile_command_with_permission_policy_inner(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        agent_profile,
        persona_block,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
        pre_execution_learned_skills,
        permission_policy,
        prompt_delivery,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_spawnable_profile_command_with_permission_policy_inner(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
    pre_execution_learned_skills: Option<&PreExecutionLearnedSkillContext>,
    permission_policy: ClaudePermissionPolicy,
    prompt_delivery: ClaudePromptDelivery,
    enforce_spawn_guard: bool,
) -> Result<SpawnableCommand, String> {
    let mut cmd = build_base_cli_command_inner_with_runtime_context_and_profile(
        cli_path,
        plugin_dir,
        agent,
        agent_profile,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
        enforce_spawn_guard,
        permission_policy,
    )?;
    let prompt_args = add_prompt_args(
        &mut cmd,
        plugin_dir,
        prompt,
        persona_block,
        agent,
        agent_profile,
        resume_session,
        prompt_delivery.is_interactive(),
        mcp_runtime_context,
        pre_execution_learned_skills,
        permission_policy,
    );
    configure_spawn(
        &mut cmd,
        working_directory,
        prompt_delivery.is_interactive() || prompt_args.stdin_prompt.is_some(),
    );
    Ok(SpawnableCommand::new_with_stdin_transport(
        cmd,
        prompt_args.stdin_prompt,
        SpawnableStdinTransport::Piped,
    )
    .with_persona_injection_outcome(
        prompt_args.persona_injected,
        prompt_args.persona_injection_skipped_reason,
    )
    .with_injected_skill_names(prompt_args.injected_skill_names))
}

#[cfg(any(test, feature = "test-utils"))]
pub fn build_spawnable_command_for_test(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    effort_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_command_with_mcp_runtime_context_for_test(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        resume_session,
        working_directory,
        effort_override,
        model_override,
        None,
    )
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(clippy::too_many_arguments)]
pub fn build_spawnable_command_with_mcp_runtime_context_for_test(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<SpawnableCommand, String> {
    let mut cmd = build_base_cli_command_inner_with_runtime_context(
        cli_path,
        plugin_dir,
        agent,
        false,
        effort_override,
        model_override,
        mcp_runtime_context,
        false,
    )?;
    let prompt_args = add_prompt_args(
        &mut cmd,
        plugin_dir,
        prompt,
        None,
        agent,
        None,
        resume_session,
        false,
        mcp_runtime_context,
        None,
        ClaudePermissionPolicy::InheritConfigured,
    );
    configure_spawn(
        &mut cmd,
        working_directory,
        prompt_args.stdin_prompt.is_some(),
    );
    Ok(SpawnableCommand::new_with_stdin_transport(
        cmd,
        prompt_args.stdin_prompt,
        SpawnableStdinTransport::Piped,
    )
    .with_persona_injection_outcome(
        prompt_args.persona_injected,
        prompt_args.persona_injection_skipped_reason,
    )
    .with_injected_skill_names(prompt_args.injected_skill_names))
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(clippy::too_many_arguments)]
pub fn build_spawnable_command_with_mcp_runtime_context_and_profile_for_test(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_profile_command_with_permission_policy_for_test(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        agent_profile,
        persona_block,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
        None,
        ClaudePermissionPolicy::InheritConfigured,
        ClaudePromptDelivery::NonInteractive,
    )
}

/// Build a ready-to-spawn interactive CLI command (no `-p` flag).
///
/// Like `build_spawnable_command` but omits `-p` so the process enters
/// interactive/REPL mode. The prompt is stored for delivery via stdin
/// when `spawn_interactive()` is called.
///
/// Use `SpawnableCommand::spawn_interactive()` (instead of `spawn()`) to get
/// back the stdin handle for multi-turn message delivery.
pub fn build_spawnable_interactive_command(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_interactive_command_with_mcp_runtime_context(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_spawnable_interactive_command_with_mcp_runtime_context(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_interactive_command_with_mcp_runtime_context_and_profile(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        None,
        None,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_spawnable_interactive_command_with_mcp_runtime_context_and_profile(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_profile_command_with_permission_policy(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        agent_profile,
        persona_block,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
        None,
        ClaudePermissionPolicy::InheritConfigured,
        ClaudePromptDelivery::Interactive,
    )
}

#[cfg(any(test, feature = "test-utils"))]
pub fn build_spawnable_interactive_command_for_test(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_interactive_command_with_mcp_runtime_context_for_test(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        None,
    )
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(clippy::too_many_arguments)]
pub fn build_spawnable_interactive_command_with_mcp_runtime_context_for_test(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_interactive_command_with_mcp_runtime_context_and_profile_for_test(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        None,
        None,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
    )
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(clippy::too_many_arguments)]
pub fn build_spawnable_interactive_command_with_mcp_runtime_context_and_profile_for_test(
    cli_path: &Path,
    plugin_dir: &Path,
    prompt: &str,
    agent: Option<&str>,
    agent_profile: Option<&str>,
    persona_block: Option<&str>,
    resume_session: Option<&str>,
    working_directory: &Path,
    is_external_mcp: bool,
    effort_override: Option<&str>,
    model_override: Option<&str>,
    mcp_runtime_context: Option<&McpRuntimeContext>,
) -> Result<SpawnableCommand, String> {
    build_spawnable_profile_command_with_permission_policy_for_test(
        cli_path,
        plugin_dir,
        prompt,
        agent,
        agent_profile,
        persona_block,
        resume_session,
        working_directory,
        is_external_mcp,
        effort_override,
        model_override,
        mcp_runtime_context,
        None,
        ClaudePermissionPolicy::InheritConfigured,
        ClaudePromptDelivery::Interactive,
    )
}

/// Find the Claude CLI path (uses same approach as ClaudeCodeClient)
pub fn find_claude_cli() -> Option<PathBuf> {
    crate::infrastructure::tool_paths::find_claude_cli_path()
}

/// Find the plugin directory relative to the app
pub fn find_plugin_dir() -> Option<PathBuf> {
    let base_plugin_dir = find_base_plugin_dir()?;
    match generated_plugin::materialize_generated_plugin_dir(&base_plugin_dir) {
        Ok(generated_dir) => Some(generated_dir),
        Err(error) => {
            warn!(
                base_plugin_dir = %base_plugin_dir.display(),
                error = %error,
                "Failed to materialize generated Claude plugin dir; falling back to base plugin dir"
            );
            Some(base_plugin_dir)
        }
    }
}

/// Resolve plugin directory for a specific working directory context.
///
/// Priority:
/// 1) configured bundled/runtime plugin dir
/// 2) RalphX source checkout plugin dir
/// 3) source checkout `plugins/app` fallback
///
/// The active `working_dir` is the target project checkout and must not be used as
/// the RalphX-owned runtime root.
pub fn resolve_base_plugin_dir(_working_dir: &Path) -> PathBuf {
    find_base_plugin_dir().unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|repo_root| repo_root.join(PRIMARY_PLUGIN_DIR_REL))
            .unwrap_or_else(|| PathBuf::from(PRIMARY_PLUGIN_DIR_REL))
    })
}

pub fn resolve_plugin_dir(working_dir: &Path) -> PathBuf {
    let base_plugin_dir = resolve_base_plugin_dir(working_dir);
    if !base_plugin_dir.exists() {
        return base_plugin_dir;
    }

    match generated_plugin::materialize_generated_plugin_dir(&base_plugin_dir) {
        Ok(generated_dir) => generated_dir,
        Err(error) => {
            warn!(
                base_plugin_dir = %base_plugin_dir.display(),
                error = %error,
                "Failed to materialize generated Claude plugin dir for working directory; falling back to base plugin dir"
            );
            base_plugin_dir
        }
    }
}

// ============================================================================
// Wave 3 stubs — allow mod_tests.rs to compile in TDD red state.
// Tests call these functions and fail at runtime (todo!) until Wave 3 implements them.
// ============================================================================

/// Validate that an MCP tool name matches `^[a-z][a-z0-9_]*$`.
/// Returns `false` for empty strings, names starting with a digit, names with uppercase,
/// or names containing special characters (commas, spaces, hyphens, dots, etc.).
pub fn validate_mcp_tool_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Format the `--allowed-tools` arg value from an optional tool list.
/// - `None` → `None` (agent has no mcp_tools config → no arg injected)
/// - `Some([])` → `Some("__NONE__")` sentinel (explicit empty, no server-side fallback)
/// - `Some([t1, t2, ...])` → `Some("t1,t2,...")`
pub fn format_allowed_tools_arg_value(tools: Option<&[String]>) -> Option<String> {
    match tools {
        None => None,
        Some([]) => Some("__NONE__".to_string()),
        Some(tools) => Some(tools.join(",")),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[path = "mod_tests.rs"]
mod create_mcp_config_tests;

#[cfg(test)]
#[path = "spawnable_command_tests.rs"]
mod spawnable_command_tests;

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
