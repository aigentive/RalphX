use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

use crate::domain::agents::{
    plan_judge_model_for_provider, standard_harness_map, standard_harness_registry,
    AgentHarnessKind, DEFAULT_AGENT_HARNESS,
};
use crate::infrastructure::agents::claude::{
    agent_harness_defaults_config, automations_config, clear_claude_cli_capability_cache,
    execution_defaults_config, external_mcp_config, find_claude_cli, git_runtime_config,
    node_utils, probe_claude_cli_cached, reconciliation_config, resolve_plugin_dir,
    scheduler_config, ui_feature_flags_config, validate_external_mcp_config, verification_config,
    AgentHarnessDefaultsConfig, ExecutionDefaultsConfig, ExternalMcpConfig, SchedulerConfig,
    UiFeatureFlagsConfig,
};
use crate::infrastructure::agents::{
    find_codex_cli, probe_codex_cli, resolve_codex_cli, CodexCliCapabilities, ResolvedCodexCli,
};
use which::which;

pub(crate) type HarnessProbeFn = fn() -> HarnessRuntimeProbe;
pub(crate) type ChatHarnessCliResolver = fn(&Path) -> Result<ResolvedChatHarnessCli, String>;

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
    pub ultra_supported_models: Vec<String>,
    pub supports_fast_mode: bool,
    pub fast_mode_supported_models: Vec<String>,
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

#[derive(Clone, Copy)]
pub(crate) struct HarnessRuntimeAdapter {
    pub probe: HarnessProbeFn,
    pub resolve_chat_cli: ChatHarnessCliResolver,
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
                        ultra_supported_models: Vec::new(),
                        supports_fast_mode: false,
                        fast_mode_supported_models: Vec::new(),
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
                    ultra_supported_models: Vec::new(),
                    supports_fast_mode: false,
                    fast_mode_supported_models: Vec::new(),
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
            ultra_supported_models: Vec::new(),
            supports_fast_mode: false,
            fast_mode_supported_models: Vec::new(),
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
            let supports_fast_mode = capabilities.supports_fast_mode();
            let fast_mode_supported_models = capabilities.fast_mode_supported_models();
            let supported_model_aliases =
                non_empty_capability_values(capabilities.supported_model_aliases.clone());
            let supported_efforts =
                non_empty_capability_values(capabilities.supported_effort_labels());
            let ultra_supported_models = capabilities.ultra_supported_models.clone();
            HarnessRuntimeProbe {
                binary_path,
                binary_found: true,
                probe_succeeded: true,
                available,
                missing_core_exec_features,
                cli_version: capabilities.version.clone(),
                supported_model_aliases,
                supported_efforts,
                ultra_supported_models,
                supports_fast_mode,
                fast_mode_supported_models,
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
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
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
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
                error: Some(error),
            },
        },
    }
}

fn non_empty_capability_values(values: Vec<String>) -> Option<Vec<String>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
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
static HARNESS_RUNTIME_REFRESH_CACHE: OnceLock<
    Mutex<HashMap<AgentHarnessKind, CachedHarnessRuntimeProbe>>,
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

