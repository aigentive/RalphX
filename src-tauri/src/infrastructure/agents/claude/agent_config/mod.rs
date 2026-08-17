pub mod live_flags;
pub mod process_config;
pub mod runtime_config;
mod tool_sets;
mod ui_config;
pub use live_flags::{agent_personas_enabled, standalone_conversations_enabled};
pub use ui_config::{UiConfig, UiFeatureFlagsConfig};

use crate::domain::agents::{
    standard_agent_lane_defaults, AgentHarnessKind, AgentLane, AgentLaneSettings, LogicalEffort,
    CLAUDE_DEFAULT_ALLOW_DANGEROUSLY_SKIP_PERMISSIONS, CLAUDE_DEFAULT_DANGEROUSLY_SKIP_PERMISSIONS,
    CLAUDE_DEFAULT_PERMISSION_MODE,
};
use crate::domain::execution::{ExecutionSettings, GlobalExecutionSettings};
use crate::infrastructure::agents::harness_agent_catalog::{
    internal_mcp_server_name, list_canonical_prompt_backed_agents, load_canonical_agent_definition,
    load_canonical_agent_definition_for_profile, resolve_harness_agent_prompt_path,
    resolve_project_root_from_catalog_path, resolve_project_root_from_plugin_dir,
    try_load_canonical_claude_metadata, try_load_canonical_claude_metadata_for_profile,
    AgentPromptHarness, CanonicalClaudeToolSpec,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tool_sets::canonical_claude_tool_sets;

use process_config::{resolve_canonical_process_mapping, ProcessMapping};

pub use runtime_config::{
    bounded_external_mcp_shutdown_grace_ms, bounded_shutdown_watchdog_deadline_secs,
    validate_external_mcp_config, AllRuntimeConfig, AutomationsRuntimeConfig,
    DatabaseMaintenanceConfig, DelegationConfig, ExternalMcpConfig, GitRuntimeConfig, LimitsConfig,
    ReconciliationConfig, SchedulerConfig, ShutdownConfig, SpecialistEntry, StreamTimeoutsConfig,
    SupervisorRuntimeConfig, VerificationConfig, WorkspaceReviewRuntimeConfig,
    MAX_EXTERNAL_MCP_SHUTDOWN_GRACE_MS,
};

const VALID_EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

fn validate_effort(value: &str, agent_name: &str) -> bool {
    if VALID_EFFORT_LEVELS.contains(&value) {
        true
    } else {
        tracing::warn!(agent = %agent_name, effort = %value, "Invalid effort level; ignoring");
        false
    }
}

const MEMORY_SKILLS: &[&str] = &[
    "Skill(ralphx:rule-manager)",
    "Skill(ralphx:knowledge-capture)",
];

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub mcp_only: bool,
    pub resolved_cli_tools: Vec<String>,
    pub allowed_mcp_tools: Vec<String>,
    pub preapproved_cli_tools: Vec<String>,
    pub system_prompt_file: String,
    pub model: Option<String>,
    /// Effective claude settings profile selection for this agent (if any).
    pub settings_profile: Option<String>,
    /// Effective settings JSON for this agent (if any), resolved from settings_profile.
    pub settings: Option<serde_json::Value>,
    /// Optional per-agent effort level override (e.g. "max"). Validated at parse time.
    pub effort: Option<String>,
    /// Optional per-agent permission mode override (e.g. "acceptEdits"). None means inherit global.
    pub permission_mode: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaudeRuntimeConfig {
    pub mcp_server_name: String,
    pub permission_mode: String,
    pub dangerously_skip_permissions: bool,
    pub allow_dangerously_skip_permissions: bool,
    pub permission_prompt_tool: String,
    pub use_append_system_prompt_file: bool,
    pub setting_sources: Option<Vec<String>>,
    /// JSON object passed to claude CLI via --settings (path or JSON string).
    pub settings: Option<serde_json::Value>,
    /// Global default effort level for all agents (e.g. "medium"). Validated at parse time.
    pub default_effort: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ExecutionDefaultsConfig {
    #[serde(default)]
    pub project: ExecutionSettings,
    #[serde(default)]
    pub global: GlobalExecutionSettings,
}

impl Default for ExecutionDefaultsConfig {
    fn default() -> Self {
        Self {
            project: ExecutionSettings::default(),
            global: GlobalExecutionSettings::default(),
        }
    }
}

pub type AgentHarnessDefaultsConfig = HashMap<AgentLane, AgentLaneSettings>;
type AgentHarnessDefaultsConfigRaw = HashMap<AgentLane, AgentLaneSettingsConfigRaw>;

#[derive(Debug, Clone, Deserialize)]
struct AgentLaneSettingsConfigRaw {
    harness: AgentHarnessKind,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<LogicalEffort>,
    #[serde(default)]
    approval_policy: Option<String>,
    #[serde(default)]
    sandbox_mode: Option<String>,
}

impl From<AgentLaneSettingsConfigRaw> for AgentLaneSettings {
    fn from(value: AgentLaneSettingsConfigRaw) -> Self {
        Self {
            harness: value.harness,
            model: value.model,
            effort: value.effort,
            approval_policy: value.approval_policy,
            sandbox_mode: value.sandbox_mode,
        }
    }
}

impl From<AgentLaneSettings> for AgentLaneSettingsConfigRaw {
    fn from(value: AgentLaneSettings) -> Self {
        Self {
            harness: value.harness,
            model: value.model,
            effort: value.effort,
            approval_policy: value.approval_policy,
            sandbox_mode: value.sandbox_mode,
        }
    }
}

fn default_agent_harness_defaults() -> AgentHarnessDefaultsConfig {
    standard_agent_lane_defaults()
}

fn default_agent_harness_defaults_raw() -> AgentHarnessDefaultsConfigRaw {
    default_agent_harness_defaults()
        .into_iter()
        .map(|(lane, settings)| (lane, settings.into()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
struct AgentToolsSpec {
    #[serde(default)]
    mcp_only: bool,
    extends: Option<String>,
    #[serde(default)]
    include: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentConfigRaw {
    name: String,
    /// Parent agent name for config inheritance. Child fields override parent.
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    tools: AgentToolsSpec,
    #[serde(default)]
    mcp_tools: Vec<String>,
    #[serde(default)]
    preapproved_cli_tools: Vec<String>,
    #[serde(default)]
    system_prompt_file: Option<String>,
    model: Option<String>,
    settings_profile: Option<String>,
    effort: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct ClaudeRuntimeConfigRaw {
    mcp_server_name: String,
    setting_sources: Option<Vec<String>>,
    permission_mode: String,
    dangerously_skip_permissions: bool,
    allow_dangerously_skip_permissions: bool,
    permission_prompt_tool: String,
    append_system_prompt_file: bool,
    /// Optional profile selector for claude settings (`settings_profiles.<name>`).
    settings_profile: Option<String>,
    /// Optional settings merged into every selected profile.
    settings_profile_defaults: Option<serde_json::Value>,
    /// Named claude settings profiles passed via --settings.
    #[serde(default)]
    settings_profiles: HashMap<String, serde_json::Value>,
    /// Optional settings passed to claude CLI via --settings (see docs/ai-docs/claude-code/settings.md).
    /// Legacy field kept for backwards compatibility when profiles are not configured.
    settings: Option<serde_json::Value>,
    #[serde(default)]
    default_effort: Option<String>,
}

impl Default for ClaudeRuntimeConfigRaw {
    fn default() -> Self {
        Self {
            mcp_server_name: "ralphx".to_string(),
            setting_sources: None,
            permission_mode: CLAUDE_DEFAULT_PERMISSION_MODE.to_string(),
            dangerously_skip_permissions: CLAUDE_DEFAULT_DANGEROUSLY_SKIP_PERMISSIONS,
            allow_dangerously_skip_permissions: CLAUDE_DEFAULT_ALLOW_DANGEROUSLY_SKIP_PERMISSIONS,
            permission_prompt_tool: "permission_request".to_string(),
            append_system_prompt_file: true,
            settings_profile: None,
            settings_profile_defaults: None,
            settings_profiles: HashMap::new(),
            settings: None,
            default_effort: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RalphxConfig {
    #[serde(default)]
    tool_sets: HashMap<String, Vec<String>>,
    #[serde(default)]
    claude: ClaudeRuntimeConfigRaw,
    #[serde(default)]
    agents: Vec<AgentConfigRaw>,
    #[serde(default)]
    process_mapping: ProcessMapping,
    /// If true (default), defers merges when conflicts exist or agents are running.
    /// If false, all merges proceed immediately without deferral.
    #[serde(default = "default_defer_merge_enabled")]
    defer_merge_enabled: bool,
    /// Write tracing output to a per-launch log file in addition to console.
    #[serde(default = "default_file_logging")]
    file_logging: bool,
    // ── Runtime config sections ──────────────────────────────────────
    #[serde(default)]
    timeouts: runtime_config::TimeoutsWrapper,
    #[serde(default)]
    database_maintenance: runtime_config::DatabaseMaintenanceConfig,
    #[serde(default)]
    reconciliation: ReconciliationConfig,
    #[serde(default)]
    git: GitRuntimeConfig,
    #[serde(default)]
    scheduler: SchedulerConfig,
    #[serde(default)]
    shutdown: ShutdownConfig,
    #[serde(default)]
    automations: AutomationsRuntimeConfig,
    #[serde(default)]
    supervisor: SupervisorRuntimeConfig,
    #[serde(default)]
    limits: LimitsConfig,
    #[serde(default)]
    ideation: runtime_config::IdeationConfigWrapper,
    #[serde(default)]
    external_mcp: ExternalMcpConfig,
    #[serde(default)]
    delegation: runtime_config::DelegationConfig,
    #[serde(default)]
    workspace_review: runtime_config::WorkspaceReviewRuntimeConfig,
    #[serde(default)]
    ui: Option<UiConfig>,
    #[serde(default)]
    execution_defaults: ExecutionDefaultsConfig,
    #[serde(default = "default_agent_harness_defaults_raw")]
    agent_harness_defaults: AgentHarnessDefaultsConfigRaw,
}

#[derive(Debug, Deserialize, Default)]
struct ClaudeRuntimeConfigOverlay {
    mcp_server_name: Option<String>,
    setting_sources: Option<Vec<String>>,
    permission_mode: Option<String>,
    dangerously_skip_permissions: Option<bool>,
    allow_dangerously_skip_permissions: Option<bool>,
    permission_prompt_tool: Option<String>,
    append_system_prompt_file: Option<bool>,
    settings_profile: Option<String>,
    settings_profile_defaults: Option<serde_json::Value>,
    settings_profiles: Option<HashMap<String, serde_json::Value>>,
    settings: Option<serde_json::Value>,
    default_effort: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ClaudeConfigOverlay {
    #[serde(default)]
    tool_sets: HashMap<String, Vec<String>>,
    claude: Option<ClaudeRuntimeConfigOverlay>,
}

#[derive(Debug, Deserialize, Default)]
struct CodexConfigOverlay {
    #[serde(default)]
    agent_harness_defaults: AgentHarnessDefaultsConfigRaw,
}

#[derive(Debug, Deserialize, Default)]
struct ProcessConfigOverlay {
    #[serde(default)]
    process_mapping: Option<ProcessMapping>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ExternalMcpConfigRawOverlay {
    enabled: Option<bool>,
    port: Option<u16>,
    host: Option<String>,
    max_restart_attempts: Option<u32>,
    restart_delay_ms: Option<u64>,
    shutdown_grace_ms: Option<u64>,
    startup_timeout_secs: Option<u64>,
    human_wait_timeout_secs: Option<u64>,
    auth_token: Option<String>,
    node_path: Option<String>,
    max_external_ideation_sessions: Option<u32>,
    external_session_stale_secs: Option<u64>,
    external_message_queue_cap: Option<u32>,
    external_session_similarity_threshold: Option<f64>,
    external_session_startup_grace_secs: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct ExternalMcpConfigOverlay {
    #[serde(default)]
    external_mcp: Option<ExternalMcpConfigRawOverlay>,
}

const EMBEDDED_CONFIG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../config/ralphx.yaml"
));
const EMBEDDED_EXTERNAL_MCP_CONFIG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../config/external-mcp.yaml"
));

fn default_defer_merge_enabled() -> bool {
    true
}

fn default_file_logging() -> bool {
    true
}

fn default_file_logging_max_bytes() -> u64 {
    1024 * 1024 * 1024
}

fn default_file_logging_keep_files() -> usize {
    5
}

struct LoadedConfig {
    agents: Vec<AgentConfig>,
    claude: ClaudeRuntimeConfig,
    tool_sets: HashMap<String, Vec<String>>,
    process_mapping: ProcessMapping,
    defer_merge_enabled: bool,
    file_logging: bool,
    shutdown: ShutdownConfig,
    runtime: AllRuntimeConfig,
    automations: AutomationsRuntimeConfig,
    execution_defaults: ExecutionDefaultsConfig,
    agent_harness_defaults: AgentHarnessDefaultsConfig,
}

static LOADED_CONFIG_CELL: OnceLock<LoadedConfig> = OnceLock::new();

fn runtime_config_dir_override() -> &'static Mutex<Option<PathBuf>> {
    static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

pub(crate) fn configure_runtime_config_dir(config_dir: PathBuf) {
    if let Ok(mut guard) = runtime_config_dir_override().lock() {
        *guard = Some(config_dir);
    }
}

fn configured_runtime_config_dir() -> Option<PathBuf> {
    runtime_config_dir_override()
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

fn config_path_for_runtime_config_dir(config_dir: Option<&Path>) -> PathBuf {
    if let Some(config_dir) = config_dir {
        return config_dir.join("ralphx.yaml");
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("config")
        .join("ralphx.yaml")
}

fn normalize_mcp_tool_name(raw: &str, server_name: &str) -> String {
    if raw.starts_with("mcp__") {
        raw.to_string()
    } else {
        format!("mcp__{server_name}__{raw}")
    }
}

pub fn config_path() -> PathBuf {
    let configured = configured_runtime_config_dir();
    config_path_for_runtime_config_dir(configured.as_deref())
}

fn config_dir_path() -> PathBuf {
    config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("config")
        })
}

pub fn process_config_path() -> PathBuf {
    config_dir_path().join("processes.yaml")
}

pub fn claude_config_path() -> PathBuf {
    config_dir_path().join("harnesses").join("claude.yaml")
}

pub fn codex_config_path() -> PathBuf {
    config_dir_path().join("harnesses").join("codex.yaml")
}

pub fn external_mcp_config_path() -> PathBuf {
    config_dir_path().join("external-mcp.yaml")
}

fn parse_raw_config(yaml: &str) -> Option<RalphxConfig> {
    match serde_yaml::from_str(yaml) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse RalphX config file");
            None
        }
    }
}

fn apply_claude_runtime_config_overlay(
    cfg: &mut ClaudeRuntimeConfigRaw,
    overlay: ClaudeRuntimeConfigOverlay,
) {
    if let Some(mcp_server_name) = overlay.mcp_server_name {
        cfg.mcp_server_name = mcp_server_name;
    }
    if let Some(setting_sources) = overlay.setting_sources {
        cfg.setting_sources = Some(setting_sources);
    }
    if let Some(permission_mode) = overlay.permission_mode {
        cfg.permission_mode = permission_mode;
    }
    if let Some(dangerously_skip_permissions) = overlay.dangerously_skip_permissions {
        cfg.dangerously_skip_permissions = dangerously_skip_permissions;
    }
    if let Some(allow_dangerously_skip_permissions) = overlay.allow_dangerously_skip_permissions {
        cfg.allow_dangerously_skip_permissions = allow_dangerously_skip_permissions;
    }
    if let Some(permission_prompt_tool) = overlay.permission_prompt_tool {
        cfg.permission_prompt_tool = permission_prompt_tool;
    }
    if let Some(append_system_prompt_file) = overlay.append_system_prompt_file {
        cfg.append_system_prompt_file = append_system_prompt_file;
    }
    if let Some(settings_profile) = overlay.settings_profile {
        cfg.settings_profile = Some(settings_profile);
    }
    if let Some(settings_profile_defaults) = overlay.settings_profile_defaults {
        cfg.settings_profile_defaults = Some(settings_profile_defaults);
    }
    if let Some(settings_profiles) = overlay.settings_profiles {
        cfg.settings_profiles = settings_profiles;
    }
    if let Some(settings) = overlay.settings {
        cfg.settings = Some(settings);
    }
    if let Some(default_effort) = overlay.default_effort {
        cfg.default_effort = Some(default_effort);
    }
}

fn apply_claude_config_overlay(cfg: &mut RalphxConfig, overlay: ClaudeConfigOverlay) {
    cfg.tool_sets.extend(overlay.tool_sets);
    if let Some(claude) = overlay.claude {
        apply_claude_runtime_config_overlay(&mut cfg.claude, claude);
    }
}

fn parse_claude_config_overlay(yaml: &str) -> Option<ClaudeConfigOverlay> {
    serde_yaml::from_str::<ClaudeConfigOverlay>(yaml)
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to parse Claude harness config overlay");
            e
        })
        .ok()
}

fn load_claude_config_overlay() -> Option<(PathBuf, ClaudeConfigOverlay)> {
    let path = claude_config_path();
    // Harness config paths are RalphX-owned runtime config paths.
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(&path).ok()?;
    let overlay = parse_claude_config_overlay(&raw)?;
    Some((path, overlay))
}

fn apply_codex_config_overlay(cfg: &mut RalphxConfig, overlay: CodexConfigOverlay) {
    cfg.agent_harness_defaults
        .extend(overlay.agent_harness_defaults);
}

fn parse_codex_config_overlay(yaml: &str) -> Option<CodexConfigOverlay> {
    serde_yaml::from_str::<CodexConfigOverlay>(yaml)
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to parse Codex harness config overlay");
            e
        })
        .ok()
}

fn load_codex_config_overlay() -> Option<(PathBuf, CodexConfigOverlay)> {
    let path = codex_config_path();
    // Harness config paths are RalphX-owned runtime config paths.
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(&path).ok()?;
    let overlay = parse_codex_config_overlay(&raw)?;
    Some((path, overlay))
}

fn parse_process_config_overlay(yaml: &str) -> Option<ProcessConfigOverlay> {
    serde_yaml::from_str::<ProcessConfigOverlay>(yaml)
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to parse process config overlay");
            e
        })
        .ok()
}

fn apply_process_config_overlay(cfg: &mut LoadedConfig, overlay: ProcessConfigOverlay) {
    if let Some(process_mapping) = overlay.process_mapping {
        cfg.process_mapping = resolve_canonical_process_mapping(&process_mapping);
    }
}

fn load_process_config_overlay() -> Option<(PathBuf, ProcessConfigOverlay)> {
    let path = process_config_path();
    // Process config paths are RalphX-owned runtime config paths.
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(&path).ok()?;
    let overlay = parse_process_config_overlay(&raw)?;
    Some((path, overlay))
}

fn apply_external_mcp_config_overlay(cfg: &mut RalphxConfig, overlay: ExternalMcpConfigOverlay) {
    let Some(overlay) = overlay.external_mcp else {
        return;
    };

    if let Some(enabled) = overlay.enabled {
        cfg.external_mcp.enabled = enabled;
    }
    if let Some(port) = overlay.port {
        cfg.external_mcp.port = port;
    }
    if let Some(host) = overlay.host {
        cfg.external_mcp.host = host;
    }
    if let Some(max_restart_attempts) = overlay.max_restart_attempts {
        cfg.external_mcp.max_restart_attempts = max_restart_attempts;
    }
    if let Some(restart_delay_ms) = overlay.restart_delay_ms {
        cfg.external_mcp.restart_delay_ms = restart_delay_ms;
    }
    if let Some(shutdown_grace_ms) = overlay.shutdown_grace_ms {
        cfg.external_mcp.shutdown_grace_ms = shutdown_grace_ms;
    }
    if let Some(startup_timeout_secs) = overlay.startup_timeout_secs {
        cfg.external_mcp.startup_timeout_secs = startup_timeout_secs;
    }
    if let Some(human_wait_timeout_secs) = overlay.human_wait_timeout_secs {
        cfg.external_mcp.human_wait_timeout_secs = human_wait_timeout_secs;
    }
    if let Some(auth_token) = overlay.auth_token {
        cfg.external_mcp.auth_token = Some(auth_token);
    }
    if let Some(node_path) = overlay.node_path {
        cfg.external_mcp.node_path = Some(node_path);
    }
    if let Some(max_external_ideation_sessions) = overlay.max_external_ideation_sessions {
        cfg.external_mcp.max_external_ideation_sessions = max_external_ideation_sessions;
    }
    if let Some(external_session_stale_secs) = overlay.external_session_stale_secs {
        cfg.external_mcp.external_session_stale_secs = external_session_stale_secs;
    }
    if let Some(external_message_queue_cap) = overlay.external_message_queue_cap {
        cfg.external_mcp.external_message_queue_cap = external_message_queue_cap;
    }
    if let Some(external_session_similarity_threshold) =
        overlay.external_session_similarity_threshold
    {
        cfg.external_mcp.external_session_similarity_threshold =
            external_session_similarity_threshold;
    }
    if let Some(external_session_startup_grace_secs) = overlay.external_session_startup_grace_secs {
        cfg.external_mcp.external_session_startup_grace_secs =
            Some(external_session_startup_grace_secs);
    }
}

fn parse_external_mcp_config_overlay(yaml: &str) -> Option<ExternalMcpConfigOverlay> {
    serde_yaml::from_str::<ExternalMcpConfigOverlay>(yaml)
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to parse external MCP config overlay");
            e
        })
        .ok()
}

