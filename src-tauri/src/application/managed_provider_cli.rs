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
) -> Option<Result<PathBuf, String>> {
    if settings.cli_management_mode != AgentProviderCliManagementMode::RxManaged {
        return None;
    }

    match settings.provider {
        AgentHarnessKind::Codex => Some(Ok(managed_codex_cli_path())),
        AgentHarnessKind::Claude => None,
    }
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

    match launch_path {
        Ok(path) if settings.provider == AgentHarnessKind::Codex => {
            Some(managed_codex_runtime_probe(path))
        }
        Ok(_) => Some(unavailable_managed_provider_probe(
            "RX-managed provider launches are unavailable for this provider.",
        )),
        Err(error) => Some(unavailable_managed_provider_probe(error)),
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

fn unavailable_managed_provider_probe(error: impl Into<String>) -> HarnessRuntimeProbe {
    HarnessRuntimeProbe {
        binary_path: None,
        binary_found: false,
        probe_succeeded: false,
        available: false,
        missing_core_exec_features: Vec::new(),
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        error: Some(error.into()),
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
