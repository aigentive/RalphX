use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use toml::Value;

use crate::domain::agents::{
    AgentHarnessKind, McpServerKey, NativeMcpServerSnapshot, NativeMcpState,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

pub(crate) fn discover_native_mcp_servers(
    codex_root: &Path,
    project_root: Option<&Path>,
) -> Result<Vec<NativeMcpServerSnapshot>, String> {
    let codex_root = validate_absolute_non_root_path(codex_root, "Codex config root")
        .map_err(|error| error.to_string())?;
    let user_config_path = codex_root.join("config.toml");
    let user_config = read_fixed_toml(
        &codex_root,
        &codex_root,
        &user_config_path,
        Path::new("config.toml"),
    )?;
    let mut servers = BTreeMap::<String, NativeMcpServerSnapshot>::new();
    if let Some(config) = user_config.as_ref() {
        insert_server_table(&mut servers, config, "user", true)?;
    }

    if let Some(project_root) = project_root {
        let project_root = validate_absolute_non_root_path(project_root, "Codex project root")
            .map_err(|error| error.to_string())?;
        let trusted = user_config
            .as_ref()
            .and_then(|config| config.get("projects"))
            .and_then(Value::as_table)
            .and_then(|projects| projects.get(project_root.to_string_lossy().as_ref()))
            .and_then(Value::as_table)
            .and_then(|project| project.get("trust_level"))
            .and_then(Value::as_str)
            == Some("trusted");
        let project_config_root = project_root.join(".codex");
        let project_config_path = project_config_root.join("config.toml");
        if let Some(config) = read_fixed_toml(
            &project_root,
            &project_config_root,
            &project_config_path,
            Path::new("config.toml"),
        )? {
            insert_server_table(&mut servers, &config, "project", trusted)?;
        }
    }
    Ok(servers.into_values().collect())
}

fn read_fixed_toml(
    containment_root: &Path,
    owned_root: &Path,
    path: &Path,
    expected_relative: &Path,
) -> Result<Option<Value>, String> {
    if path.strip_prefix(owned_root).ok() != Some(expected_relative) {
        return Err(format!(
            "Codex config path is not fixed: {}",
            path.display()
        ));
    }
    // codeql[rust/path-injection]
    if !path.exists() {
        return Ok(None);
    }
    let canonical_root = containment_root
        .canonicalize()
        .map_err(|error| format!("Resolve Codex config containment root: {error}"))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| format!("Resolve Codex config: {error}"))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(format!(
            "Codex config escapes owned root: {}",
            path.display()
        ));
    }
    // codeql[rust/path-injection]
    let contents = fs::read_to_string(canonical_path)
        .map_err(|error| format!("Read Codex config metadata: {error}"))?;
    toml::from_str::<Value>(&contents)
        .map(Some)
        .map_err(|error| format!("Parse Codex config metadata: {error}"))
}

fn insert_server_table(
    servers: &mut BTreeMap<String, NativeMcpServerSnapshot>,
    config: &Value,
    native_scope: &str,
    project_trusted: bool,
) -> Result<(), String> {
    let Some(table) = config.get("mcp_servers").and_then(Value::as_table) else {
        return Ok(());
    };
    for (server_id, definition) in table {
        let definition = definition.as_table();
        let enabled = definition
            .and_then(|table| table.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let native_state = if native_scope == "project" && !project_trusted {
            NativeMcpState::Untrusted
        } else if enabled {
            NativeMcpState::Enabled
        } else {
            NativeMcpState::Disabled
        };
        let known_tools = definition
            .into_iter()
            .flat_map(|table| [table.get("enabled_tools"), table.get("disabled_tools")])
            .flatten()
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let key = McpServerKey::new(AgentHarnessKind::Codex, server_id.to_string())?;
        servers.insert(
            server_id.to_string(),
            NativeMcpServerSnapshot {
                key,
                native_scope: Some(native_scope.to_string()),
                native_state,
                known_tools,
                diagnostic: (native_state == NativeMcpState::Untrusted).then_some(
                    "Codex project MCP configuration is not active because this worktree is not trusted."
                        .to_string(),
                ),
            },
        );
    }
    Ok(())
}