fn load_external_mcp_config_overlay_from_path(
    path: &Path,
) -> Option<(PathBuf, ExternalMcpConfigOverlay)> {
    // External MCP config paths are RalphX-owned runtime config paths.
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(path).ok()?;
    let overlay = parse_external_mcp_config_overlay(&raw)?;
    Some((path.to_path_buf(), overlay))
}

fn load_embedded_external_mcp_config_overlay() -> Option<ExternalMcpConfigOverlay> {
    parse_external_mcp_config_overlay(EMBEDDED_EXTERNAL_MCP_CONFIG)
}

fn apply_external_mcp_overlay_or_embedded(parsed: &mut RalphxConfig) {
    apply_external_mcp_overlay_or_embedded_from_path(parsed, &external_mcp_config_path());
}

fn apply_external_mcp_overlay_or_embedded_from_path(
    parsed: &mut RalphxConfig,
    external_mcp_path: &Path,
) {
    if let Some((_external_mcp_path, overlay)) =
        load_external_mcp_config_overlay_from_path(external_mcp_path)
    {
        apply_external_mcp_config_overlay(parsed, overlay);
        return;
    }

    if let Some(overlay) = load_embedded_external_mcp_config_overlay() {
        apply_external_mcp_config_overlay(parsed, overlay);
    }
}

