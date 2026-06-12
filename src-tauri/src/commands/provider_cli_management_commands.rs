use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::{Method, Request};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use tokio::process::Command;
use tokio_util::bytes::Bytes;

use crate::application::{managed_provider_cli::is_launchable_file, AppState};
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
    STANDARD_AGENT_HARNESSES,
};
use crate::infrastructure::tool_paths::{agent_subprocess_env_path, resolve_shell_cli_path};
use crate::utils::runtime_log_paths::{
    managed_codex_bin_dir, managed_codex_binary_path, managed_codex_home_dir,
    managed_codex_installer_home_dir,
};

const CODEX_INSTALLER_SCRIPT_URL: &str = "https://chatgpt.com/codex/install.sh";
const CODEX_LATEST_RELEASE_URL: &str = "https://api.github.com/repos/openai/codex/releases/latest";
const CODEX_INSTALLER_COMMAND: &str = "curl -fsSL https://chatgpt.com/codex/install.sh | sh";
const MANAGED_CLI_INSTALL_TIMEOUT_SECS: u64 = 10 * 60;
const MANAGED_CLI_VERSION_TIMEOUT_SECS: u64 = 10;
const MANAGED_CLI_LATEST_VERSION_TIMEOUT_SECS: u64 = 15;
const MAX_PROCESS_OUTPUT_CHARS: usize = 4_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProviderCliStatusesResponse {
    pub providers: Vec<ManagedProviderCliStatusResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProviderCliStatusResponse {
    pub provider: String,
    pub cli_management_mode: String,
    pub auto_update_enabled: bool,
    pub supported: bool,
    pub installed: bool,
    pub binary_path: Option<String>,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub action: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProviderCliActionInput {
    pub provider: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProviderCliActionResponse {
    pub provider: String,
    pub success: bool,
    pub status: ManagedProviderCliStatusResponse,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProviderCliAutoUpdateResponse {
    pub updated: Vec<ManagedProviderCliActionResponse>,
    pub skipped: Vec<ManagedProviderCliStatusResponse>,
}

#[derive(Debug, Clone)]
struct ManagedProviderCliObservation {
    supported: bool,
    installed: bool,
    binary_path: Option<PathBuf>,
    current_version: Option<String>,
    latest_version: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct ManagedCodexInstallPlan {
    shell_path: PathBuf,
    command: &'static str,
    bin_dir: PathBuf,
    home_dir: PathBuf,
    installer_home_dir: PathBuf,
    binary_path: PathBuf,
    path_env: OsString,
}

fn parse_provider(value: &str) -> Result<AgentHarnessKind, String> {
    value
        .parse::<AgentHarnessKind>()
        .map_err(|err| format!("Invalid provider: {err}"))
}

fn settings_for_provider(
    stored: &[AgentProviderSettings],
    provider: AgentHarnessKind,
) -> AgentProviderSettings {
    stored
        .iter()
        .find(|row| row.provider == provider)
        .cloned()
        .unwrap_or_else(|| AgentProviderSettings::disabled_defaults(provider))
}

fn managed_cli_action(
    settings: &AgentProviderSettings,
    observation: &ManagedProviderCliObservation,
) -> &'static str {
    if !observation.supported {
        return "unsupported";
    }
    if settings.cli_management_mode != AgentProviderCliManagementMode::RxManaged {
        return "none";
    }
    if !observation.installed {
        return "install";
    }
    if managed_cli_update_available(observation) {
        return "update";
    }
    "none"
}

fn managed_cli_update_available(observation: &ManagedProviderCliObservation) -> bool {
    let (Some(current), Some(latest)) = (
        observation.current_version.as_deref(),
        observation.latest_version.as_deref(),
    ) else {
        return false;
    };
    compare_version_strings(current, latest).is_some_and(|ordering| ordering.is_lt())
}

fn managed_cli_status_text(
    provider: AgentHarnessKind,
    settings: &AgentProviderSettings,
    observation: &ManagedProviderCliObservation,
) -> String {
    if provider == AgentHarnessKind::Claude && !observation.supported {
        return "RX-managed Claude installs are unavailable for this installer path.".to_string();
    }
    if !observation.supported {
        return format!("RX-managed {provider} installs are unavailable.");
    }
    if settings.cli_management_mode != AgentProviderCliManagementMode::RxManaged {
        return format!("{provider} CLI is user-managed. RX will not install or update it.");
    }
    if !observation.installed {
        return format!("RX-managed {provider} is not installed.");
    }
    if managed_cli_update_available(observation) {
        let current = observation.current_version.as_deref().unwrap_or("unknown");
        let latest = observation.latest_version.as_deref().unwrap_or("latest");
        return format!("RX-managed {provider} {current} can update to {latest}.");
    }
    if let Some(version) = observation.current_version.as_deref() {
        return format!("RX-managed {provider} {version} is installed.");
    }
    format!("RX-managed {provider} is installed.")
}

fn managed_cli_status_response(
    settings: AgentProviderSettings,
    observation: ManagedProviderCliObservation,
) -> ManagedProviderCliStatusResponse {
    let action = managed_cli_action(&settings, &observation);
    let update_available = managed_cli_update_available(&observation);
    let status = managed_cli_status_text(settings.provider, &settings, &observation);
    ManagedProviderCliStatusResponse {
        provider: settings.provider.to_string(),
        cli_management_mode: settings.cli_management_mode.to_string(),
        auto_update_enabled: settings.auto_update_enabled,
        supported: observation.supported,
        installed: observation.installed,
        binary_path: observation
            .binary_path
            .map(|path| path.to_string_lossy().into_owned()),
        current_version: observation.current_version,
        latest_version: observation.latest_version,
        update_available,
        action: action.to_string(),
        status,
        error: observation.error,
    }
}

fn unsupported_claude_observation() -> ManagedProviderCliObservation {
    ManagedProviderCliObservation {
        supported: false,
        installed: false,
        binary_path: None,
        current_version: None,
        latest_version: None,
        error: Some(
            "Claude Code does not expose a documented RX-owned install prefix yet.".to_string(),
        ),
    }
}

async fn managed_codex_observation(include_latest_version: bool) -> ManagedProviderCliObservation {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return ManagedProviderCliObservation {
            supported: false,
            installed: false,
            binary_path: None,
            current_version: None,
            latest_version: None,
            error: Some(
                "RX-managed Codex installs are only supported on macOS and Linux.".to_string(),
            ),
        };
    }

    let binary_path = managed_codex_binary_path();
    let installed = is_launchable_file(&binary_path);
    let current_version = if installed {
        match probe_cli_version(&binary_path).await {
            Ok(output) => parse_codex_version(&output).or_else(|| Some(output.trim().to_string())),
            Err(error) => {
                return ManagedProviderCliObservation {
                    supported: true,
                    installed,
                    binary_path: Some(binary_path),
                    current_version: None,
                    latest_version: None,
                    error: Some(error),
                }
            }
        }
    } else {
        None
    };

    let latest_version = if include_latest_version {
        fetch_latest_codex_version().await.ok()
    } else {
        None
    };

    ManagedProviderCliObservation {
        supported: true,
        installed,
        binary_path: Some(binary_path),
        current_version,
        latest_version,
        error: None,
    }
}

async fn managed_provider_cli_status_for_settings(
    settings: AgentProviderSettings,
    include_latest_version: bool,
) -> ManagedProviderCliStatusResponse {
    let observation = match settings.provider {
        AgentHarnessKind::Codex => {
            managed_codex_observation(
                include_latest_version
                    && settings.cli_management_mode == AgentProviderCliManagementMode::RxManaged,
            )
            .await
        }
        AgentHarnessKind::Claude => unsupported_claude_observation(),
    };
    managed_cli_status_response(settings, observation)
}

async fn read_managed_provider_cli_statuses(
    state: &AppState,
    include_latest_version: bool,
) -> Result<ManagedProviderCliStatusesResponse, String> {
    let stored = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|err| err.to_string())?;
    let mut providers = Vec::new();
    for provider in STANDARD_AGENT_HARNESSES {
        providers.push(
            managed_provider_cli_status_for_settings(
                settings_for_provider(&stored, provider),
                include_latest_version,
            )
            .await,
        );
    }
    Ok(ManagedProviderCliStatusesResponse { providers })
}

fn path_with_prepended_dir(dir: &Path, existing_path: &OsStr) -> OsString {
    let mut entries = vec![dir.to_path_buf()];
    entries.extend(std::env::split_paths(existing_path));
    std::env::join_paths(entries).unwrap_or_else(|_| existing_path.to_os_string())
}

fn managed_codex_install_plan() -> ManagedCodexInstallPlan {
    let bin_dir = managed_codex_bin_dir();
    let home_dir = managed_codex_home_dir();
    let installer_home_dir = managed_codex_installer_home_dir();
    let binary_path = managed_codex_binary_path();
    let base_path = agent_subprocess_env_path();
    ManagedCodexInstallPlan {
        shell_path: resolve_shell_cli_path(),
        command: CODEX_INSTALLER_COMMAND,
        path_env: path_with_prepended_dir(&bin_dir, base_path.as_os_str()),
        bin_dir,
        home_dir,
        installer_home_dir,
        binary_path,
    }
}

fn ensure_managed_codex_dirs(plan: &ManagedCodexInstallPlan) -> Result<(), String> {
    for dir in [&plan.bin_dir, &plan.home_dir, &plan.installer_home_dir] {
        // Directory is derived from RalphX-owned app runtime storage and fixed components.
        // codeql[rust/path-injection]
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("Failed to create {}: {error}", dir.display()))?;
    }
    Ok(())
}

async fn run_managed_codex_installer() -> Result<ManagedProviderCliActionOutput, String> {
    let plan = managed_codex_install_plan();
    ensure_managed_codex_dirs(&plan)?;

    tracing::info!(
        bin_dir = %plan.bin_dir.display(),
        home_dir = %plan.home_dir.display(),
        installer_home_dir = %plan.installer_home_dir.display(),
        script_url = CODEX_INSTALLER_SCRIPT_URL,
        "Starting RX-managed Codex installer"
    );

    let mut command = Command::new(&plan.shell_path);
    command
        .arg("-c")
        .arg(plan.command)
        .env("CODEX_INSTALL_DIR", &plan.bin_dir)
        .env("CODEX_HOME", &plan.home_dir)
        .env("CODEX_NON_INTERACTIVE", "1")
        .env("HOME", &plan.installer_home_dir)
        .env("PATH", &plan.path_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = tokio::time::timeout(
        Duration::from_secs(MANAGED_CLI_INSTALL_TIMEOUT_SECS),
        command.output(),
    )
    .await
    .map_err(|_| {
        format!("RX-managed Codex installer timed out after {MANAGED_CLI_INSTALL_TIMEOUT_SECS}s")
    })?
    .map_err(|error| format!("Failed to run RX-managed Codex installer: {error}"))?;

    let stdout = truncate_process_output(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate_process_output(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!(
            "RX-managed Codex installer failed with status {}. {}",
            output.status,
            stderr
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("No stderr captured.")
        ));
    }

    Ok(ManagedProviderCliActionOutput { stdout, stderr })
}

#[derive(Debug, Clone)]
struct ManagedProviderCliActionOutput {
    stdout: Option<String>,
    stderr: Option<String>,
}

fn truncate_process_output(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let truncated = trimmed
        .chars()
        .take(MAX_PROCESS_OUTPUT_CHARS)
        .collect::<String>();
    Some(if trimmed.chars().count() > MAX_PROCESS_OUTPUT_CHARS {
        format!("{truncated}\n...")
    } else {
        truncated
    })
}

async fn install_or_update_managed_provider_cli_inner(
    provider: AgentHarnessKind,
    state: &AppState,
) -> Result<ManagedProviderCliActionResponse, String> {
    let settings = state
        .agent_provider_settings_repo
        .get(provider)
        .await
        .map_err(|err| err.to_string())?
        .unwrap_or_else(|| AgentProviderSettings::disabled_defaults(provider));

    if settings.cli_management_mode != AgentProviderCliManagementMode::RxManaged {
        return Err(format!(
            "{provider} is configured as user-managed. Enable RX-managed installs before running this action."
        ));
    }
    if provider != AgentHarnessKind::Codex {
        return Err(format!(
            "RX-managed installs are not available for {provider}."
        ));
    }

    let output = run_managed_codex_installer().await?;
    let status = managed_provider_cli_status_for_settings(settings, true).await;
    Ok(ManagedProviderCliActionResponse {
        provider: provider.to_string(),
        success: true,
        status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

async fn probe_cli_version(cli_path: &Path) -> Result<String, String> {
    let mut command = Command::new(cli_path);
    command
        .arg("--version")
        .env("PATH", agent_subprocess_env_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(
        Duration::from_secs(MANAGED_CLI_VERSION_TIMEOUT_SECS),
        command.output(),
    )
    .await
    .map_err(|_| {
        format!(
            "Timed out while checking {} version",
            cli_path
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("CLI")
        )
    })?
    .map_err(|error| format!("Failed to check {} version: {error}", cli_path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Failed to check {} version: {}",
            cli_path.display(),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_codex_version(output: &str) -> Option<String> {
    let trimmed = output.trim();
    trimmed
        .strip_prefix("codex-cli ")
        .or_else(|| trimmed.strip_prefix("codex "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn fetch_latest_codex_version() -> Result<String, String> {
    install_rustls_crypto_provider();
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .map_err(|error| format!("native root certificates unavailable: {error}"))?
        .https_only()
        .enable_http1()
        .build();
    let client: Client<HttpsConnector<HttpConnector>, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build(https);
    let uri = CODEX_LATEST_RELEASE_URL
        .parse::<hyper::Uri>()
        .map_err(|error| format!("Invalid Codex release URL: {error}"))?;
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("User-Agent", "RalphX")
        .header("Accept", "application/vnd.github+json")
        .body(Full::new(Bytes::new()))
        .map_err(|error| format!("Failed to build Codex release request: {error}"))?;
    let response = tokio::time::timeout(
        Duration::from_secs(MANAGED_CLI_LATEST_VERSION_TIMEOUT_SECS),
        client.request(request),
    )
    .await
    .map_err(|_| "Timed out while checking latest Codex CLI version".to_string())?
    .map_err(|error| format!("Failed to check latest Codex CLI version: {error}"))?;
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|error| format!("Failed to read latest Codex CLI version: {error}"))?
        .to_bytes();
    if !status.is_success() {
        return Err(format!(
            "GitHub returned HTTP {} while checking latest Codex CLI version",
            status.as_u16()
        ));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Failed to parse latest Codex CLI version: {error}"))?;
    let tag = value
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "Latest Codex CLI release is missing tag_name".to_string())?;
    normalize_codex_release_tag(tag)
        .ok_or_else(|| format!("Latest Codex CLI release tag is not supported: {tag}"))
}

fn normalize_codex_release_tag(tag: &str) -> Option<String> {
    let version = tag
        .trim()
        .strip_prefix("rust-v")
        .or_else(|| tag.trim().strip_prefix('v'))
        .unwrap_or_else(|| tag.trim());
    (!version.is_empty()).then(|| version.to_string())
}

fn compare_version_strings(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let left_parts = version_number_parts(left)?;
    let right_parts = version_number_parts(right)?;
    Some(left_parts.cmp(&right_parts))
}

fn version_number_parts(version: &str) -> Option<Vec<u64>> {
    let parts = version
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then_some(parts)
}

fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

#[tauri::command]
pub async fn get_managed_provider_cli_status(
    state: State<'_, AppState>,
) -> Result<ManagedProviderCliStatusesResponse, String> {
    read_managed_provider_cli_statuses(&state, true).await
}

#[tauri::command]
pub async fn install_or_update_managed_provider_cli(
    input: ManagedProviderCliActionInput,
    state: State<'_, AppState>,
) -> Result<ManagedProviderCliActionResponse, String> {
    let provider = parse_provider(&input.provider)?;
    install_or_update_managed_provider_cli_inner(provider, &state).await
}

#[tauri::command]
pub async fn auto_update_managed_provider_clis(
    state: State<'_, AppState>,
) -> Result<ManagedProviderCliAutoUpdateResponse, String> {
    let statuses = read_managed_provider_cli_statuses(&state, true).await?;
    let mut updated = Vec::new();
    let mut skipped = Vec::new();

    for status in statuses.providers {
        if status.cli_management_mode == AgentProviderCliManagementMode::RxManaged.to_string()
            && status.auto_update_enabled
            && status.supported
            && matches!(status.action.as_str(), "install" | "update")
        {
            let provider = parse_provider(&status.provider)?;
            updated.push(install_or_update_managed_provider_cli_inner(provider, &state).await?);
        } else {
            skipped.push(status);
        }
    }

    Ok(ManagedProviderCliAutoUpdateResponse { updated, skipped })
}

#[cfg(test)]
#[path = "provider_cli_management_commands_tests.rs"]
mod tests;
