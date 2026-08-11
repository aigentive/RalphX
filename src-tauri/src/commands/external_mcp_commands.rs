// External MCP config commands — expose ExternalMcpConfig as readable/writable via Tauri IPC.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::harness_runtime_registry::{
    default_external_mcp_config, default_external_mcp_config_path,
};

const AUTH_TOKEN_MASK: &str = "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}";

#[tauri::command]
pub fn get_external_mcp_readiness(
    handle: State<'_, crate::infrastructure::ExternalMcpHandle>,
) -> crate::infrastructure::ExternalMcpReadinessState {
    handle.readiness()
}

/// Masked view of ExternalMcpConfig safe for frontend consumption.
/// auth_token is never returned in plaintext — masked as "••••••••" when set, None when unset.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalMcpConfigView {
    pub enabled: bool,
    pub port: u16,
    pub host: String,
    pub auth_token: Option<String>,
    pub node_path: Option<String>,
}

/// Input for updating ExternalMcpConfig. All fields are optional — only Some values are applied.
/// auth_token is accepted as plaintext on write and stored verbatim in config/external-mcp.yaml.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalMcpConfigUpdate {
    pub enabled: Option<bool>,
    pub port: Option<u16>,
    pub host: Option<String>,
    pub auth_token: Option<String>,
    pub node_path: Option<String>,
}

/// Returns a masked view of the current ExternalMcpConfig.
/// auth_token is masked as "••••••••" when set, None when unset.
#[tauri::command]
pub fn get_external_mcp_config() -> ExternalMcpConfigView {
    let config = default_external_mcp_config();
    ExternalMcpConfigView {
        enabled: config.enabled,
        port: config.port,
        host: config.host.clone(),
        auth_token: config
            .auth_token
            .as_ref()
            .map(|_| AUTH_TOKEN_MASK.to_string()),
        node_path: config.node_path.clone(),
    }
}

/// Atomically updates ExternalMcpConfig fields in config/external-mcp.yaml.
/// Writes to {path}.tmp then renames atomically — original is unchanged if process exits mid-write.
/// Only Some fields from `input` are written; absent fields are preserved as-is.
#[tauri::command]
pub async fn update_external_mcp_config(input: ExternalMcpConfigUpdate) -> Result<(), String> {
    let path = default_external_mcp_config_path();

    // This command only edits RalphX-owned config paths resolved by the runtime registry.
    // codeql[rust/path-injection]
    let contents = tokio::fs::read_to_string(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "Permission denied reading {}. Check file permissions and try again.",
                path.display()
            )
        } else {
            format!("Failed to read config file {}: {}", path.display(), e)
        }
    })?;

    let mut doc: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    let root = doc
        .as_mapping_mut()
        .ok_or_else(|| format!("{} root is not a YAML mapping", path.display()))?;

    let mcp_section = root
        .entry(serde_yaml::Value::String("external_mcp".to_string()))
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

    let mcp_map = mcp_section
        .as_mapping_mut()
        .ok_or_else(|| "external_mcp section is not a YAML mapping".to_string())?;

    if let Some(enabled) = input.enabled {
        mcp_map.insert(
            serde_yaml::Value::String("enabled".to_string()),
            serde_yaml::Value::Bool(enabled),
        );
    }
    if let Some(port) = input.port {
        mcp_map.insert(
            serde_yaml::Value::String("port".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(port)),
        );
    }
    if let Some(host) = input.host {
        mcp_map.insert(
            serde_yaml::Value::String("host".to_string()),
            serde_yaml::Value::String(host),
        );
    }
    if let Some(auth_token) = input.auth_token {
        mcp_map.insert(
            serde_yaml::Value::String("auth_token".to_string()),
            serde_yaml::Value::String(auth_token),
        );
    }
    if let Some(node_path) = input.node_path {
        mcp_map.insert(
            serde_yaml::Value::String("node_path".to_string()),
            serde_yaml::Value::String(node_path),
        );
    }

    let updated =
        serde_yaml::to_string(&doc).map_err(|e| format!("Failed to serialize config: {e}"))?;

    let tmp_path = {
        let mut p = path.clone();
        let ext = p
            .extension()
            .map(|e| format!("{}.tmp", e.to_string_lossy()))
            .unwrap_or_else(|| "tmp".to_string());
        p.set_extension(ext);
        p
    };

    // The temp path is derived from the validated RalphX config path above.
    // codeql[rust/path-injection]
    tokio::fs::write(&tmp_path, &updated).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "Permission denied writing to {}. Check file permissions and try again.",
                tmp_path.display()
            )
        } else {
            format!(
                "Failed to write temp config file {}: {}",
                tmp_path.display(),
                e
            )
        }
    })?;

    // Atomic replacement stays within the same RalphX-owned config path.
    // codeql[rust/path-injection]
    tokio::fs::rename(&tmp_path, &path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "Permission denied renaming config to {}. Check file permissions and try again.",
                path.display()
            )
        } else {
            format!("Failed to rename temp config to {}: {}", path.display(), e)
        }
    })?;

    Ok(())
}

#[cfg(test)]
#[path = "external_mcp_commands_tests.rs"]
mod tests;