#[derive(Deserialize)]
struct MinimalEarlyConfig {
    #[serde(default = "default_file_logging")]
    file_logging: bool,
    #[serde(default = "default_file_logging_max_bytes")]
    file_logging_max_bytes: u64,
    #[serde(default = "default_file_logging_keep_files")]
    file_logging_keep_files: usize,
}

fn parse_early_file_logging(yaml: &str) -> Option<bool> {
    serde_yaml::from_str::<MinimalEarlyConfig>(yaml)
        .ok()
        .map(|config| config.file_logging)
}

fn parse_early_file_logging_limits(yaml: &str) -> Option<(u64, usize)> {
    serde_yaml::from_str::<MinimalEarlyConfig>(yaml)
        .ok()
        .map(|config| {
            (
                config.file_logging_max_bytes,
                config.file_logging_keep_files,
            )
        })
}

fn resolve_file_logging_limits_from_sources(
    max_bytes_environment_value: Option<&str>,
    keep_files_environment_value: Option<&str>,
    runtime_config_path: Option<&Path>,
    embedded_config: &str,
) -> (u64, usize) {
    let embedded = parse_early_file_logging_limits(embedded_config).unwrap_or_else(|| {
        (
            default_file_logging_max_bytes(),
            default_file_logging_keep_files(),
        )
    });
    let configured = runtime_config_path
        .and_then(|path| {
            // Runtime config paths are established from RalphX-owned bundled resources.
            // codeql[rust/path-injection]
            std::fs::read_to_string(path)
                .ok()
                .and_then(|contents| parse_early_file_logging_limits(&contents))
        })
        .unwrap_or(embedded);

    let max_bytes = max_bytes_environment_value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(configured.0);
    // A zero cap (from any source, including YAML) would create a writer that
    // silently records nothing; fall back to the default instead.
    let max_bytes = if max_bytes == 0 {
        default_file_logging_max_bytes()
    } else {
        max_bytes
    };
    let keep_files = keep_files_environment_value
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(configured.1);
    (max_bytes, keep_files)
}

fn resolve_file_logging_from_sources(
    environment_value: Option<&str>,
    runtime_config_path: Option<&Path>,
    embedded_config: &str,
) -> bool {
    if let Some(value) = environment_value {
        return matches!(value.to_lowercase().as_str(), "true" | "1" | "yes");
    }

    if let Some(path) = runtime_config_path {
        // Runtime config paths are established from RalphX-owned bundled resources.
        // codeql[rust/path-injection]
        if let Ok(contents) = std::fs::read_to_string(path) {
            if let Some(file_logging) = parse_early_file_logging(&contents) {
                return file_logging;
            }
        }
    }

    parse_early_file_logging(embedded_config).unwrap_or(true)
}

/// Resolve file logging before tracing initialization.
///
/// Priority: environment override, configured bundled runtime file, embedded canonical config,
/// then the fail-safe default (`true`).
pub fn resolve_file_logging_early() -> bool {
    let environment_value = std::env::var("RALPHX_FILE_LOGGING").ok();
    let runtime_config_path = configured_runtime_config_dir().map(|dir| dir.join("ralphx.yaml"));
    resolve_file_logging_from_sources(
        environment_value.as_deref(),
        runtime_config_path.as_deref(),
        EMBEDDED_CONFIG,
    )
}

