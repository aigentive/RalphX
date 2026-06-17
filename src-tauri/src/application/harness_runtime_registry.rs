use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::{collections::HashMap, time::Instant};

use crate::application::reconciliation::verification_reconciliation::VerificationReconciliationConfig;
use crate::domain::agents::{standard_harness_registry, AgentHarnessKind, DEFAULT_AGENT_HARNESS};
use crate::infrastructure::agents::claude::{
    agent_harness_defaults_config, clear_claude_cli_capability_cache, execution_defaults_config,
    external_mcp_config, find_claude_cli, node_utils, probe_claude_cli_cached,
    reconciliation_config, register_mcp_server, resolve_plugin_dir, scheduler_config,
    ui_feature_flags_config, validate_external_mcp_config, verification_config,
    AgentHarnessDefaultsConfig, ExecutionDefaultsConfig, ExternalMcpConfig, SchedulerConfig,
    SpecialistEntry, UiFeatureFlagsConfig, VerificationConfig,
};
use crate::infrastructure::agents::{
    find_codex_cli, probe_codex_cli, resolve_codex_cli, CodexCliCapabilities, ResolvedCodexCli,
};
use which::which;

pub(crate) type HarnessProbeFn = fn() -> HarnessRuntimeProbe;
pub(crate) type ChatHarnessCliResolver = fn(&Path) -> Result<ResolvedChatHarnessCli, String>;
pub(crate) type StartupHarnessIntegrationResolver =
    fn() -> Result<Option<ResolvedHarnessStartupIntegration>, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HarnessRuntimeProbe {
    pub binary_path: Option<String>,
    pub binary_found: bool,
    pub probe_succeeded: bool,
    pub available: bool,
    pub missing_core_exec_features: Vec<String>,
    pub cli_version: Option<String>,
    pub supported_model_aliases: Option<Vec<String>>,
    pub supported_efforts: Option<Vec<String>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedChatHarnessCli {
    Claude {
        cli_path: PathBuf,
    },
    Codex {
        cli_path: PathBuf,
        capabilities: CodexCliCapabilities,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedHarnessStartupIntegration {
    RegisterConfiguredMcpServer {
        harness: AgentHarnessKind,
        cli_path: PathBuf,
        plugin_dir: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DefaultChatServiceBootstrap {
    pub cli_path: PathBuf,
    pub plugin_dir: PathBuf,
    pub default_working_directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DefaultHarnessAgentBootstrap {
    pub working_directory: PathBuf,
    pub plugin_dir: PathBuf,
    pub agent_name: String,
    pub agent_role: String,
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DefaultExternalMcpBootstrap {
    pub config: ExternalMcpConfig,
    pub node_path: PathBuf,
    pub entry_path: PathBuf,
}

impl ResolvedHarnessStartupIntegration {
    pub(crate) fn harness(&self) -> AgentHarnessKind {
        match self {
            Self::RegisterConfiguredMcpServer { harness, .. } => *harness,
        }
    }

    pub(crate) fn description(&self) -> &'static str {
        match self {
            Self::RegisterConfiguredMcpServer { .. } => "configured MCP server registration",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct HarnessRuntimeAdapter {
    pub probe: HarnessProbeFn,
    pub resolve_chat_cli: ChatHarnessCliResolver,
    pub resolve_startup_integration: StartupHarnessIntegrationResolver,
}

fn probe_claude_harness() -> HarnessRuntimeProbe {
    match find_claude_cli() {
        Some(cli_path) => {
            let binary_path = Some(cli_path.to_string_lossy().into_owned());
            match probe_claude_cli_cached(&cli_path) {
                Ok(capabilities) => {
                    tracing::info!(
                        cli_path = %cli_path.display(),
                        version = ?capabilities.version,
                        supported_model_aliases = ?capabilities.supported_model_aliases,
                        supported_efforts = ?capabilities.supported_effort_labels(),
                        "Claude CLI capability probe completed"
                    );
                    HarnessRuntimeProbe {
                        binary_path,
                        binary_found: true,
                        probe_succeeded: true,
                        available: true,
                        missing_core_exec_features: Vec::new(),
                        cli_version: capabilities.version.clone(),
                        supported_model_aliases: Some(capabilities.supported_model_aliases.clone()),
                        supported_efforts: Some(capabilities.supported_effort_labels()),
                        error: None,
                    }
                }
                Err(error) => HarnessRuntimeProbe {
                    binary_path,
                    binary_found: true,
                    probe_succeeded: false,
                    available: true,
                    missing_core_exec_features: Vec::new(),
                    cli_version: None,
                    supported_model_aliases: None,
                    supported_efforts: None,
                    error: Some(error),
                },
            }
        }
        None => HarnessRuntimeProbe {
            binary_path: None,
            binary_found: false,
            probe_succeeded: false,
            available: false,
            missing_core_exec_features: Vec::new(),
            cli_version: None,
            supported_model_aliases: None,
            supported_efforts: None,
            error: Some("Claude CLI not found".to_string()),
        },
    }
}

fn probe_codex_harness() -> HarnessRuntimeProbe {
    match resolve_codex_cli_cached() {
        Ok(resolved) => {
            let binary_path = Some(resolved.path.to_string_lossy().into_owned());
            let capabilities = resolved.capabilities;
            let missing_core_exec_features = capabilities
                .missing_core_exec_features()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            let available = missing_core_exec_features.is_empty();
            let error = if available {
                None
            } else {
                Some(format!(
                    "Codex CLI is missing required capability: {}",
                    missing_core_exec_features.join(", ")
                ))
            };
            HarnessRuntimeProbe {
                binary_path,
                binary_found: true,
                probe_succeeded: true,
                available,
                missing_core_exec_features,
                cli_version: capabilities.version,
                supported_model_aliases: None,
                supported_efforts: None,
                error,
            }
        }
        Err(error) => match find_codex_cli() {
            Some(cli_path) => HarnessRuntimeProbe {
                binary_path: Some(cli_path.to_string_lossy().into_owned()),
                binary_found: true,
                probe_succeeded: false,
                available: false,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                error: Some(error),
            },
            None => HarnessRuntimeProbe {
                binary_path: None,
                binary_found: false,
                probe_succeeded: false,
                available: false,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                error: Some(error),
            },
        },
    }
}

fn resolve_claude_chat_harness_cli(
    claude_cli_path: &Path,
) -> Result<ResolvedChatHarnessCli, String> {
    if !claude_cli_path.exists() && which(claude_cli_path).is_err() {
        return Err(format!(
            "Claude CLI not found at {}",
            claude_cli_path.display()
        ));
    }

    Ok(ResolvedChatHarnessCli::Claude {
        cli_path: claude_cli_path.to_path_buf(),
    })
}

fn resolve_codex_chat_harness_cli(codex_cli_path: &Path) -> Result<ResolvedChatHarnessCli, String> {
    if codex_cli_path == Path::new(default_chat_service_cli_name(AgentHarnessKind::Codex)) {
        return codex_chat_harness_cli_from_resolve_result(resolve_codex_cli_cached());
    }

    if !codex_cli_path.exists() && which(codex_cli_path).is_err() {
        return Err(format!(
            "Codex CLI not found at {}",
            codex_cli_path.display()
        ));
    }

    let capabilities = probe_codex_cli_cached(codex_cli_path)?;
    Ok(ResolvedChatHarnessCli::Codex {
        cli_path: codex_cli_path.to_path_buf(),
        capabilities,
    })
}

fn codex_chat_harness_cli_from_resolve_result(
    resolved: Result<ResolvedCodexCli, String>,
) -> Result<ResolvedChatHarnessCli, String> {
    let resolved = resolved?;
    Ok(ResolvedChatHarnessCli::Codex {
        cli_path: resolved.path,
        capabilities: resolved.capabilities,
    })
}

static RESOLVED_CODEX_CLI_CACHE: OnceLock<Mutex<Option<Result<ResolvedCodexCli, String>>>> =
    OnceLock::new();
static CODEX_CLI_CAPABILITY_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, Result<CodexCliCapabilities, String>>>,
> = OnceLock::new();
static HARNESS_RUNTIME_PROBE_CACHE: OnceLock<
    Mutex<HashMap<AgentHarnessKind, HarnessRuntimeProbe>>,
> = OnceLock::new();
static HARNESS_RUNTIME_PROBE_IN_FLIGHT: OnceLock<
    Mutex<HashMap<AgentHarnessKind, Arc<HarnessRuntimeProbeInFlight>>>,
> = OnceLock::new();
static CHAT_HARNESS_CLI_CACHE: OnceLock<
    Mutex<HashMap<(AgentHarnessKind, PathBuf), Result<ResolvedChatHarnessCli, String>>>,
> = OnceLock::new();

#[derive(Debug)]
struct HarnessRuntimeProbeInFlight {
    result: Mutex<Option<HarnessRuntimeProbe>>,
    completed: Condvar,
}

impl HarnessRuntimeProbeInFlight {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            completed: Condvar::new(),
        }
    }
}

fn resolve_codex_cli_cached() -> Result<ResolvedCodexCli, String> {
    let cache = RESOLVED_CODEX_CLI_CACHE.get_or_init(|| Mutex::new(None));
    let mut cached = cache.lock().unwrap();
    if let Some(result) = cached.as_ref() {
        tracing::debug!(
            success = result.is_ok(),
            cli_path = ?result.as_ref().ok().map(|resolved| resolved.path.display().to_string()),
            "Codex CLI resolved from app-session cache"
        );
        return result.clone();
    }

    let started = Instant::now();
    let result = resolve_codex_cli();
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        success = result.is_ok(),
        cli_path = ?result.as_ref().ok().map(|resolved| resolved.path.display().to_string()),
        error = ?result.as_ref().err(),
        "Codex CLI capability probe completed"
    );
    *cached = Some(result.clone());
    result
}

fn probe_codex_cli_cached(cli_path: &Path) -> Result<CodexCliCapabilities, String> {
    if let Some(Ok(resolved)) = RESOLVED_CODEX_CLI_CACHE
        .get()
        .and_then(|cache| cache.lock().ok().and_then(|cached| cached.clone()))
    {
        if resolved.path == cli_path {
            tracing::debug!(
                cli_path = %cli_path.display(),
                "Codex CLI capabilities reused from resolved cache"
            );
            return Ok(resolved.capabilities);
        }
    }

    let cache = CODEX_CLI_CAPABILITY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cached = cache.lock().unwrap();
    if let Some(result) = cached.get(cli_path) {
        tracing::debug!(
            cli_path = %cli_path.display(),
            success = result.is_ok(),
            "Codex CLI capabilities reused from path cache"
        );
        return result.clone();
    }

    let started = Instant::now();
    let result = probe_codex_cli(cli_path);
    tracing::info!(
        cli_path = %cli_path.display(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        success = result.is_ok(),
        error = ?result.as_ref().err(),
        "Codex CLI path capability probe completed"
    );
    cached.insert(cli_path.to_path_buf(), result.clone());
    result
}

fn resolve_claude_startup_integration() -> Result<Option<ResolvedHarnessStartupIntegration>, String>
{
    let cli_path = find_claude_cli().ok_or_else(|| "Claude CLI not found".to_string())?;
    let plugin_dir = crate::infrastructure::agents::claude::find_plugin_dir()
        .ok_or_else(|| "Claude plugin directory not found".to_string())?;
    Ok(Some(
        ResolvedHarnessStartupIntegration::RegisterConfiguredMcpServer {
            harness: AgentHarnessKind::Claude,
            cli_path,
            plugin_dir,
        },
    ))
}

fn resolve_codex_startup_integration() -> Result<Option<ResolvedHarnessStartupIntegration>, String>
{
    Ok(None)
}

pub(crate) fn standard_harness_runtime_adapters() -> HashMap<AgentHarnessKind, HarnessRuntimeAdapter>
{
    standard_harness_registry(|harness| match harness {
        AgentHarnessKind::Claude => HarnessRuntimeAdapter {
            probe: probe_claude_harness,
            resolve_chat_cli: resolve_claude_chat_harness_cli,
            resolve_startup_integration: resolve_claude_startup_integration,
        },
        AgentHarnessKind::Codex => HarnessRuntimeAdapter {
            probe: probe_codex_harness,
            resolve_chat_cli: resolve_codex_chat_harness_cli,
            resolve_startup_integration: resolve_codex_startup_integration,
        },
    })
}

#[cfg(test)]
pub(crate) fn standard_harness_probe_registry() -> HashMap<AgentHarnessKind, HarnessProbeFn> {
    standard_harness_runtime_adapters()
        .into_iter()
        .map(|(harness, adapter)| (harness, adapter.probe))
        .collect()
}

#[cfg(test)]
pub(crate) fn standard_chat_harness_cli_resolvers(
) -> HashMap<AgentHarnessKind, ChatHarnessCliResolver> {
    standard_harness_runtime_adapters()
        .into_iter()
        .map(|(harness, adapter)| (harness, adapter.resolve_chat_cli))
        .collect()
}

fn probe_harness_uncached(harness: AgentHarnessKind) -> HarnessRuntimeProbe {
    let adapters = standard_harness_runtime_adapters();
    adapters
        .get(&harness)
        .map(|adapter| (adapter.probe)())
        .unwrap_or(HarnessRuntimeProbe {
            binary_path: None,
            binary_found: false,
            probe_succeeded: false,
            available: false,
            missing_core_exec_features: Vec::new(),
            cli_version: None,
            supported_model_aliases: None,
            supported_efforts: None,
            error: Some(format!("No harness probe registered for {}", harness)),
        })
}

pub(crate) fn probe_harness(harness: AgentHarnessKind) -> HarnessRuntimeProbe {
    let cache = HARNESS_RUNTIME_PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let cached = cache.lock().unwrap();
        if let Some(probe) = cached.get(&harness) {
            tracing::debug!(
                harness = %harness,
                available = probe.available,
                binary_path = ?probe.binary_path,
                "Harness runtime probe reused from app-session cache"
            );
            return probe.clone();
        }
    }

    let in_flight = HARNESS_RUNTIME_PROBE_IN_FLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
    let (is_owner, probe_in_flight) = {
        let mut probes = in_flight.lock().unwrap();
        if let Some(probe) = probes.get(&harness) {
            (false, Arc::clone(probe))
        } else {
            let probe = Arc::new(HarnessRuntimeProbeInFlight::new());
            probes.insert(harness, Arc::clone(&probe));
            (true, probe)
        }
    };

    if !is_owner {
        return wait_for_in_flight_harness_probe(harness, probe_in_flight);
    }

    {
        let cached = cache.lock().unwrap();
        if let Some(probe) = cached.get(&harness) {
            complete_in_flight_harness_probe(harness, &probe_in_flight, probe.clone());
            return probe.clone();
        }
    }

    let started = Instant::now();
    let probe = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        probe_harness_uncached(harness)
    })) {
        Ok(probe) => probe,
        Err(_) => {
            tracing::warn!(
                harness = %harness,
                "Harness runtime probe panicked"
            );
            HarnessRuntimeProbe {
                binary_path: None,
                binary_found: false,
                probe_succeeded: false,
                available: false,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                error: Some("Harness runtime probe panicked".to_string()),
            }
        }
    };
    tracing::info!(
        harness = %harness,
        available = probe.available,
        binary_found = probe.binary_found,
        binary_path = ?probe.binary_path,
        missing_core_exec_features = ?probe.missing_core_exec_features,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Harness runtime probe completed"
    );

    let mut cached = cache.lock().unwrap();
    let probe = cached
        .entry(harness)
        .or_insert_with(|| probe.clone())
        .clone();
    complete_in_flight_harness_probe(harness, &probe_in_flight, probe.clone());
    probe
}

fn wait_for_in_flight_harness_probe(
    harness: AgentHarnessKind,
    probe_in_flight: Arc<HarnessRuntimeProbeInFlight>,
) -> HarnessRuntimeProbe {
    let started = Instant::now();
    let mut result = probe_in_flight.result.lock().unwrap();
    loop {
        if let Some(probe) = result.as_ref() {
            tracing::debug!(
                harness = %harness,
                available = probe.available,
                binary_path = ?probe.binary_path,
                wait_ms = started.elapsed().as_millis() as u64,
                "Harness runtime probe reused from in-flight app-session probe"
            );
            return probe.clone();
        }
        result = probe_in_flight.completed.wait(result).unwrap();
    }
}

fn complete_in_flight_harness_probe(
    harness: AgentHarnessKind,
    probe_in_flight: &Arc<HarnessRuntimeProbeInFlight>,
    probe: HarnessRuntimeProbe,
) {
    {
        let mut result = probe_in_flight.result.lock().unwrap();
        *result = Some(probe);
    }
    probe_in_flight.completed.notify_all();

    if let Some(in_flight) = HARNESS_RUNTIME_PROBE_IN_FLIGHT.get() {
        let mut probes = in_flight.lock().unwrap();
        if probes
            .get(&harness)
            .is_some_and(|current| Arc::ptr_eq(current, probe_in_flight))
        {
            probes.remove(&harness);
        }
    }
}

pub(crate) fn refresh_harness_runtime_probe(harness: AgentHarnessKind) -> HarnessRuntimeProbe {
    clear_harness_runtime_caches_for_harness(harness);
    probe_harness(harness)
}

pub(crate) fn clear_harness_runtime_caches_for_harness(harness: AgentHarnessKind) {
    if let Some(cache) = HARNESS_RUNTIME_PROBE_CACHE.get() {
        cache.lock().unwrap().remove(&harness);
    }
    if let Some(cache) = CHAT_HARNESS_CLI_CACHE.get() {
        cache
            .lock()
            .unwrap()
            .retain(|(cached_harness, _), _| *cached_harness != harness);
    }
    match harness {
        AgentHarnessKind::Claude => {
            clear_claude_cli_capability_cache();
        }
        AgentHarnessKind::Codex => {
            if let Some(cache) = RESOLVED_CODEX_CLI_CACHE.get() {
                *cache.lock().unwrap() = None;
            }
            if let Some(cache) = CODEX_CLI_CAPABILITY_CACHE.get() {
                cache.lock().unwrap().clear();
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn clear_harness_runtime_caches_for_tests(harness: AgentHarnessKind) {
    clear_harness_runtime_caches_for_harness(harness);
}

pub(crate) fn probe_default_harness() -> HarnessRuntimeProbe {
    probe_harness(DEFAULT_AGENT_HARNESS)
}

pub(crate) fn default_harness_runtime_available() -> bool {
    probe_default_harness().available
}

fn default_repo_root_working_directory_from(cwd: PathBuf) -> PathBuf {
    if cwd.file_name().is_some_and(|name| name == "src-tauri") {
        cwd.parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or(cwd)
    } else {
        cwd
    }
}

pub(crate) fn default_repo_root_working_directory() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    default_repo_root_working_directory_from(cwd)
}

pub(crate) fn resolve_default_harness_plugin_dir(working_directory: &Path) -> PathBuf {
    resolve_plugin_dir(working_directory)
}

pub(crate) fn resolve_harness_plugin_dir(
    harness: AgentHarnessKind,
    working_directory: &Path,
) -> PathBuf {
    match harness {
        AgentHarnessKind::Claude | AgentHarnessKind::Codex => {
            resolve_default_harness_plugin_dir(working_directory)
        }
    }
}

fn default_chat_service_cli_name(harness: AgentHarnessKind) -> &'static str {
    match harness {
        AgentHarnessKind::Claude => "claude",
        AgentHarnessKind::Codex => "codex",
    }
}

fn resolve_chat_service_cli_path(harness: AgentHarnessKind) -> PathBuf {
    probe_harness(harness)
        .binary_path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_chat_service_cli_name(harness)))
}

#[cfg(test)]
fn codex_chat_service_cli_path_from_resolve_result(
    resolved: Result<ResolvedCodexCli, String>,
) -> PathBuf {
    resolved
        .map(|resolved| resolved.path)
        .unwrap_or_else(|_| PathBuf::from(default_chat_service_cli_name(AgentHarnessKind::Codex)))
}

pub(crate) fn resolve_chat_service_bootstrap(
    harness: AgentHarnessKind,
) -> DefaultChatServiceBootstrap {
    let default_working_directory = default_repo_root_working_directory();
    DefaultChatServiceBootstrap {
        cli_path: resolve_chat_service_cli_path(harness),
        plugin_dir: resolve_default_harness_plugin_dir(&default_working_directory),
        default_working_directory,
    }
}

pub(crate) fn resolve_default_chat_service_bootstrap() -> DefaultChatServiceBootstrap {
    resolve_chat_service_bootstrap(DEFAULT_AGENT_HARNESS)
}

pub(crate) fn resolve_harness_agent_bootstrap(
    harness: AgentHarnessKind,
    agent_name: &'static str,
    working_directory: PathBuf,
) -> DefaultHarnessAgentBootstrap {
    let plugin_dir = resolve_harness_plugin_dir(harness, &working_directory);
    let agent_role = crate::infrastructure::agents::claude::mcp_agent_type(agent_name).to_string();
    let mut env = HashMap::new();
    env.insert("RALPHX_AGENT_TYPE".to_string(), agent_role.clone());

    DefaultHarnessAgentBootstrap {
        working_directory,
        plugin_dir,
        agent_name: agent_name.to_string(),
        agent_role,
        env,
    }
}

pub(crate) fn resolve_default_external_mcp_bootstrap(
) -> Result<Option<DefaultExternalMcpBootstrap>, String> {
    let config = default_external_mcp_config();
    if !config.enabled {
        return Ok(None);
    }

    validate_external_mcp_config(&config)?;

    let entry_path = find_claude_external_mcp_entry()
        .ok_or_else(|| "Plugin dir not found, cannot start external MCP".to_string())?;

    Ok(Some(DefaultExternalMcpBootstrap {
        config,
        node_path: node_utils::find_node_binary(),
        entry_path,
    }))
}

pub(crate) fn default_external_mcp_config() -> ExternalMcpConfig {
    external_mcp_config().clone()
}

pub(crate) fn default_external_mcp_config_path() -> PathBuf {
    crate::infrastructure::agents::claude::external_mcp_config_path()
}

pub(crate) fn default_external_mcp_port() -> u16 {
    default_external_mcp_config().port
}

pub(crate) fn default_external_mcp_human_wait_timeout_secs() -> u64 {
    default_external_mcp_config().human_wait_timeout_secs
}

pub(crate) fn default_external_mcp_message_queue_cap() -> usize {
    default_external_mcp_config().external_message_queue_cap as usize
}

pub(crate) fn default_external_session_similarity_threshold() -> f64 {
    default_external_mcp_config().external_session_similarity_threshold
}

pub(crate) fn default_verification_config() -> VerificationConfig {
    verification_config().clone()
}

pub(crate) fn default_verification_auto_verify_enabled() -> bool {
    verification_config().auto_verify
}

pub(crate) fn default_verification_max_rounds() -> u32 {
    verification_config().max_rounds
}

pub(crate) fn default_verification_specialists() -> Vec<SpecialistEntry> {
    verification_config().specialists.clone()
}

pub(crate) fn default_ui_feature_flags() -> UiFeatureFlagsConfig {
    ui_feature_flags_config().clone()
}

pub(crate) fn default_verification_reconciliation_config() -> VerificationReconciliationConfig {
    let verification = default_verification_config();
    let external_mcp = default_external_mcp_config();
    VerificationReconciliationConfig {
        stale_after_secs: verification.reconciliation_stale_after_secs,
        auto_verify_stale_secs: verification.auto_verify_stale_secs,
        interval_secs: verification.reconciliation_interval_secs,
        external_session_stale_secs: external_mcp.external_session_stale_secs,
        external_session_startup_grace_secs: external_mcp.external_session_startup_grace_secs,
    }
}

pub(crate) fn default_execution_settings_config() -> ExecutionDefaultsConfig {
    execution_defaults_config().clone()
}

pub(crate) fn default_agent_harness_settings_config() -> AgentHarnessDefaultsConfig {
    agent_harness_defaults_config().clone()
}

pub(crate) fn default_scheduler_runtime_config() -> SchedulerConfig {
    scheduler_config().clone()
}

pub(crate) fn default_scheduler_ready_settle_ms() -> u64 {
    scheduler_config().ready_settle_ms
}

pub(crate) fn default_scheduler_merge_settle_ms() -> u64 {
    scheduler_config().merge_settle_ms
}

pub(crate) fn default_reconciliation_merger_timeout_secs() -> u64 {
    reconciliation_config().merger_timeout_secs
}

pub(crate) fn default_reconciliation_merging_max_retries() -> u32 {
    reconciliation_config().merging_max_retries as u32
}

pub(crate) fn default_reconciliation_merge_registry_grace_period_secs() -> u64 {
    reconciliation_config().merge_registry_grace_period_secs
}

pub(crate) fn default_reconciliation_attempt_merge_deadline_secs() -> u64 {
    reconciliation_config().attempt_merge_deadline_secs
}

pub(crate) fn default_reconciliation_validation_revert_max_count() -> u32 {
    reconciliation_config().validation_revert_max_count as u32
}

pub(crate) fn default_reconciliation_validation_failure_circuit_breaker_count() -> u32 {
    reconciliation_config().validation_failure_circuit_breaker_count as u32
}

pub(crate) fn default_reconciliation_validation_retry_min_cooldown_secs() -> u64 {
    reconciliation_config().validation_retry_min_cooldown_secs
}

pub(crate) fn default_reconciliation_merge_starvation_guard_secs() -> u64 {
    reconciliation_config().merge_starvation_guard_secs
}

pub(crate) fn default_reconciliation_merge_circuit_breaker_threshold() -> usize {
    reconciliation_config().merge_circuit_breaker_threshold as usize
}

pub(crate) fn default_reconciliation_merge_circuit_breaker_window() -> usize {
    reconciliation_config().merge_circuit_breaker_window as usize
}

pub(crate) fn default_reconciliation_merge_incomplete_max_retries() -> u32 {
    reconciliation_config().merge_incomplete_max_retries as u32
}

pub(crate) fn default_reconciliation_merge_conflict_max_retries() -> u32 {
    reconciliation_config().merge_conflict_max_retries as u32
}

pub(crate) fn default_reconciliation_merge_incomplete_retry_base_secs() -> u64 {
    reconciliation_config().merge_incomplete_retry_base_secs
}

pub(crate) fn default_reconciliation_merge_incomplete_retry_max_secs() -> u64 {
    reconciliation_config().merge_incomplete_retry_max_secs
}

pub(crate) fn default_reconciliation_merge_conflict_retry_base_secs() -> u64 {
    reconciliation_config().merge_conflict_retry_base_secs
}

pub(crate) fn default_reconciliation_merge_conflict_retry_max_secs() -> u64 {
    reconciliation_config().merge_conflict_retry_max_secs
}

pub(crate) fn default_reconciliation_validation_deadline_secs() -> u64 {
    reconciliation_config().validation_deadline_secs
}

pub(crate) fn default_reconciliation_execution_failed_max_retries() -> u32 {
    reconciliation_config().execution_failed_max_retries as u32
}

pub(crate) fn default_reconciliation_recovery_staleness_secs() -> u64 {
    reconciliation_config().recovery_staleness_secs
}

pub(crate) fn default_reconciliation_git_isolation_max_retries() -> u32 {
    reconciliation_config().git_isolation_max_retries as u32
}

pub(crate) fn default_reconciliation_executing_max_wall_clock_minutes() -> u64 {
    reconciliation_config().executing_max_wall_clock_minutes
}

pub(crate) fn default_reconciliation_executing_max_retries() -> u32 {
    reconciliation_config().executing_max_retries as u32
}

pub(crate) fn default_reconciliation_reviewing_max_wall_clock_minutes() -> u64 {
    reconciliation_config().reviewing_max_wall_clock_minutes
}

pub(crate) fn default_reconciliation_reviewing_max_retries() -> u32 {
    reconciliation_config().reviewing_max_retries as u32
}

pub(crate) fn default_reconciliation_qa_max_wall_clock_minutes() -> u64 {
    reconciliation_config().qa_max_wall_clock_minutes
}

pub(crate) fn default_reconciliation_qa_stale_minutes() -> u64 {
    reconciliation_config().qa_stale_minutes
}

pub(crate) fn default_reconciliation_qa_max_retries() -> u32 {
    reconciliation_config().qa_max_retries as u32
}

pub(crate) fn default_reconciliation_pending_merge_stale_minutes() -> u64 {
    reconciliation_config().pending_merge_stale_minutes
}

pub(crate) fn default_reconciliation_merge_watcher_grace_secs() -> u64 {
    reconciliation_config().merge_watcher_grace_secs
}

pub(crate) fn default_reconciliation_merge_watcher_poll_secs() -> u64 {
    reconciliation_config().merge_watcher_poll_secs
}

pub(crate) fn default_reconciliation_execution_failed_retry_base_secs() -> u64 {
    reconciliation_config().execution_failed_retry_base_secs
}

pub(crate) fn default_reconciliation_execution_failed_retry_max_secs() -> u64 {
    reconciliation_config().execution_failed_retry_max_secs
}

pub(crate) fn default_reconciliation_git_isolation_retry_base_secs() -> u64 {
    reconciliation_config().git_isolation_retry_base_secs
}

fn find_claude_external_mcp_entry() -> Option<PathBuf> {
    crate::infrastructure::agents::claude::find_plugin_dir()
        .map(|plugin_dir| external_mcp_entry_for_plugin_dir(&plugin_dir))
}

fn external_mcp_entry_for_plugin_dir(plugin_dir: &Path) -> PathBuf {
    plugin_dir.join("ralphx-external-mcp/build/index.js")
}

pub(crate) fn probe_supported_harnesses() -> HashMap<AgentHarnessKind, HarnessRuntimeProbe> {
    probe_standard_harnesses_with(probe_harness, "probe")
}

pub(crate) fn refresh_supported_harnesses() -> HashMap<AgentHarnessKind, HarnessRuntimeProbe> {
    probe_standard_harnesses_with(refresh_harness_runtime_probe, "refresh")
}

fn probe_standard_harnesses_with(
    probe_fn: fn(AgentHarnessKind) -> HarnessRuntimeProbe,
    operation: &'static str,
) -> HashMap<AgentHarnessKind, HarnessRuntimeProbe> {
    let started = Instant::now();
    let harnesses = standard_harness_runtime_adapters()
        .into_keys()
        .collect::<Vec<_>>();
    let mut probes = HashMap::new();

    std::thread::scope(|scope| {
        let handles = harnesses
            .into_iter()
            .map(|harness| (harness, scope.spawn(move || probe_fn(harness))))
            .collect::<Vec<_>>();

        for (harness, handle) in handles {
            match handle.join() {
                Ok(probe) => {
                    probes.insert(harness, probe);
                }
                Err(_) => {
                    tracing::warn!(
                        harness = %harness,
                        "Harness runtime probe worker panicked"
                    );
                }
            }
        }
    });

    tracing::info!(
        harnesses = probes.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        operation,
        "Harness runtime batch completed"
    );
    probes
}

pub(crate) fn probe_codex_harness_with_capabilities(
) -> (HarnessRuntimeProbe, Option<CodexCliCapabilities>) {
    match resolve_codex_cli_cached() {
        Ok(resolved) => {
            let capabilities = resolved.capabilities;
            let missing_core_exec_features = capabilities
                .missing_core_exec_features()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            let available = missing_core_exec_features.is_empty();
            let error = if available {
                None
            } else {
                Some(format!(
                    "Codex CLI is missing required capability: {}",
                    missing_core_exec_features.join(", ")
                ))
            };
            (
                HarnessRuntimeProbe {
                    binary_path: Some(resolved.path.to_string_lossy().into_owned()),
                    binary_found: true,
                    probe_succeeded: true,
                    available,
                    missing_core_exec_features,
                    cli_version: capabilities.version.clone(),
                    supported_model_aliases: None,
                    supported_efforts: None,
                    error,
                },
                Some(capabilities),
            )
        }
        Err(error) => {
            let probe = match find_codex_cli() {
                Some(cli_path) => HarnessRuntimeProbe {
                    binary_path: Some(cli_path.to_string_lossy().into_owned()),
                    binary_found: true,
                    probe_succeeded: false,
                    available: false,
                    missing_core_exec_features: Vec::new(),
                    cli_version: None,
                    supported_model_aliases: None,
                    supported_efforts: None,
                    error: Some(error),
                },
                None => HarnessRuntimeProbe {
                    binary_path: None,
                    binary_found: false,
                    probe_succeeded: false,
                    available: false,
                    missing_core_exec_features: Vec::new(),
                    cli_version: None,
                    supported_model_aliases: None,
                    supported_efforts: None,
                    error: Some(error),
                },
            };
            (probe, None)
        }
    }
}

pub(crate) fn resolve_chat_harness_cli(
    harness: AgentHarnessKind,
    claude_cli_path: &Path,
) -> Result<ResolvedChatHarnessCli, String> {
    let cache_key = (harness, claude_cli_path.to_path_buf());
    let cache = CHAT_HARNESS_CLI_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cached = cache.lock().unwrap();
    if let Some(result) = cached.get(&cache_key) {
        tracing::debug!(
            harness = %harness,
            cli_path = %claude_cli_path.display(),
            success = result.is_ok(),
            "Chat harness CLI resolution reused from app-session cache"
        );
        return result.clone();
    }

    let adapters = standard_harness_runtime_adapters();
    let adapter = adapters
        .get(&harness)
        .copied()
        .ok_or_else(|| format!("No chat harness CLI resolver registered for {}", harness))?;
    let started = Instant::now();
    let result = (adapter.resolve_chat_cli)(claude_cli_path);
    tracing::info!(
        harness = %harness,
        cli_path = %claude_cli_path.display(),
        success = result.is_ok(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        error = ?result.as_ref().err(),
        "Chat harness CLI resolution completed"
    );
    cached.insert(cache_key, result.clone());
    result
}

pub(crate) fn resolve_startup_harness_integration(
    harness: AgentHarnessKind,
) -> Result<Option<ResolvedHarnessStartupIntegration>, String> {
    let adapters = standard_harness_runtime_adapters();
    let adapter = adapters
        .get(&harness)
        .copied()
        .ok_or_else(|| format!("No startup harness integration registered for {}", harness))?;
    (adapter.resolve_startup_integration)()
}

pub(crate) async fn run_startup_harness_integration(
    integration: ResolvedHarnessStartupIntegration,
) -> Result<(), String> {
    match integration {
        ResolvedHarnessStartupIntegration::RegisterConfiguredMcpServer {
            cli_path,
            plugin_dir,
            ..
        } => register_mcp_server(&cli_path, &plugin_dir).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use tempfile::TempDir;

    fn plugin_override_lock() -> &'static std::sync::Mutex<()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_os(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }

        fn unset(key: &'static str) -> Self {
            let original = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[cfg(unix)]
    fn write_fake_executable(path: &Path, body: &str) {
        std::fs::write(path, body).expect("write fake executable");
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("fake executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("mark fake executable");
    }

    fn make_runtime_plugin_layout() -> (TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let plugin_dir = root.join("plugins/app");
        let generated_dir = root.join("generated/claude-plugin");

        std::fs::create_dir_all(plugin_dir.join("agents")).expect("create agents dir");
        std::fs::write(
            plugin_dir.join("agents/session-namer.md"),
            "# Session Namer\n",
        )
        .expect("write session namer prompt");
        std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build"))
            .expect("create mcp build dir");
        std::fs::create_dir_all(
            plugin_dir.join("ralphx-mcp-server/node_modules/@modelcontextprotocol/sdk"),
        )
        .expect("create mcp sdk marker dir");
        std::fs::write(
            plugin_dir.join("ralphx-mcp-server/build/index.js"),
            "// fake mcp runtime\n",
        )
        .expect("write mcp runtime entry");
        std::fs::write(
            plugin_dir
                .join("ralphx-mcp-server/node_modules/@modelcontextprotocol/sdk/package.json"),
            "{}\n",
        )
        .expect("write mcp runtime marker");

        (temp, plugin_dir, generated_dir)
    }

    fn test_codex_capabilities() -> CodexCliCapabilities {
        CodexCliCapabilities {
            version: Some("0.124.0".to_string()),
            supports_exec_subcommand: true,
            supports_json_output: true,
            supports_model_flag: true,
            supports_config_override: true,
            supports_sandbox_flag: true,
            supports_add_dir: true,
            supports_search_flag: true,
            supports_resume_subcommand: true,
            supports_mcp_subcommand: true,
        }
    }

    fn test_resolved_codex_cli(path: &str) -> ResolvedCodexCli {
        ResolvedCodexCli {
            path: PathBuf::from(path),
            capabilities: test_codex_capabilities(),
        }
    }

    #[test]
    fn resolve_startup_harness_integration_returns_none_for_codex() {
        let integration = resolve_startup_harness_integration(AgentHarnessKind::Codex).unwrap();
        assert!(integration.is_none());
    }

    #[test]
    fn default_chat_service_cli_name_matches_standard_harnesses() {
        assert_eq!(
            default_chat_service_cli_name(AgentHarnessKind::Claude),
            "claude"
        );
        assert_eq!(
            default_chat_service_cli_name(AgentHarnessKind::Codex),
            "codex"
        );
    }

    #[test]
    fn resolve_default_chat_service_bootstrap_uses_default_harness() {
        let _lock = plugin_override_lock().lock().expect("lock harness caches");
        let (_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
        let _runtime_guard =
            crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
                plugin_dir,
                generated_dir,
            );
        if let Some(cache) = HARNESS_RUNTIME_PROBE_CACHE.get() {
            cache.lock().unwrap().clear();
        }
        if let Some(cache) = CHAT_HARNESS_CLI_CACHE.get() {
            cache.lock().unwrap().clear();
        }

        assert_eq!(
            resolve_default_chat_service_bootstrap(),
            resolve_chat_service_bootstrap(DEFAULT_AGENT_HARNESS)
        );
    }

    #[test]
    fn codex_chat_harness_cli_maps_compatible_default_candidate() {
        let resolved = codex_chat_harness_cli_from_resolve_result(Ok(test_resolved_codex_cli(
            "/opt/homebrew/bin/codex",
        )))
        .unwrap();

        match resolved {
            ResolvedChatHarnessCli::Codex {
                cli_path,
                capabilities,
            } => {
                assert_eq!(cli_path, PathBuf::from("/opt/homebrew/bin/codex"));
                assert!(capabilities.has_core_exec_support());
            }
            ResolvedChatHarnessCli::Claude { .. } => panic!("expected Codex CLI resolution"),
        }
    }

    #[test]
    fn chat_harness_cli_resolution_uses_app_session_caches() {
        let _lock = plugin_override_lock().lock().expect("lock harness caches");
        if let Some(cache) = CODEX_CLI_CAPABILITY_CACHE.get() {
            cache.lock().unwrap().clear();
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let claude_cli = temp.path().join("claude");
        let codex_cli = temp.path().join("codex");
        std::fs::write(&claude_cli, "#!/bin/sh\n").expect("write fake claude");
        std::fs::write(&codex_cli, "#!/bin/sh\n").expect("write fake codex");

        let claude = resolve_claude_chat_harness_cli(&claude_cli)
            .expect("fake existing Claude CLI path should resolve");
        match claude {
            ResolvedChatHarnessCli::Claude { cli_path } => assert_eq!(cli_path, claude_cli),
            ResolvedChatHarnessCli::Codex { .. } => panic!("expected Claude CLI"),
        }

        CODEX_CLI_CAPABILITY_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(codex_cli.clone(), Ok(test_codex_capabilities()));
        let codex = resolve_codex_chat_harness_cli(&codex_cli)
            .expect("cached fake Codex CLI path should resolve");
        match codex {
            ResolvedChatHarnessCli::Codex {
                cli_path,
                capabilities,
            } => {
                assert_eq!(cli_path, codex_cli);
                assert!(capabilities.has_core_exec_support());
            }
            ResolvedChatHarnessCli::Claude { .. } => panic!("expected Codex CLI"),
        }

        let missing = resolve_codex_chat_harness_cli(&temp.path().join("missing-codex"))
            .expect_err("missing explicit Codex path should fail");
        assert!(missing.contains("Codex CLI not found"));

        CODEX_CLI_CAPABILITY_CACHE
            .get()
            .expect("capability cache should exist")
            .lock()
            .unwrap()
            .clear();
    }

    #[test]
    fn codex_resolution_cache_is_reused_for_probe_and_capabilities() {
        let _lock = plugin_override_lock().lock().expect("lock harness caches");
        let resolved = test_resolved_codex_cli("/tmp/cached-codex");
        *RESOLVED_CODEX_CLI_CACHE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some(Ok(resolved.clone()));

        let cached = resolve_codex_cli_cached().expect("cached Codex resolution should return");
        assert_eq!(cached.path, resolved.path);
        let capabilities =
            probe_codex_cli_cached(&resolved.path).expect("resolved capabilities should be reused");
        assert!(capabilities.has_core_exec_support());

        let (probe, returned_capabilities) = probe_codex_harness_with_capabilities();
        let expected_path = resolved.path.to_string_lossy().to_string();
        assert!(probe.available);
        assert_eq!(probe.binary_path.as_deref(), Some(expected_path.as_str()));
        assert!(returned_capabilities
            .expect("capabilities should be returned")
            .has_core_exec_support());

        *RESOLVED_CODEX_CLI_CACHE
            .get()
            .expect("Codex resolution cache should exist")
            .lock()
            .unwrap() = None;
    }

    #[test]
    fn harness_probe_and_chat_cli_resolution_cache_results() {
        let _lock = plugin_override_lock().lock().expect("lock harness caches");
        if let Some(cache) = HARNESS_RUNTIME_PROBE_CACHE.get() {
            cache.lock().unwrap().clear();
        }
        if let Some(cache) = CHAT_HARNESS_CLI_CACHE.get() {
            cache.lock().unwrap().clear();
        }
        HARNESS_RUNTIME_PROBE_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(
                AgentHarnessKind::Claude,
                HarnessRuntimeProbe {
                    binary_path: Some("/tmp/cached-claude".to_string()),
                    binary_found: true,
                    probe_succeeded: true,
                    available: true,
                    missing_core_exec_features: Vec::new(),
                    cli_version: None,
                    supported_model_aliases: None,
                    supported_efforts: None,
                    error: None,
                },
            );
        assert_eq!(
            probe_harness(AgentHarnessKind::Claude)
                .binary_path
                .as_deref(),
            Some("/tmp/cached-claude")
        );

        let temp = tempfile::tempdir().expect("tempdir");
        let claude_cli = temp.path().join("claude");
        std::fs::write(&claude_cli, "#!/bin/sh\n").expect("write fake claude");
        let first = resolve_chat_harness_cli(AgentHarnessKind::Claude, &claude_cli)
            .expect("Claude chat CLI should resolve");
        let second = resolve_chat_harness_cli(AgentHarnessKind::Claude, &claude_cli)
            .expect("cached Claude chat CLI should resolve");

        match (first, second) {
            (
                ResolvedChatHarnessCli::Claude { cli_path: first },
                ResolvedChatHarnessCli::Claude { cli_path: second },
            ) => assert_eq!(first, second),
            _ => panic!("expected Claude CLI results"),
        }

        HARNESS_RUNTIME_PROBE_CACHE
            .get()
            .expect("probe cache should exist")
            .lock()
            .unwrap()
            .clear();
        CHAT_HARNESS_CLI_CACHE
            .get()
            .expect("chat CLI cache should exist")
            .lock()
            .unwrap()
            .clear();
    }

    #[test]
    fn harness_probe_reuses_in_flight_probe_result() {
        let _lock = plugin_override_lock().lock().expect("lock harness caches");
        if let Some(cache) = HARNESS_RUNTIME_PROBE_CACHE.get() {
            cache.lock().unwrap().clear();
        }
        if let Some(in_flight) = HARNESS_RUNTIME_PROBE_IN_FLIGHT.get() {
            in_flight.lock().unwrap().clear();
        }

        let expected = HarnessRuntimeProbe {
            binary_path: Some("/tmp/in-flight-claude".to_string()),
            binary_found: true,
            probe_succeeded: true,
            available: true,
            missing_core_exec_features: Vec::new(),
            cli_version: None,
            supported_model_aliases: None,
            supported_efforts: None,
            error: None,
        };
        let probe_in_flight = Arc::new(HarnessRuntimeProbeInFlight::new());
        {
            let mut result = probe_in_flight.result.lock().unwrap();
            *result = Some(expected.clone());
        }
        HARNESS_RUNTIME_PROBE_IN_FLIGHT
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(AgentHarnessKind::Claude, probe_in_flight);

        assert_eq!(probe_harness(AgentHarnessKind::Claude), expected);

        HARNESS_RUNTIME_PROBE_IN_FLIGHT
            .get()
            .expect("in-flight probe map should exist")
            .lock()
            .unwrap()
            .clear();
    }

    #[cfg(unix)]
    #[test]
    fn claude_harness_probe_reports_cli_supported_efforts() {
        let _plugin_lock = plugin_override_lock().lock().expect("lock harness caches");
        let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
            .lock()
            .expect("env mutex");
        clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
        let temp = tempfile::tempdir().expect("tempdir");
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        write_fake_executable(
            &bin_dir.join("claude"),
            r#"#!/bin/sh
case "$1" in
  --version)
    echo "2.1.142 (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)"
    ;;
  *)
    exit 2
    ;;
esac
"#,
        );
        let _path = EnvGuard::set_os("PATH", &bin_dir);
        let _home = EnvGuard::set_os("HOME", temp.path());
        let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp.path());
        let _nvm = EnvGuard::unset("NVM_BIN");
        let _volta = EnvGuard::unset("VOLTA_HOME");

        let probe = probe_claude_harness();

        assert!(probe.available);
        assert!(probe.probe_succeeded);
        assert_eq!(probe.cli_version.as_deref(), Some("2.1.142"));
        assert_eq!(
            probe.supported_model_aliases,
            Some(vec![
                "sonnet".to_string(),
                "opus".to_string(),
                "haiku".to_string(),
            ])
        );
        assert_eq!(
            probe.supported_efforts,
            Some(vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
                "max".to_string(),
            ])
        );

        clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
    }

    #[cfg(unix)]
    #[test]
    fn claude_harness_probe_keeps_binary_available_when_capability_probe_fails() {
        let _plugin_lock = plugin_override_lock().lock().expect("lock harness caches");
        let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
            .lock()
            .expect("env mutex");
        clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
        let temp = tempfile::tempdir().expect("tempdir");
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        write_fake_executable(
            &bin_dir.join("claude"),
            r#"#!/bin/sh
echo "probe failed" >&2
exit 42
"#,
        );
        let _path = EnvGuard::set_os("PATH", &bin_dir);
        let _home = EnvGuard::set_os("HOME", temp.path());
        let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp.path());
        let _nvm = EnvGuard::unset("NVM_BIN");
        let _volta = EnvGuard::unset("VOLTA_HOME");

        let probe = probe_claude_harness();

        assert!(probe.binary_found);
        assert!(probe.available);
        assert!(!probe.probe_succeeded);
        assert_eq!(probe.supported_efforts, None);
        assert!(probe
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("probe failed"));

        clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
    }

    #[cfg(unix)]
    #[test]
    fn clearing_claude_runtime_caches_removes_cached_cli_capabilities() {
        let _lock = plugin_override_lock().lock().expect("lock harness caches");
        let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
            .lock()
            .expect("env mutex");
        let temp = tempfile::tempdir().expect("tempdir");
        let cli_path = temp.path().join("claude");
        write_fake_executable(
            &cli_path,
            r#"#!/bin/sh
case "$1" in
  --version)
    echo "2.1.142 (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)"
    ;;
esac
"#,
        );

        assert_eq!(
            crate::infrastructure::agents::claude::normalize_claude_effort_for_cli_path(
                &cli_path, "xhigh",
            ),
            "xhigh"
        );

        write_fake_executable(
            &cli_path,
            r#"#!/bin/sh
case "$1" in
  --version)
    echo "2.1.110 (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, max)"
    ;;
esac
"#,
        );
        assert_eq!(
            crate::infrastructure::agents::claude::normalize_claude_effort_for_cli_path(
                &cli_path, "xhigh",
            ),
            "xhigh"
        );

        clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);

        assert_eq!(
            crate::infrastructure::agents::claude::normalize_claude_effort_for_cli_path(
                &cli_path, "xhigh",
            ),
            "high"
        );

        clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
    }

    #[test]
    fn clearing_codex_runtime_caches_removes_probe_cli_and_capability_entries() {
        let _lock = plugin_override_lock().lock().expect("lock harness caches");
        let codex_path = PathBuf::from("/tmp/codex-cache-test");
        HARNESS_RUNTIME_PROBE_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(
                AgentHarnessKind::Codex,
                HarnessRuntimeProbe {
                    binary_path: Some(codex_path.display().to_string()),
                    binary_found: true,
                    probe_succeeded: true,
                    available: true,
                    missing_core_exec_features: Vec::new(),
                    cli_version: None,
                    supported_model_aliases: None,
                    supported_efforts: None,
                    error: None,
                },
            );
        CHAT_HARNESS_CLI_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(
                (AgentHarnessKind::Codex, codex_path.clone()),
                Ok(ResolvedChatHarnessCli::Codex {
                    cli_path: codex_path.clone(),
                    capabilities: test_codex_capabilities(),
                }),
            );
        *RESOLVED_CODEX_CLI_CACHE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some(Ok(ResolvedCodexCli {
            path: codex_path.clone(),
            capabilities: test_codex_capabilities(),
        }));
        CODEX_CLI_CAPABILITY_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(codex_path, Ok(test_codex_capabilities()));

        clear_harness_runtime_caches_for_harness(AgentHarnessKind::Codex);

        assert!(!HARNESS_RUNTIME_PROBE_CACHE
            .get()
            .expect("probe cache should exist")
            .lock()
            .unwrap()
            .contains_key(&AgentHarnessKind::Codex));
        assert!(CHAT_HARNESS_CLI_CACHE
            .get()
            .expect("chat CLI cache should exist")
            .lock()
            .unwrap()
            .is_empty());
        assert!(RESOLVED_CODEX_CLI_CACHE
            .get()
            .expect("Codex resolution cache should exist")
            .lock()
            .unwrap()
            .is_none());
        assert!(CODEX_CLI_CAPABILITY_CACHE
            .get()
            .expect("Codex capability cache should exist")
            .lock()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn codex_chat_service_cli_path_uses_compatible_candidate() {
        let cli_path = codex_chat_service_cli_path_from_resolve_result(Ok(
            test_resolved_codex_cli("/opt/homebrew/bin/codex"),
        ));

        assert_eq!(cli_path, PathBuf::from("/opt/homebrew/bin/codex"));
    }

    #[test]
    fn codex_chat_service_cli_path_falls_back_to_default_name_when_resolution_fails() {
        let cli_path =
            codex_chat_service_cli_path_from_resolve_result(Err("Codex CLI not found".to_string()));

        assert_eq!(cli_path, PathBuf::from("codex"));
    }

    #[test]
    fn startup_integration_description_matches_variant() {
        let integration = ResolvedHarnessStartupIntegration::RegisterConfiguredMcpServer {
            harness: AgentHarnessKind::Claude,
            cli_path: PathBuf::from("claude"),
            plugin_dir: PathBuf::from("plugins/app"),
        };
        assert_eq!(integration.harness(), AgentHarnessKind::Claude);
        assert_eq!(
            integration.description(),
            "configured MCP server registration"
        );
    }

    #[test]
    fn default_repo_root_working_directory_uses_parent_for_src_tauri() {
        let cwd = PathBuf::from("/tmp/example/src-tauri");
        assert_eq!(
            default_repo_root_working_directory_from(cwd),
            PathBuf::from("/tmp/example")
        );
    }

    #[test]
    fn default_repo_root_working_directory_keeps_non_src_tauri_paths() {
        let cwd = PathBuf::from("/tmp/example");
        assert_eq!(default_repo_root_working_directory_from(cwd.clone()), cwd);
    }

    #[test]
    fn external_mcp_entry_for_plugin_dir_appends_expected_relative_path() {
        let plugin_dir = PathBuf::from("/tmp/plugins/app");
        assert_eq!(
            external_mcp_entry_for_plugin_dir(&plugin_dir),
            plugin_dir.join("ralphx-external-mcp/build/index.js")
        );
    }

    #[test]
    fn resolve_default_harness_agent_bootstrap_sets_expected_defaults() {
        let _lock = plugin_override_lock().lock().expect("lock plugin override");
        let (_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
        let _runtime_guard =
            crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
                plugin_dir,
                generated_dir,
            );
        let working_directory = PathBuf::from("/tmp/example");
        let agent_name = crate::infrastructure::agents::claude::agent_names::AGENT_SESSION_NAMER;
        let bootstrap = resolve_harness_agent_bootstrap(
            DEFAULT_AGENT_HARNESS,
            agent_name,
            working_directory.clone(),
        );

        assert_eq!(bootstrap.agent_name, agent_name);
        assert_eq!(bootstrap.agent_role, "ralphx-utility-session-namer");
        assert_eq!(bootstrap.working_directory, working_directory);
        assert_eq!(
            bootstrap.env.get("RALPHX_AGENT_TYPE"),
            Some(&"ralphx-utility-session-namer".to_string())
        );
        assert_eq!(
            bootstrap.plugin_dir,
            resolve_default_harness_plugin_dir(&bootstrap.working_directory)
        );
    }

    #[test]
    fn resolve_harness_agent_bootstrap_uses_harness_plugin_dir_resolution() {
        let _lock = plugin_override_lock().lock().expect("lock plugin override");
        let (_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
        let _runtime_guard =
            crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
                plugin_dir,
                generated_dir,
            );
        let working_directory = PathBuf::from("/tmp/example");
        let agent_name = crate::infrastructure::agents::claude::agent_names::AGENT_SESSION_NAMER;
        let bootstrap = resolve_harness_agent_bootstrap(
            AgentHarnessKind::Codex,
            agent_name,
            working_directory.clone(),
        );

        assert_eq!(bootstrap.agent_name, agent_name);
        assert_eq!(bootstrap.agent_role, "ralphx-utility-session-namer");
        assert_eq!(bootstrap.working_directory, working_directory);
        assert_eq!(
            bootstrap.plugin_dir,
            resolve_harness_plugin_dir(AgentHarnessKind::Codex, &bootstrap.working_directory)
        );
    }

    #[test]
    fn resolve_harness_plugin_dir_uses_generated_plugin_dir_for_codex() {
        let _lock = plugin_override_lock().lock().expect("lock plugin override");
        let (_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
        let _runtime_guard =
            crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
                plugin_dir,
                generated_dir.clone(),
            );
        let working_directory = PathBuf::from("/tmp/example");

        assert_eq!(
            resolve_harness_plugin_dir(AgentHarnessKind::Codex, &working_directory),
            generated_dir
        );
        assert_eq!(
            resolve_default_harness_plugin_dir(&working_directory),
            generated_dir
        );
    }
}
