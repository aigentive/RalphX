use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use crate::application::harness_runtime_registry::HarnessRuntimeProbe;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::infrastructure::agents::codex::probe_codex_cli;
use crate::utils::runtime_log_paths::managed_codex_binary_path;

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

pub(crate) fn checked_managed_provider_cli_launch_path(
    settings: &AgentProviderSettings,
    purpose: &str,
) -> Option<Result<PathBuf, String>> {
    let cli_path = managed_provider_cli_launch_path(settings)?;
    if let Some(probe) = managed_provider_runtime_probe(settings) {
        if !probe.available {
            return Some(Err(managed_probe_error(probe, purpose, settings.provider)));
        }
    }
    Some(Ok(cli_path))
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

pub(crate) fn managed_provider_runtime_probe(
    settings: &AgentProviderSettings,
) -> Option<HarnessRuntimeProbe> {
    let launch_path = managed_provider_cli_launch_path(settings)?;
    Some(managed_codex_runtime_probe(launch_path))
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
            HarnessRuntimeProbe {
                binary_path: Some(path.to_string_lossy().into_owned()),
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
        Err(error) => HarnessRuntimeProbe {
            binary_path: Some(path.to_string_lossy().into_owned()),
            binary_found: true,
            probe_succeeded: false,
            available: false,
            missing_core_exec_features: Vec::new(),
            cli_version: None,
            supported_model_aliases: None,
            supported_efforts: None,
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