/// Resolves the file logging cap and retained previous-run file count before tracing starts.
pub fn resolve_file_logging_limits_early() -> (u64, usize) {
    let max_bytes_environment_value = std::env::var("RALPHX_FILE_LOGGING_MAX_BYTES").ok();
    let keep_files_environment_value = std::env::var("RALPHX_FILE_LOGGING_KEEP_FILES").ok();
    let runtime_config_path = configured_runtime_config_dir().map(|dir| dir.join("ralphx.yaml"));
    resolve_file_logging_limits_from_sources(
        max_bytes_environment_value.as_deref(),
        keep_files_environment_value.as_deref(),
        runtime_config_path.as_deref(),
        EMBEDDED_CONFIG,
    )
}

fn resolve_tools_from_spec(
    agent_name: &str,
    tools: &AgentToolsSpec,
    tool_sets: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    if tools.mcp_only {
        return Vec::new();
    }

    let mut out = Vec::<String>::new();

    let extends = tools.extends.as_deref().unwrap_or("base_tools");

    if let Some(base) = tool_sets.get(extends) {
        out.extend(base.iter().cloned());
    } else if let Some(base) = canonical_claude_tool_sets().get(extends) {
        out.extend(base.iter().cloned());
    } else {
        tracing::warn!(agent = %agent_name, tool_set = %extends, "Unknown tools.extends set; using include only");
    }

    out.extend(tools.include.iter().cloned());

    // Stable de-dup while preserving first-seen order
    let mut seen = HashSet::new();
    out.retain(|t| seen.insert(t.clone()));
    out
}

fn runtime_tools_spec_from_canonical(spec: &CanonicalClaudeToolSpec) -> AgentToolsSpec {
    AgentToolsSpec {
        mcp_only: spec.mcp_only,
        extends: spec.extends.clone(),
        include: spec.include.clone(),
    }
}

fn resolve_tool_spec(project_root: &Path, raw: &AgentConfigRaw) -> AgentToolsSpec {
    let Ok(metadata) = try_load_canonical_claude_metadata(project_root, &raw.name) else {
        return raw.tools.clone();
    };

    let Some(spec) = metadata.tools else {
        return raw.tools.clone();
    };

    let canonical_spec = runtime_tools_spec_from_canonical(&spec);
    if raw.tools != canonical_spec && !tools_spec_is_default(&raw.tools) {
        tracing::warn!(
            agent = %raw.name,
            runtime_tools = ?raw.tools,
            canonical_tools = ?canonical_spec,
            "Canonical Claude metadata overrides divergent runtime tools spec"
        );
    } else if raw.tools != canonical_spec {
        tracing::debug!(
            agent = %raw.name,
            canonical_tools = ?canonical_spec,
            "Canonical Claude metadata fills default runtime tools spec"
        );
    }

    canonical_spec
}

fn canonical_agent_project_root() -> PathBuf {
    let fallback_plugin_dir = super::find_base_plugin_dir();
    canonical_agent_project_root_from_config_path(&config_path(), fallback_plugin_dir.as_deref())
}

fn canonical_agent_project_root_from_config_path(
    config_path: &Path,
    fallback_runtime_plugin_dir: Option<&Path>,
) -> PathBuf {
    let config_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."));

    if let Some(project_root) = resolve_project_root_from_catalog_path(&config_dir) {
        return project_root;
    }

    if let Some(runtime_plugin_dir) = fallback_runtime_plugin_dir {
        let runtime_project_root = resolve_project_root_from_plugin_dir(runtime_plugin_dir);
        if let Some(project_root) = resolve_project_root_from_catalog_path(&runtime_project_root) {
            return project_root;
        }
    }

    resolve_project_root_from_plugin_dir(&config_dir)
}

fn resolve_system_prompt_file(project_root: &Path, raw: &AgentConfigRaw) -> String {
    let canonical_prompt =
        resolve_harness_agent_prompt_path(project_root, &raw.name, AgentPromptHarness::Claude)
            .and_then(|path| {
                path.strip_prefix(project_root)
                    .ok()
                    .map(|relative| relative.to_string_lossy().to_string())
            });

    if let Some(canonical_prompt) = canonical_prompt {
        if raw.system_prompt_file.as_deref().is_some()
            && raw.system_prompt_file.as_deref() != Some(canonical_prompt.as_str())
        {
            tracing::warn!(
                agent = %raw.name,
                runtime_system_prompt_file = ?raw.system_prompt_file,
                canonical_system_prompt_file = %canonical_prompt,
                "Canonical prompt path overrides divergent runtime system_prompt_file"
            );
        }
        return canonical_prompt;
    }

    match &raw.system_prompt_file {
        Some(path) => path.clone(),
        None => {
            tracing::warn!(
                agent = %raw.name,
                "Agent has no system_prompt_file and no canonical Claude prompt path"
            );
            String::new()
        }
    }
}

fn resolve_allowed_mcp_tools(project_root: &Path, raw: &AgentConfigRaw) -> Vec<String> {
    let Some(definition) = load_canonical_agent_definition(project_root, &raw.name) else {
        return raw.mcp_tools.clone();
    };

    if definition.capabilities.mcp_tools.is_empty() {
        return raw.mcp_tools.clone();
    }

    if !raw.mcp_tools.is_empty() && raw.mcp_tools != definition.capabilities.mcp_tools {
        tracing::warn!(
            agent = %raw.name,
            runtime_tools = ?raw.mcp_tools,
            canonical_tools = ?definition.capabilities.mcp_tools,
            "Canonical agent metadata overrides divergent runtime mcp_tools"
        );
    }

    definition.capabilities.mcp_tools
}

fn resolve_preapproved_cli_tools(project_root: &Path, raw: &AgentConfigRaw) -> Vec<String> {
    let Ok(metadata) = try_load_canonical_claude_metadata(project_root, &raw.name) else {
        return raw.preapproved_cli_tools.clone();
    };

    if metadata.preapproved_cli_tools.is_empty() {
        return raw.preapproved_cli_tools.clone();
    }

    if !raw.preapproved_cli_tools.is_empty()
        && raw.preapproved_cli_tools != metadata.preapproved_cli_tools
    {
        tracing::warn!(
            agent = %raw.name,
            runtime_preapproved_cli_tools = ?raw.preapproved_cli_tools,
            canonical_preapproved_cli_tools = ?metadata.preapproved_cli_tools,
            "Canonical Claude metadata overrides divergent runtime preapproved_cli_tools"
        );
    }

    metadata.preapproved_cli_tools
}

fn resolve_model(project_root: &Path, raw: &AgentConfigRaw) -> Option<String> {
    let Ok(metadata) = try_load_canonical_claude_metadata(project_root, &raw.name) else {
        return raw.model.clone();
    };

    let Some(model) = metadata.model else {
        return raw.model.clone();
    };

    if raw.model.as_deref().is_some() && raw.model.as_deref() != Some(model.as_str()) {
        tracing::warn!(
            agent = %raw.name,
            runtime_model = ?raw.model,
            canonical_model = %model,
            "Canonical Claude metadata overrides divergent runtime model"
        );
    }

    Some(model)
}

fn resolve_effort(project_root: &Path, raw: &AgentConfigRaw) -> Option<String> {
    let runtime_effort = raw.effort.clone().filter(|v| validate_effort(v, &raw.name));
    let Ok(metadata) = try_load_canonical_claude_metadata(project_root, &raw.name) else {
        return runtime_effort;
    };

    let Some(effort) = metadata.effort else {
        return runtime_effort;
    };

    if !validate_effort(&effort, &raw.name) {
        return runtime_effort;
    }

    if runtime_effort.as_deref().is_some() && runtime_effort.as_deref() != Some(effort.as_str()) {
        tracing::warn!(
            agent = %raw.name,
            runtime_effort = ?runtime_effort,
            canonical_effort = %effort,
            "Canonical Claude metadata overrides divergent runtime effort"
        );
    }

    Some(effort)
}

fn resolve_permission_mode(project_root: &Path, raw: &AgentConfigRaw) -> Option<String> {
    let Ok(metadata) = try_load_canonical_claude_metadata(project_root, &raw.name) else {
        return raw.permission_mode.clone();
    };

    let Some(permission_mode) = metadata.permission_mode else {
        return raw.permission_mode.clone();
    };

    if raw.permission_mode.as_deref().is_some()
        && raw.permission_mode.as_deref() != Some(permission_mode.as_str())
    {
        tracing::warn!(
            agent = %raw.name,
            runtime_permission_mode = ?raw.permission_mode,
            canonical_permission_mode = %permission_mode,
            "Canonical Claude metadata overrides divergent runtime permission_mode"
        );
    }

    Some(permission_mode)
}

// ── Agent config inheritance (extends) ──────────────────────────────────

/// Check if a tools spec has any explicit user-provided values.
fn tools_spec_is_default(spec: &AgentToolsSpec) -> bool {
    !spec.mcp_only && spec.extends.is_none() && spec.include.is_empty()
}

