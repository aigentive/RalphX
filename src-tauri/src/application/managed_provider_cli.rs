use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use crate::application::harness_runtime_registry::HarnessRuntimeProbe;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::infrastructure::agents::claude::probe_claude_cli_cached;
use crate::infrastructure::agents::codex::probe_codex_cli;
use crate::infrastructure::tool_paths::{
    has_safe_absolute_binary_path_shape, is_safe_launchable_binary_path,
};
use crate::utils::runtime_log_paths::managed_codex_binary_path;

pub(crate) fn provider_cli_launch_path(
    settings: &AgentProviderSettings,
) -> Option<Result<PathBuf, String>> {
    if let Some(path) = custom_provider_cli_launch_path(settings) {
        return Some(path);
    }

    managed_provider_cli_launch_path(settings).map(Ok)
}

pub(crate) fn managed_provider_cli_launch_path(
    settings: &AgentProviderSettings,
) -> Option<PathBuf> {
    if settings.cli_management_mode != AgentProviderCliManagementMode::RxManaged {
        return None;
    }

    match settings.provider {
        AgentHarnessKind::Codex => Some(managed_codex_cli_path()),
        AgentHarnessKind::Claude => None,
    }
}

pub(crate) fn checked_provider_cli_launch_path(
    settings: &AgentProviderSettings,
    purpose: &str,
) -> Option<Result<PathBuf, String>> {
    let cli_path = match provider_cli_launch_path(settings)? {
        Ok(path) => path,
        Err(error) => return Some(Err(error)),
    };
    if let Some(probe) = provider_runtime_probe(settings) {
        if !probe.available {
            return Some(Err(managed_probe_error(probe, purpose, settings.provider)));
        }
    }
    Some(Ok(cli_path))
}

pub(crate) fn checked_managed_provider_cli_launch_path(
    settings: &AgentProviderSettings,
    purpose: &str,
) -> Option<Result<PathBuf, String>> {
    checked_provider_cli_launch_path(settings, purpose)
}

fn managed_probe_error(
    probe: HarnessRuntimeProbe,
    purpose: &str,
    provider: AgentHarnessKind,
) -> String {
    probe
        .error
        .unwrap_or_else(|| format!("{purpose} harness unavailable: {provider}"))
}

fn managed_codex_cli_path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = managed_codex_binary_path_override()
        .lock()
        .expect("managed Codex path override mutex")
        .clone()
    {
        return path;
    }

    managed_codex_binary_path()
}

pub(crate) fn provider_runtime_probe(
    settings: &AgentProviderSettings,
) -> Option<HarnessRuntimeProbe> {
    if settings.custom_binary_enabled {
        return Some(custom_provider_runtime_probe(
            settings,
            custom_provider_cli_launch_path(settings)
                .unwrap_or_else(|| Err(custom_binary_path_required_error(settings.provider))),
        ));
    }

    let launch_path = managed_provider_cli_launch_path(settings)?;
    Some(managed_codex_runtime_probe(launch_path))
}

fn custom_provider_cli_launch_path(
    settings: &AgentProviderSettings,
) -> Option<Result<PathBuf, String>> {
    if !settings.custom_binary_enabled {
        return None;
    }

    Some(validate_custom_provider_cli_path(settings))
}

fn validate_custom_provider_cli_path(settings: &AgentProviderSettings) -> Result<PathBuf, String> {
    let raw_path = settings
        .custom_binary_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| custom_binary_path_required_error(settings.provider))?;
    let path = PathBuf::from(raw_path);
    if !has_safe_absolute_binary_path_shape(&path) {
        return Err(format!(
            "Custom {} binary path must be an absolute path without . or .. components.",
            settings.provider
        ));
    }
    if !is_safe_launchable_binary_path(&path) {
        return Err(format!(
            "Custom {} binary path is not a launchable executable file: {}",
            settings.provider,
            path.display()
        ));
    }
    Ok(path)
}

fn custom_binary_path_required_error(provider: AgentHarnessKind) -> String {
    format!("Custom {provider} binary path is required before enabling custom binary mode")
}

fn custom_provider_runtime_probe(
    settings: &AgentProviderSettings,
    path: Result<PathBuf, String>,
) -> HarnessRuntimeProbe {
    let path = match path {
        Ok(path) => path,
        Err(error) => {
            return HarnessRuntimeProbe {
                binary_path: settings.custom_binary_path.clone(),
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
            }
        }
    };

    match settings.provider {
        AgentHarnessKind::Codex => custom_codex_runtime_probe(path),
        AgentHarnessKind::Claude => custom_claude_runtime_probe(path),
    }
}

fn managed_codex_runtime_probe(path: PathBuf) -> HarnessRuntimeProbe {
    if !is_launchable_file(&path) {
        return HarnessRuntimeProbe {
            binary_path: Some(path.to_string_lossy().into_owned()),
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
            error: Some("RX-managed Codex is not installed.".to_string()),
        };
    }

    match probe_codex_cli(&path) {
        Ok(capabilities) => {
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
                    "RX-managed Codex is missing required capability: {}",
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
                binary_path: Some(path.to_string_lossy().into_owned()),
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
        Err(error) => HarnessRuntimeProbe {
            binary_path: Some(path.to_string_lossy().into_owned()),
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
    }
}

fn custom_codex_runtime_probe(path: PathBuf) -> HarnessRuntimeProbe {
    match probe_codex_cli(&path) {
        Ok(capabilities) => {
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
                    "Custom Codex binary is missing required capability: {}",
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
                binary_path: Some(path.to_string_lossy().into_owned()),
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
        Err(error) => HarnessRuntimeProbe {
            binary_path: Some(path.to_string_lossy().into_owned()),
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
    }
}

fn non_empty_capability_values(values: Vec<String>) -> Option<Vec<String>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn custom_claude_runtime_probe(path: PathBuf) -> HarnessRuntimeProbe {
    match probe_claude_cli_cached(&path) {
        Ok(capabilities) => {
            let supported_efforts = capabilities.supported_effort_labels();
            HarnessRuntimeProbe {
                binary_path: Some(path.to_string_lossy().into_owned()),
                binary_found: true,
                probe_succeeded: true,
                available: true,
                missing_core_exec_features: Vec::new(),
                cli_version: capabilities.version,
                supported_model_aliases: Some(capabilities.supported_model_aliases),
                supported_efforts: Some(supported_efforts),
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
                error: None,
            }
        }
        Err(error) => HarnessRuntimeProbe {
            binary_path: Some(path.to_string_lossy().into_owned()),
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
    }
}

pub(crate) fn is_launchable_file(path: &Path) -> bool {
    // Path is expected to be derived from RalphX-owned runtime storage plus
    // fixed provider components before reaching this sink.
    // codeql[rust/path-injection]
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
fn managed_codex_binary_path_override() -> &'static Mutex<Option<PathBuf>> {
    static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(crate) struct ManagedCodexBinaryPathOverrideGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl Drop for ManagedCodexBinaryPathOverrideGuard {
    fn drop(&mut self) {
        *managed_codex_binary_path_override()
            .lock()
            .expect("managed Codex path override mutex") = self.previous.take();
    }
}

#[cfg(test)]
pub(crate) fn override_managed_codex_binary_path_for_tests(
    path: PathBuf,
) -> ManagedCodexBinaryPathOverrideGuard {
    let mut override_path = managed_codex_binary_path_override()
        .lock()
        .expect("managed Codex path override mutex");
    let previous = override_path.replace(path);
    ManagedCodexBinaryPathOverrideGuard { previous }
}

#[cfg(test)]
#[path = "managed_provider_cli_tests.rs"]
mod tests;