#[derive(Debug, Clone)]
struct CachedHarnessRuntimeProbe {
    binary_path: PathBuf,
    binary_size: u64,
    binary_modified: SystemTime,
    refreshed_at: Instant,
    probe: HarnessRuntimeProbe,
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

pub(crate) fn standard_harness_runtime_adapters() -> HashMap<AgentHarnessKind, HarnessRuntimeAdapter>
{
    standard_harness_registry(|harness| match harness {
        AgentHarnessKind::Claude => HarnessRuntimeAdapter {
            probe: probe_claude_harness,
            resolve_chat_cli: resolve_claude_chat_harness_cli,
        },
        AgentHarnessKind::Codex => HarnessRuntimeAdapter {
            probe: probe_codex_harness,
            resolve_chat_cli: resolve_codex_chat_harness_cli,
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

/// Test-only seam: seed the harness probe cache so harness-availability checks
/// resolve as available without a real agent CLI on PATH. Tests that exercise
/// real start/send flows must call this so sandboxed CI does not depend on
/// installed `claude`/`codex` binaries.
#[cfg(any(test, feature = "test-utils"))]
pub(crate) fn seed_available_harness_probes_for_test() {
    seed_available_harness_probes_for_test_at("/tmp/test-harness");
}

/// Like [`seed_available_harness_probes_for_test`], but pins the probe's
/// binary path to a caller-owned fixture so send paths that validate CLI
/// existence on disk (e.g. `resolve_claude_chat_harness_cli`) can spawn a
/// real fake CLI.
#[cfg(any(test, feature = "test-utils"))]
pub(crate) fn seed_available_harness_probes_for_test_at(binary_path: &str) {
    let cache = HARNESS_RUNTIME_PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().expect("lock harness probe cache");
    for harness in standard_harness_runtime_adapters().into_keys() {
        cache.insert(
            harness,
            HarnessRuntimeProbe {
                binary_path: Some(binary_path.to_string()),
                binary_found: true,
                probe_succeeded: true,
                available: true,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
                error: None,
            },
        );
    }
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
            ultra_supported_models: Vec::new(),
            supports_fast_mode: false,
            fast_mode_supported_models: Vec::new(),
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
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
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
    refresh_harness_runtime_probe_with_force(harness, false)
}

pub(crate) fn refresh_harness_runtime_probe_with_force(
    harness: AgentHarnessKind,
    force: bool,
) -> HarnessRuntimeProbe {
    if force {
        tracing::info!(
            operation = "harness_runtime_probe_cache",
            outcome = "forced",
            harness = %harness,
            "Harness runtime probe cache bypassed"
        );
    } else if let Some(probe) = cached_harness_runtime_refresh_probe(harness) {
        tracing::info!(
            operation = "harness_runtime_probe_cache",
            outcome = "hit",
            harness = %harness,
            "Harness runtime probe cache hit"
        );
        return probe;
    } else {
        tracing::info!(
            operation = "harness_runtime_probe_cache",
            outcome = "miss",
            harness = %harness,
            "Harness runtime probe cache miss"
        );
    }

    clear_harness_runtime_caches_for_harness(harness);
    let probe = probe_harness(harness);
    cache_successful_harness_runtime_refresh_probe(harness, &probe);
    probe
}

fn cached_harness_runtime_refresh_probe(harness: AgentHarnessKind) -> Option<HarnessRuntimeProbe> {
    let binary_path = resolved_harness_binary_path(harness)?;
    let metadata = std::fs::metadata(&binary_path).ok()?;
    let binary_modified = metadata.modified().ok()?;
    let binary_size = metadata.len();
    let cache = HARNESS_RUNTIME_REFRESH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cached = cache.lock().ok()?.get(&harness)?.clone();
    let ttl = Duration::from_secs(git_runtime_config().provider_probe_cache_ttl_secs);

    (cached.binary_path == binary_path
        && cached.binary_size == binary_size
        && cached.binary_modified == binary_modified
        && cached.refreshed_at.elapsed() <= ttl)
        .then_some(cached.probe)
}

fn cache_successful_harness_runtime_refresh_probe(
    harness: AgentHarnessKind,
    probe: &HarnessRuntimeProbe,
) {
    if !probe.probe_succeeded || !probe.available {
        return;
    }

    let Some(binary_path) = probe.binary_path.as_deref().map(PathBuf::from) else {
        return;
    };
    let Ok(metadata) = std::fs::metadata(&binary_path) else {
        return;
    };
    let Ok(binary_modified) = metadata.modified() else {
        return;
    };

    let cache = HARNESS_RUNTIME_REFRESH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    cache.lock().expect("lock harness refresh cache").insert(
        harness,
        CachedHarnessRuntimeProbe {
            binary_path,
            binary_size: metadata.len(),
            binary_modified,
            refreshed_at: Instant::now(),
            probe: probe.clone(),
        },
    );
}

fn resolved_harness_binary_path(harness: AgentHarnessKind) -> Option<PathBuf> {
    match harness {
        AgentHarnessKind::Claude => find_claude_cli(),
        AgentHarnessKind::Codex => find_codex_cli(),
    }
}

pub(crate) fn clear_harness_runtime_caches_for_harness(harness: AgentHarnessKind) {
    if let Some(cache) = HARNESS_RUNTIME_PROBE_CACHE.get() {
        cache.lock().unwrap().remove(&harness);
    }
    if let Some(cache) = HARNESS_RUNTIME_REFRESH_CACHE.get() {
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

pub(crate) fn default_verification_max_rounds() -> u32 {
    verification_config().max_rounds
}

pub(crate) fn default_ui_feature_flags() -> UiFeatureFlagsConfig {
    ui_feature_flags_config().clone()
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

pub(crate) fn default_automation_scheduler_poll_secs() -> u64 {
    automations_config().scheduler_poll_secs
}

pub(crate) fn default_automation_signal_failure_pause_threshold() -> u64 {
    automations_config().signal_failure_pause_threshold
}

pub(crate) fn default_automation_judge_timeout_secs() -> u64 {
    automations_config().judge_timeout_secs
}

pub(crate) fn default_automation_publish_grace_secs() -> u64 {
    automations_config().publish_grace_secs
}

pub(crate) fn default_automation_max_run_duration_secs() -> u64 {
    automations_config().max_run_duration_secs
}

pub(crate) fn default_automation_plan_judge_models() -> HashMap<AgentHarnessKind, String> {
    let config = automations_config();
    standard_harness_map(
        config
            .plan_judge_model
            .get(&AgentHarnessKind::Claude)
            .cloned()
            .unwrap_or_else(|| plan_judge_model_for_provider(AgentHarnessKind::Claude).to_string()),
        config
            .plan_judge_model
            .get(&AgentHarnessKind::Codex)
            .cloned()
            .unwrap_or_else(|| plan_judge_model_for_provider(AgentHarnessKind::Codex).to_string()),
    )
}

pub(crate) fn default_automation_plan_max_revision_rounds() -> u64 {
    automations_config().plan_max_revision_rounds
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
    refresh_supported_harnesses_with_force(false)
}

pub(crate) fn refresh_supported_harnesses_with_force(
    force: bool,
) -> HashMap<AgentHarnessKind, HarnessRuntimeProbe> {
    probe_standard_harnesses_with(
        |harness| refresh_harness_runtime_probe_with_force(harness, force),
        "refresh",
    )
}

fn probe_standard_harnesses_with<F>(
    probe_fn: F,
    operation: &'static str,
) -> HashMap<AgentHarnessKind, HarnessRuntimeProbe>
where
    F: Fn(AgentHarnessKind) -> HarnessRuntimeProbe + Copy + Send + Sync,
{
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
            let supports_fast_mode = capabilities.supports_fast_mode();
            let fast_mode_supported_models = capabilities.fast_mode_supported_models();
            let supported_model_aliases =
                non_empty_capability_values(capabilities.supported_model_aliases.clone());
            let supported_efforts =
                non_empty_capability_values(capabilities.supported_effort_labels());
            let ultra_supported_models = capabilities.ultra_supported_models.clone();
            (
                HarnessRuntimeProbe {
                    binary_path: Some(resolved.path.to_string_lossy().into_owned()),
                    binary_found: true,
                    probe_succeeded: true,
                    available,
                    missing_core_exec_features,
                    cli_version: capabilities.version.clone(),
                    supported_model_aliases,
                    supported_efforts,
                    ultra_supported_models,
                    supports_fast_mode,
                    fast_mode_supported_models,
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
                    ultra_supported_models: Vec::new(),
                    supports_fast_mode: false,
                    fast_mode_supported_models: Vec::new(),
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
                    ultra_supported_models: Vec::new(),
                    supports_fast_mode: false,
                    fast_mode_supported_models: Vec::new(),
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

#[cfg(test)]
#[path = "harness_runtime_registry_inline_tests.rs"]
mod tests;