/// Recursively resolve agent inheritance via `extends` field.
///
/// Child fields override parent; missing/default fields fall through.
/// Circular extends detected via stack tracking.
fn resolve_agent_extends(
    raw: &AgentConfigRaw,
    all_agents: &[AgentConfigRaw],
    stack: &mut Vec<String>,
) -> AgentConfigRaw {
    let parent_name = match &raw.extends {
        Some(name) => name,
        None => return raw.clone(),
    };

    if stack.contains(parent_name) {
        tracing::warn!(
            agent = %raw.name,
            parent = %parent_name,
            chain = ?stack,
            "Circular agent extends detected"
        );
        return raw.clone();
    }

    stack.push(parent_name.clone());
    let parent = all_agents.iter().find(|a| a.name == *parent_name);
    let result = if let Some(parent) = parent {
        let resolved_parent = resolve_agent_extends(parent, all_agents, stack);
        merge_agent_configs(&resolved_parent, raw)
    } else {
        tracing::warn!(
            agent = %raw.name,
            parent = %parent_name,
            "Agent extends references unknown parent"
        );
        raw.clone()
    };
    stack.pop();
    result
}

/// Merge parent and child agent configs. Child fields override parent.
fn merge_agent_configs(parent: &AgentConfigRaw, child: &AgentConfigRaw) -> AgentConfigRaw {
    AgentConfigRaw {
        name: child.name.clone(),
        extends: None, // inheritance resolved
        system_prompt_file: child
            .system_prompt_file
            .clone()
            .or_else(|| parent.system_prompt_file.clone()),
        model: child.model.clone().or_else(|| parent.model.clone()),
        tools: if tools_spec_is_default(&child.tools) {
            parent.tools.clone()
        } else {
            child.tools.clone()
        },
        mcp_tools: if child.mcp_tools.is_empty() {
            parent.mcp_tools.clone()
        } else {
            child.mcp_tools.clone()
        },
        preapproved_cli_tools: if child.preapproved_cli_tools.is_empty() {
            parent.preapproved_cli_tools.clone()
        } else {
            child.preapproved_cli_tools.clone()
        },
        settings_profile: child
            .settings_profile
            .clone()
            .or_else(|| parent.settings_profile.clone()),
        effort: child.effort.clone().or_else(|| parent.effort.clone()),
        permission_mode: child
            .permission_mode
            .clone()
            .or_else(|| parent.permission_mode.clone()),
    }
}

fn canonical_runtime_agent_stub(name: String) -> AgentConfigRaw {
    AgentConfigRaw {
        name,
        extends: None,
        tools: AgentToolsSpec::default(),
        mcp_tools: Vec::new(),
        preapproved_cli_tools: Vec::new(),
        system_prompt_file: None,
        model: None,
        settings_profile: None,
        effort: None,
        permission_mode: None,
    }
}

fn resolve_loaded_config_with_lookup(
    parsed: RalphxConfig,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Option<LoadedConfig> {
    let canonical_project_root = canonical_agent_project_root();
    let canonical_runtime_agents =
        list_canonical_prompt_backed_agents(&canonical_project_root, AgentPromptHarness::Claude);
    let canonical_runtime_agent_names = canonical_runtime_agents
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let raw_agents = canonical_runtime_agents
        .into_iter()
        .map(|name| {
            parsed
                .agents
                .iter()
                .find(|raw| raw.name == name)
                .cloned()
                .unwrap_or_else(|| canonical_runtime_agent_stub(name))
        })
        .chain(
            parsed
                .agents
                .iter()
                .filter(|raw| !canonical_runtime_agent_names.contains(&raw.name))
                .cloned(),
        )
        .collect::<Vec<_>>();

    // Phase 1: resolve extends inheritance for all agents
    let resolved_raw_agents: Vec<AgentConfigRaw> = raw_agents
        .iter()
        .map(|raw| {
            let mut stack = Vec::new();
            resolve_agent_extends(raw, &raw_agents, &mut stack)
        })
        .collect();

    let mut seen_names = HashSet::new();
    let mut resolved = Vec::with_capacity(resolved_raw_agents.len());
    let global_profile_selection = runtime_settings_profile_override_with(lookup)
        .or_else(|| parsed.claude.settings_profile.clone());
    let resolved_settings =
        resolve_claude_settings(&parsed.claude, global_profile_selection.as_deref());

    for raw in &resolved_raw_agents {
        if !seen_names.insert(raw.name.clone()) {
            tracing::warn!(agent = %raw.name, "Duplicate agent name in config");
            return None;
        }

        let system_prompt = resolve_system_prompt_file(&canonical_project_root, raw);

        let tool_spec = resolve_tool_spec(&canonical_project_root, raw);
        let cli_tools = resolve_tools_from_spec(raw.name.as_str(), &tool_spec, &parsed.tool_sets);
        let agent_profile_selection =
            runtime_settings_profile_override_for_agent_with(&raw.name, lookup)
                .or_else(|| raw.settings_profile.clone());
        let agent_settings = if let Some(profile_name) = agent_profile_selection.as_deref() {
            if parsed.claude.settings_profiles.contains_key(profile_name) {
                resolve_claude_settings(&parsed.claude, Some(profile_name))
            } else {
                tracing::warn!(
                    agent = %raw.name,
                    profile = profile_name,
                    "Unknown agent settings_profile; falling back to global settings profile"
                );
                resolved_settings.clone()
            }
        } else {
            resolved_settings.clone()
        };
        let allowed_mcp_tools = resolve_allowed_mcp_tools(&canonical_project_root, raw);
        let preapproved_cli_tools = resolve_preapproved_cli_tools(&canonical_project_root, raw);
        let model = resolve_model(&canonical_project_root, raw);
        let effort = resolve_effort(&canonical_project_root, raw);
        let permission_mode = resolve_permission_mode(&canonical_project_root, raw);
        resolved.push(AgentConfig {
            name: raw.name.clone(),
            mcp_only: tool_spec.mcp_only,
            resolved_cli_tools: cli_tools,
            allowed_mcp_tools,
            preapproved_cli_tools,
            system_prompt_file: system_prompt,
            model,
            settings_profile: agent_profile_selection.clone(),
            settings: agent_settings,
            effort,
            permission_mode,
        });
    }

    let mcp_server_name = parsed.claude.mcp_server_name.clone();
    let resolved_settings =
        resolve_claude_settings(&parsed.claude, global_profile_selection.as_deref());
    let claude = ClaudeRuntimeConfig {
        mcp_server_name,
        setting_sources: parsed.claude.setting_sources,
        permission_mode: parsed.claude.permission_mode,
        dangerously_skip_permissions: parsed.claude.dangerously_skip_permissions,
        allow_dangerously_skip_permissions: parsed.claude.allow_dangerously_skip_permissions,
        permission_prompt_tool: normalize_mcp_tool_name(
            &parsed.claude.permission_prompt_tool,
            &parsed.claude.mcp_server_name,
        ),
        use_append_system_prompt_file: parsed.claude.append_system_prompt_file,
        settings: resolved_settings,
        default_effort: parsed
            .claude
            .default_effort
            .filter(|v| VALID_EFFORT_LEVELS.contains(&v.as_str()))
            .unwrap_or_else(|| "medium".to_string()),
    };

    let ui_feature_flags = parsed
        .ui
        .as_ref()
        .and_then(|u| u.feature_flags.clone())
        .unwrap_or_default();
    let mut runtime = AllRuntimeConfig {
        database_maintenance: parsed.database_maintenance,
        stream: parsed.timeouts.stream,
        reconciliation: parsed.reconciliation,
        git: parsed.git,
        scheduler: parsed.scheduler,
        supervisor: parsed.supervisor,
        limits: parsed.limits,
        verification: parsed.ideation.verification,
        external_mcp: parsed.external_mcp,
        delegation: parsed.delegation,
        workspace_review: parsed.workspace_review,
        child_session_activity_threshold_secs: parsed
            .ideation
            .child_session_activity_threshold_secs,
        ui_feature_flags,
    };
    let mut automations = parsed.automations;
    if runtime.external_mcp.max_external_ideation_sessions != 1 {
        tracing::warn!(
            value = runtime.external_mcp.max_external_ideation_sessions,
            "config/external-mcp.yaml: external_mcp.max_external_ideation_sessions is deprecated and \
             has no effect. The session gate was removed; sessions are always created. Remove \
             this field."
        );
    }
    runtime_config::apply_env_overrides_with_lookup(&mut runtime, lookup);
    let mut shutdown = parsed.shutdown;
    runtime_config::apply_shutdown_env_overrides_with_lookup(&mut shutdown, lookup);
    runtime_config::apply_automations_env_overrides_with_lookup(&mut automations, lookup);
    let mut agent_harness_defaults = parsed
        .agent_harness_defaults
        .into_iter()
        .map(|(lane, settings)| (lane, settings.into()))
        .collect::<AgentHarnessDefaultsConfig>();
    apply_agent_harness_env_overrides_with(&mut agent_harness_defaults, lookup);

    let process_mapping = resolve_canonical_process_mapping(&parsed.process_mapping);

    Some(LoadedConfig {
        agents: resolved,
        claude,
        tool_sets: parsed.tool_sets,
        process_mapping,
        defer_merge_enabled: parsed.defer_merge_enabled,
        file_logging: parsed.file_logging,
        shutdown,
        runtime,
        automations,
        execution_defaults: parsed.execution_defaults,
        agent_harness_defaults,
    })
}

#[cfg(test)]
fn parse_config_with_lookup(
    yaml: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Option<LoadedConfig> {
    let parsed = parse_raw_config(yaml)?;
    resolve_loaded_config_with_lookup(parsed, lookup)
}

#[cfg(test)]
fn parse_config(yaml: &str) -> Option<LoadedConfig> {
    parse_config_with_lookup(yaml, &|name| std::env::var(name).ok())
}

#[cfg(test)]
fn parse_config_no_env_overrides(yaml: &str) -> Option<LoadedConfig> {
    parse_config_with_lookup(yaml, &|_| None)
}

fn runtime_settings_profile_override_with(
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    lookup("RALPHX_CLAUDE_SETTINGS_PROFILE").and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn runtime_settings_profile_override_for_agent_with(
    agent_name: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let normalized = normalize_agent_name_for_env(agent_name);
    let key = format!("RALPHX_CLAUDE_SETTINGS_PROFILE_{}", normalized);
    lookup(&key).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_lane_name_for_env(lane: AgentLane) -> String {
    lane.to_string()
        .chars()
        .map(|ch| match ch {
            'a'..='z' => ch.to_ascii_uppercase(),
            'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect()
}

fn normalize_override_value(raw: Option<String>) -> Option<String> {
    raw.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn all_agent_lanes() -> [AgentLane; 9] {
    [
        AgentLane::IdeationPrimary,
        AgentLane::IdeationVerifier,
        AgentLane::IdeationSubagent,
        AgentLane::IdeationVerifierSubagent,
        AgentLane::ExecutionWorker,
        AgentLane::ExecutionReviewer,
        AgentLane::ExecutionReexecutor,
        AgentLane::ExecutionMerger,
        AgentLane::ExecutionBranchUpdater,
    ]
}

fn apply_agent_harness_env_overrides_with(
    defaults: &mut AgentHarnessDefaultsConfig,
    lookup: &dyn Fn(&str) -> Option<String>,
) {
    for lane in all_agent_lanes() {
        let lane_key = normalize_lane_name_for_env(lane);
        let harness_key = format!("RALPHX_AGENT_HARNESS_{lane_key}");
        let model_key = format!("RALPHX_AGENT_MODEL_{lane_key}");
        let effort_key = format!("RALPHX_AGENT_EFFORT_{lane_key}");
        let approval_key = format!("RALPHX_AGENT_APPROVAL_POLICY_{lane_key}");
        let sandbox_key = format!("RALPHX_AGENT_SANDBOX_MODE_{lane_key}");

        let existing = defaults.get(&lane).cloned();
        let mut settings = existing
            .clone()
            .unwrap_or_else(|| AgentLaneSettings::new(AgentHarnessKind::Claude));
        let mut changed = false;

        if let Some(raw) = normalize_override_value(lookup(&harness_key)) {
            match raw.parse::<AgentHarnessKind>() {
                Ok(value) => {
                    settings.harness = value;
                    changed = true;
                }
                Err(error) => {
                    tracing::warn!(lane = %lane, env = %harness_key, value = %raw, %error, "Ignoring invalid agent harness env override");
                }
            }
        }

        if let Some(raw) = normalize_override_value(lookup(&model_key)) {
            settings.model = Some(raw);
            changed = true;
        }

        if let Some(raw) = normalize_override_value(lookup(&effort_key)) {
            match raw.parse::<LogicalEffort>() {
                Ok(value) => {
                    settings.effort = Some(value);
                    changed = true;
                }
                Err(error) => {
                    tracing::warn!(lane = %lane, env = %effort_key, value = %raw, %error, "Ignoring invalid agent effort env override");
                }
            }
        }

        if let Some(raw) = normalize_override_value(lookup(&approval_key)) {
            settings.approval_policy = Some(raw);
            changed = true;
        }

        if let Some(raw) = normalize_override_value(lookup(&sandbox_key)) {
            settings.sandbox_mode = Some(raw);
            changed = true;
        }

        if !changed {
            continue;
        }

        defaults.insert(lane, settings);
    }
}

fn normalize_agent_name_for_env(agent_name: &str) -> String {
    let mut out = String::with_capacity(agent_name.len());
    for ch in agent_name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    out
}

fn normalize_profile_name_for_env(profile_name: &str) -> String {
    let mut out = String::with_capacity(profile_name.len());
    for ch in profile_name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    out
}

fn resolve_claude_settings(
    raw: &ClaudeRuntimeConfigRaw,
    profile_selection: Option<&str>,
) -> Option<serde_json::Value> {
    let selected_profile = if let Some(profile_name) = profile_selection {
        Some(profile_name)
    } else if raw.settings_profiles.contains_key("default") {
        Some("default")
    } else {
        None
    };

    let mut selected = if let Some(profile_name) = selected_profile {
        resolve_profile_settings(raw, profile_name)
    } else {
        raw.settings.clone()
    };

    if let Some(defaults) = raw.settings_profile_defaults.clone() {
        selected = Some(match selected {
            Some(profile) => merge_settings(defaults, profile),
            None => defaults,
        });
    }

    if let Some(ref mut value) = selected {
        apply_prefixed_env_overrides(value, selected_profile);
        if value.as_object().is_some_and(|obj| obj.is_empty()) {
            return None;
        }
    }

    selected
}

fn resolve_profile_settings(
    raw: &ClaudeRuntimeConfigRaw,
    profile_name: &str,
) -> Option<serde_json::Value> {
    let mut stack = Vec::<String>::new();
    resolve_profile_settings_inner(raw, profile_name, &mut stack)
}

fn resolve_profile_settings_inner(
    raw: &ClaudeRuntimeConfigRaw,
    profile_name: &str,
    stack: &mut Vec<String>,
) -> Option<serde_json::Value> {
    if stack.iter().any(|v| v == profile_name) {
        tracing::warn!(
            profile = profile_name,
            chain = ?stack,
            "Cycle detected while resolving claude settings profile extends"
        );
        return None;
    }

    let profile = match raw.settings_profiles.get(profile_name) {
        Some(v) => v.clone(),
        None => {
            tracing::warn!(
                profile = profile_name,
                "Unknown claude.settings_profile; falling back to no custom settings"
            );
            return None;
        }
    };

    stack.push(profile_name.to_string());

    let mut merged = serde_json::json!({});
    let mut current_profile = profile;

    if let Some(current_obj) = current_profile.as_object_mut() {
        let extends_value = current_obj.remove("extends");
        if let Some(extends_list) = parse_extends_list(extends_value.as_ref(), profile_name) {
            for base_name in extends_list {
                if let Some(base) = resolve_profile_settings_inner(raw, &base_name, stack) {
                    merged = merge_settings(merged, base);
                }
            }
        }
    }

    stack.pop();
    Some(merge_settings(merged, current_profile))
}

fn parse_extends_list(
    extends_value: Option<&serde_json::Value>,
    profile_name: &str,
) -> Option<Vec<String>> {
    let value = extends_value?;
    match value {
        serde_json::Value::String(s) => Some(vec![s.clone()]),
        serde_json::Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                if let Some(name) = item.as_str() {
                    out.push(name.to_string());
                } else {
                    tracing::warn!(
                        profile = profile_name,
                        invalid = ?item,
                        "Ignoring non-string entry in profile extends list"
                    );
                }
            }
            Some(out)
        }
        other => {
            tracing::warn!(
                profile = profile_name,
                invalid = ?other,
                "Ignoring invalid profile extends value; expected string or array"
            );
            None
        }
    }
}

fn merge_settings(base: serde_json::Value, overlay: serde_json::Value) -> serde_json::Value {
    match (base, overlay) {
        (serde_json::Value::Object(mut base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                let merged_value = if let Some(base_value) = base_map.remove(&key) {
                    merge_settings(base_value, overlay_value)
                } else {
                    overlay_value
                };
                base_map.insert(key, merged_value);
            }
            serde_json::Value::Object(base_map)
        }
        (_, overlay_value) => overlay_value,
    }
}

fn apply_prefixed_env_overrides(settings: &mut serde_json::Value, profile_name: Option<&str>) {
    apply_prefixed_env_overrides_with(settings, profile_name, &|name| std::env::var(name).ok());
}

fn apply_prefixed_env_overrides_with(
    settings: &mut serde_json::Value,
    profile_name: Option<&str>,
    lookup: &dyn Fn(&str) -> Option<String>,
) {
    let Some(env_settings) = settings.get_mut("env").and_then(|v| v.as_object_mut()) else {
        return;
    };

    let normalized_profile = profile_name.map(normalize_profile_name_for_env);
    for (target_key, target_value) in env_settings.iter_mut() {
        let profile_source_key = normalized_profile
            .as_ref()
            .map(|profile| format!("RALPHX_{profile}_{target_key}"));
        let generic_source_key = format!("RALPHX_{target_key}");

        let value = profile_source_key
            .as_deref()
            .and_then(lookup)
            .or_else(|| lookup(&generic_source_key));

        if let Some(value) = value {
            *target_value = serde_json::Value::String(value);
        }
    }
}

fn load_config() -> LoadedConfig {
    let path = config_path();
    // Main config path is a RalphX-owned runtime config path.
    // codeql[rust/path-injection]
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Some(mut parsed) = parse_raw_config(&raw) {
            if let Some((claude_path, overlay)) = load_claude_config_overlay() {
                apply_claude_config_overlay(&mut parsed, overlay);
                tracing::info!(
                    path = %claude_path.display(),
                    "Loaded Claude harness config overlay from config/harnesses/claude.yaml"
                );
            }
            if let Some((codex_path, overlay)) = load_codex_config_overlay() {
                apply_codex_config_overlay(&mut parsed, overlay);
                tracing::info!(
                    path = %codex_path.display(),
                    "Loaded Codex harness config overlay from config/harnesses/codex.yaml"
                );
            }
            apply_external_mcp_overlay_or_embedded(&mut parsed);
            if let Some(mut cfg) =
                resolve_loaded_config_with_lookup(parsed, &|name| std::env::var(name).ok())
            {
                if let Some((process_path, overlay)) = load_process_config_overlay() {
                    apply_process_config_overlay(&mut cfg, overlay);
                    tracing::info!(
                        path = %process_path.display(),
                        "Loaded process config overlay from config/processes.yaml"
                    );
                }
                tracing::info!(
                    path = %path.display(),
                    agents = cfg.agents.len(),
                    permission_mode = %cfg.claude.permission_mode,
                    dangerously_skip_permissions = cfg.claude.dangerously_skip_permissions,
                    append_system_prompt_file = cfg.claude.use_append_system_prompt_file,
                    "Loaded agent config from RalphX config file"
                );
                return cfg;
            }
        }
        tracing::warn!(path = %path.display(), "Falling back to embedded config due to parse error");
    } else {
        tracing::warn!(path = %path.display(), "RalphX config file not found/readable, using embedded config");
    }

    let mut cfg = parse_raw_config(EMBEDDED_CONFIG)
        .and_then(|mut parsed| {
            if let Some((claude_path, overlay)) = load_claude_config_overlay() {
                apply_claude_config_overlay(&mut parsed, overlay);
                tracing::info!(
                    path = %claude_path.display(),
                    "Loaded Claude harness config overlay from config/harnesses/claude.yaml"
                );
            }
            if let Some((codex_path, overlay)) = load_codex_config_overlay() {
                apply_codex_config_overlay(&mut parsed, overlay);
                tracing::info!(
                    path = %codex_path.display(),
                    "Loaded Codex harness config overlay from config/harnesses/codex.yaml"
                );
            }
            apply_external_mcp_overlay_or_embedded(&mut parsed);
            resolve_loaded_config_with_lookup(parsed, &|name| std::env::var(name).ok())
        })
        .unwrap_or_else(|| {
            let mut runtime = AllRuntimeConfig {
                database_maintenance: runtime_config::DatabaseMaintenanceConfig::default(),
                stream: StreamTimeoutsConfig::default(),
                reconciliation: ReconciliationConfig::default(),
                git: GitRuntimeConfig::default(),
                scheduler: SchedulerConfig::default(),
                supervisor: SupervisorRuntimeConfig::default(),
                limits: LimitsConfig::default(),
                verification: VerificationConfig::default(),
                external_mcp: ExternalMcpConfig::default(),
                delegation: runtime_config::DelegationConfig::default(),
                workspace_review: runtime_config::WorkspaceReviewRuntimeConfig::default(),
                child_session_activity_threshold_secs: None,
                ui_feature_flags: UiFeatureFlagsConfig::default(),
            };
            runtime_config::apply_env_overrides(&mut runtime);
            let mut automations = AutomationsRuntimeConfig::default();
            runtime_config::apply_automations_env_overrides_with_lookup(
                &mut automations,
                &|name| std::env::var(name).ok(),
            );
            LoadedConfig {
                agents: Vec::new(),
                claude: ClaudeRuntimeConfig {
                    mcp_server_name: "ralphx".to_string(),
                    setting_sources: None,
                    permission_mode: CLAUDE_DEFAULT_PERMISSION_MODE.to_string(),
                    dangerously_skip_permissions: CLAUDE_DEFAULT_DANGEROUSLY_SKIP_PERMISSIONS,
                    allow_dangerously_skip_permissions:
                        CLAUDE_DEFAULT_ALLOW_DANGEROUSLY_SKIP_PERMISSIONS,
                    permission_prompt_tool: "mcp__ralphx__permission_request".to_string(),
                    use_append_system_prompt_file: true,
                    settings: None,
                    default_effort: "medium".to_string(),
                },
                tool_sets: canonical_claude_tool_sets().clone(),
                process_mapping: ProcessMapping::default(),
                defer_merge_enabled: true,
                file_logging: true,
                shutdown: ShutdownConfig::default(),
                runtime,
                automations,
                execution_defaults: ExecutionDefaultsConfig::default(),
                agent_harness_defaults: default_agent_harness_defaults(),
            }
        });

    if let Some((process_path, overlay)) = load_process_config_overlay() {
        apply_process_config_overlay(&mut cfg, overlay);
        tracing::info!(
            path = %process_path.display(),
            "Loaded process config overlay from config/processes.yaml"
        );
    }

    cfg
}

pub fn agent_configs() -> &'static [AgentConfig] {
    LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .agents
        .as_slice()
}

pub fn claude_runtime_config() -> &'static ClaudeRuntimeConfig {
    &LOADED_CONFIG_CELL.get_or_init(load_config).claude
}

pub fn get_agent_config(agent_name: &str) -> Option<&'static AgentConfig> {
    let lookup_name = super::canonical_short_agent_name(agent_name);
    agent_configs().iter().find(|c| c.name == lookup_name)
}

pub fn get_agent_config_for_profile(
    agent_name: &str,
    profile_name: Option<&str>,
) -> Option<AgentConfig> {
    let loaded = LOADED_CONFIG_CELL.get_or_init(load_config);
    let lookup_name = super::canonical_short_agent_name(agent_name);
    let mut config = loaded
        .agents
        .iter()
        .find(|c| c.name == lookup_name)?
        .clone();
    let Some(profile_name) = profile_name else {
        return Some(config);
    };
    let project_root = canonical_agent_project_root();
    let definition = load_canonical_agent_definition_for_profile(
        &project_root,
        lookup_name,
        Some(profile_name),
    )?;
    let metadata = try_load_canonical_claude_metadata_for_profile(
        &project_root,
        lookup_name,
        Some(profile_name),
    )
    .ok()?;

    if !definition.capabilities.mcp_tools.is_empty() {
        config.allowed_mcp_tools = definition.capabilities.mcp_tools;
    }
    if let Some(spec) = metadata.tools.as_ref() {
        let tools = runtime_tools_spec_from_canonical(spec);
        config.mcp_only = tools.mcp_only;
        config.resolved_cli_tools = resolve_tools_from_spec(lookup_name, &tools, &loaded.tool_sets);
    }
    config.preapproved_cli_tools = metadata.preapproved_cli_tools;
    if metadata.model.is_some() {
        config.model = metadata.model;
    }
    if metadata.effort.is_some() {
        config.effort = metadata.effort;
    }
    if metadata.permission_mode.is_some() {
        config.permission_mode = metadata.permission_mode;
    }
    Some(config)
}

pub fn get_effective_settings(agent_name: Option<&str>) -> Option<&'static serde_json::Value> {
    let loaded = LOADED_CONFIG_CELL.get_or_init(load_config);
    if let Some(name) = agent_name {
        let lookup_name = super::canonical_short_agent_name(name);
        if let Some(agent) = loaded.agents.iter().find(|c| c.name == lookup_name) {
            return agent.settings.as_ref();
        }
    }
    loaded.claude.settings.as_ref()
}

pub fn get_effective_settings_profile(agent_name: Option<&str>) -> Option<&'static str> {
    let loaded = LOADED_CONFIG_CELL.get_or_init(load_config);
    if let Some(name) = agent_name {
        let lookup_name = super::canonical_short_agent_name(name);
        if let Some(agent) = loaded.agents.iter().find(|c| c.name == lookup_name) {
            return agent.settings_profile.as_deref();
        }
    }
    None
}

pub fn get_allowed_tools(agent_name: &str) -> Option<String> {
    get_allowed_tools_for_profile(agent_name, None)
}

pub fn get_allowed_tools_for_profile(
    agent_name: &str,
    profile_name: Option<&str>,
) -> Option<String> {
    get_agent_config_for_profile(agent_name, profile_name).map(|c| {
        if c.mcp_only {
            String::new()
        } else {
            c.resolved_cli_tools.join(",")
        }
    })
}

pub fn process_mapping() -> &'static ProcessMapping {
    &LOADED_CONFIG_CELL.get_or_init(load_config).process_mapping
}

pub fn defer_merge_enabled() -> bool {
    LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .defer_merge_enabled
}

pub fn file_logging_enabled() -> bool {
    LOADED_CONFIG_CELL.get_or_init(load_config).file_logging
}

pub fn shutdown_config() -> &'static ShutdownConfig {
    &LOADED_CONFIG_CELL.get_or_init(load_config).shutdown
}

pub fn execution_defaults_config() -> &'static ExecutionDefaultsConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .execution_defaults
}

pub fn agent_harness_defaults_config() -> &'static AgentHarnessDefaultsConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .agent_harness_defaults
}

pub fn stream_timeouts() -> &'static StreamTimeoutsConfig {
    &LOADED_CONFIG_CELL.get_or_init(load_config).runtime.stream
}

/// Backend-held delegation waiting settings (bounded `delegate_wait` + park/wake).
pub fn delegation_config() -> &'static runtime_config::DelegationConfig {
    &LOADED_CONFIG_CELL.get_or_init(load_config).runtime.delegation
}

/// Workspace Review reviewer deadlines (idle / absolute wall-clock / completion grace).
pub fn workspace_review_config() -> &'static runtime_config::WorkspaceReviewRuntimeConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .workspace_review
}

pub fn database_maintenance_config() -> &'static runtime_config::DatabaseMaintenanceConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .database_maintenance
}

pub fn reconciliation_config() -> &'static ReconciliationConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .reconciliation
}

pub fn git_runtime_config() -> &'static GitRuntimeConfig {
    &LOADED_CONFIG_CELL.get_or_init(load_config).runtime.git
}

pub fn scheduler_config() -> &'static SchedulerConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .scheduler
}

pub fn automations_config() -> &'static AutomationsRuntimeConfig {
    &LOADED_CONFIG_CELL.get_or_init(load_config).automations
}

pub fn supervisor_runtime_config() -> &'static SupervisorRuntimeConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .supervisor
}

pub fn limits_config() -> &'static LimitsConfig {
    &LOADED_CONFIG_CELL.get_or_init(load_config).runtime.limits
}

pub fn verification_config() -> &'static VerificationConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .verification
}

pub fn ui_feature_flags_config() -> &'static UiFeatureFlagsConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .ui_feature_flags
}

#[allow(dead_code)]
pub fn external_mcp_config() -> &'static ExternalMcpConfig {
    &LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .external_mcp
}

/// Returns the activity threshold (seconds) used by `get_child_session_status` to derive
/// `estimated_status`. Reads from `AllRuntimeConfig.child_session_activity_threshold_secs`,
/// defaulting to 10 if unset.
#[allow(dead_code)]
pub fn ideation_activity_threshold_secs() -> u64 {
    LOADED_CONFIG_CELL
        .get_or_init(load_config)
        .runtime
        .child_session_activity_threshold_secs
        .unwrap_or(10)
}

pub fn get_preapproved_tools(agent_name: &str) -> Option<String> {
    get_preapproved_tools_for_profile(agent_name, None)
}

pub fn get_preapproved_tools_for_profile(
    agent_name: &str,
    profile_name: Option<&str>,
) -> Option<String> {
    get_agent_config_for_profile(agent_name, profile_name).and_then(|c| {
        let mut tools: Vec<String> = Vec::new();
        let mcp_server = &claude_runtime_config().mcp_server_name;
        let lookup_name = super::canonical_short_agent_name(agent_name);
        let metadata = try_load_canonical_claude_metadata_for_profile(
            &canonical_agent_project_root(),
            lookup_name,
            profile_name,
        )
        .ok();
        let uses_external_transport = metadata
            .as_ref()
            .and_then(|metadata| metadata.mcp_transport.as_deref())
            == Some("external");
        let internal_server = internal_mcp_server_name(mcp_server);
        let internal_tools = metadata
            .as_ref()
            .map(|metadata| metadata.internal_mcp_tools.as_slice())
            .unwrap_or(&[]);

        if uses_external_transport {
            if let Some(metadata) = metadata.as_ref() {
                for t in &metadata.mcp_tools {
                    tools.push(format!("mcp__{}__{}", mcp_server, t));
                }
            }
            for t in internal_tools {
                tools.push(format!("mcp__{}__{}", internal_server, t));
            }
        } else {
            for t in &c.allowed_mcp_tools {
                tools.push(format!("mcp__{}__{}", mcp_server, t));
            }
        }

        // CLI tools the agent can use (--tools) are also pre-approved so they don't prompt.
        if !c.mcp_only {
            tools.extend(c.resolved_cli_tools.iter().cloned());
        }
        tools.extend(c.preapproved_cli_tools.iter().cloned());

        if !c.mcp_only {
            // Memory skills only for dedicated memory agents
            if lookup_name == "ralphx-memory-maintainer" || lookup_name == "ralphx-memory-capture" {
                for t in MEMORY_SKILLS {
                    tools.push((*t).to_string());
                }
            }
        }

        // Dedupe while preserving order (first occurrence wins)
        let mut seen = HashSet::new();
        tools.retain(|t| seen.insert(t.clone()));

        // Inject permission_request on the transport that actually exposes it:
        // external agents with an internal sidecar get the sidecar tool, non-external
        // agents get it on the primary server, and external agents without internal
        // tools inject nothing. This stays in lockstep with
        // `resolve_permission_prompt_tool` so the `--permission-prompt-tool` flag
        // never names a tool absent from this surface (the Claude CLI aborts before
        // any MCP call when it does).
        let permission_tool = internal_sidecar_permission_request_tool(
            uses_external_transport,
            !internal_tools.is_empty(),
            mcp_server,
        )
        .or_else(|| {
            (!uses_external_transport).then(|| format!("mcp__{mcp_server}__permission_request"))
        });
        if let Some(permission_tool) = permission_tool {
            if !seen.contains(&permission_tool) {
                tools.push(permission_tool);
            }
        }

        if tools.is_empty() {
            None
        } else {
            Some(tools.join(","))
        }
    })
}

/// Returns the fully-qualified `permission_request` MCP tool name an
/// external-transport agent exposes on its internal MCP sidecar, or `None` when the
/// agent's `permission_request` does not live on the sidecar (no external transport,
/// or external transport without internal sidecar tools).
///
/// This is the shared decision used by both [`get_preapproved_tools_for_profile`]
/// (which injects the tool into `--allowed-tools`) and [`resolve_permission_prompt_tool`]
/// (which feeds `--permission-prompt-tool`), so the flag always points at the same
/// sidecar server the surface exposes.
fn internal_sidecar_permission_request_tool(
    uses_external_transport: bool,
    has_internal_tools: bool,
    mcp_server: &str,
) -> Option<String> {
    (uses_external_transport && has_internal_tools).then(|| {
        format!(
            "mcp__{}__permission_request",
            internal_mcp_server_name(mcp_server)
        )
    })
}

/// Resolves the `--permission-prompt-tool` value for `agent_name` (under
/// `agent_profile`) so it matches the `permission_request` tool injected into that
/// agent's `--allowed-tools` surface by [`get_preapproved_tools_for_profile`].
///
/// Only overrides the configured `default` when `permission_request` lives on the
/// agent's internal MCP sidecar (external transport + internal tools). For every
/// other case it returns `default`, preserving an operator's configured
/// `claude.permission_prompt_tool` (including fully-qualified custom values). The
/// lookup is profile-aware so a profile overriding `mcp_transport` /
/// `internal_mcp_tools` resolves against the profile's actual MCP surface.
pub(crate) fn resolve_permission_prompt_tool(
    agent_name: Option<&str>,
    agent_profile: Option<&str>,
    default: &str,
) -> String {
    let Some(agent_name) = agent_name else {
        return default.to_string();
    };
    let mcp_server = &claude_runtime_config().mcp_server_name;
    let lookup_name = super::canonical_short_agent_name(agent_name);
    let metadata = try_load_canonical_claude_metadata_for_profile(
        &canonical_agent_project_root(),
        lookup_name,
        agent_profile,
    )
    .ok();
    let uses_external_transport = metadata
        .as_ref()
        .and_then(|metadata| metadata.mcp_transport.as_deref())
        == Some("external");
    let has_internal_tools = metadata
        .as_ref()
        .map(|metadata| !metadata.internal_mcp_tools.is_empty())
        .unwrap_or(false);
    internal_sidecar_permission_request_tool(
        uses_external_transport,
        has_internal_tools,
        mcp_server,
    )
    .unwrap_or_else(|| default.to_string())
}

#[cfg(test)]
mod tests;
