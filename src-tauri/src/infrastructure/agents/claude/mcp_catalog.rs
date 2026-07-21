use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::domain::agents::{
    AgentHarnessKind, McpServerKey, NativeMcpServerSnapshot, NativeMcpState,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyClaudeRegistration {
    NotPresent,
    ExactHistorical,
    AmbiguousCollision,
}

pub(crate) fn discover_native_mcp_servers(
    home_dir: &Path,
    project_root: Option<&Path>,
) -> Result<Vec<NativeMcpServerSnapshot>, String> {
    let home_dir = validate_absolute_non_root_path(home_dir, "Claude config root")
        .map_err(|error| error.to_string())?;
    let user_state_path = home_dir.join(".claude.json");
    let user_state = read_fixed_json(&home_dir, &user_state_path, Path::new(".claude.json"))?;
    let mut servers = BTreeMap::<String, NativeMcpServerSnapshot>::new();
    if let Some(value) = user_state.as_ref() {
        insert_server_map(
            &mut servers,
            value.get("mcpServers"),
            "user",
            NativeMcpState::Enabled,
        )?;
    }

    if let Some(project_root) = project_root {
        let project_root = validate_absolute_non_root_path(project_root, "Claude project root")
            .map_err(|error| error.to_string())?;
        let project_state = user_state
            .as_ref()
            .and_then(|value| value.get("projects"))
            .and_then(Value::as_object)
            .and_then(|projects| projects.get(project_root.to_string_lossy().as_ref()));
        insert_server_map(
            &mut servers,
            project_state.and_then(|value| value.get("mcpServers")),
            "local",
            NativeMcpState::Enabled,
        )?;

        let project_file = project_root.join(".mcp.json");
        if let Some(project_config) =
            read_fixed_json(&project_root, &project_file, Path::new(".mcp.json"))?
        {
            let enabled = string_set(project_state, "enabledMcpjsonServers");
            let disabled = string_set(project_state, "disabledMcpjsonServers");
            if let Some(definitions) = project_config.get("mcpServers").and_then(Value::as_object) {
                for server_id in definitions.keys() {
                    let native_state = if disabled.contains(server_id) {
                        NativeMcpState::Disabled
                    } else if enabled.contains(server_id) {
                        NativeMcpState::Enabled
                    } else {
                        NativeMcpState::PendingApproval
                    };
                    insert_snapshot(&mut servers, server_id, "project", native_state)?;
                }
            }
        }
    }

    Ok(servers.into_values().collect())
}

pub(crate) fn classify_legacy_user_registration(
    home_dir: &Path,
    app_data_dir: &Path,
) -> Result<LegacyClaudeRegistration, String> {
    let home_dir = validate_absolute_non_root_path(home_dir, "Claude config root")
        .map_err(|error| error.to_string())?;
    let app_data_dir = validate_absolute_non_root_path(app_data_dir, "RalphX app data root")
        .map_err(|error| error.to_string())?;
    let user_state_path = home_dir.join(".claude.json");
    let Some(user_state) = read_fixed_json(&home_dir, &user_state_path, Path::new(".claude.json"))?
    else {
        return Ok(LegacyClaudeRegistration::NotPresent);
    };
    let Some(registration) = user_state
        .get("mcpServers")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get("ralphx"))
    else {
        return Ok(LegacyClaudeRegistration::NotPresent);
    };

    let exact = matches_historical_registration(registration, &app_data_dir);
    Ok(if exact {
        LegacyClaudeRegistration::ExactHistorical
    } else {
        LegacyClaudeRegistration::AmbiguousCollision
    })
}

fn matches_historical_registration(registration: &Value, app_data_dir: &Path) -> bool {
    let resolved_node = super::node_utils::find_node_binary();
    let accepted_node_commands = [Path::new("node"), resolved_node.as_path()];
    let template_script =
        app_data_dir.join("generated/claude-plugin/ralphx-mcp-server/build/index.js");
    let trace_enabled_scripts = ["release", "debug"].map(|profile| {
        app_data_dir
            .join("generated")
            .join(profile)
            .join("claude-plugin/ralphx-mcp-server/build/index.js")
    });

    accepted_node_commands.iter().any(|node_command| {
        historical_script_is_contained(app_data_dir, &template_script)
            && registration
                == &serde_json::json!({
                    "type": "stdio",
                    "command": node_command,
                    "args": [template_script]
                })
    }) || trace_enabled_scripts.into_iter().any(|script_path| {
        historical_script_is_contained(app_data_dir, &script_path)
            && accepted_node_commands.iter().any(|node_command| {
                registration
                    == &serde_json::json!({
                        "type": "stdio",
                        "command": node_command,
                        "args": [
                            script_path,
                            "--trace-dir",
                            app_data_dir.join("logs/mcp-proxy"),
                        ]
                    })
            })
    })
}

fn historical_script_is_contained(app_data_dir: &Path, script_path: &Path) -> bool {
    let Ok(canonical_root) = app_data_dir.canonicalize() else {
        return false;
    };
    let Ok(canonical_script) = script_path.canonicalize() else {
        return false;
    };
    canonical_script.starts_with(canonical_root) && canonical_script.is_file()
}

fn read_fixed_json(
    owned_root: &Path,
    path: &Path,
    expected_relative: &Path,
) -> Result<Option<Value>, String> {
    if path.strip_prefix(owned_root).ok() != Some(expected_relative) {
        return Err(format!(
            "Claude config path is not fixed: {}",
            path.display()
        ));
    }
    // codeql[rust/path-injection]
    if !path.exists() {
        return Ok(None);
    }
    let canonical_root = owned_root
        .canonicalize()
        .map_err(|error| format!("Resolve Claude config root: {error}"))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| format!("Resolve Claude config: {error}"))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(format!(
            "Claude config escapes owned root: {}",
            path.display()
        ));
    }
    // codeql[rust/path-injection]
    let contents = fs::read_to_string(canonical_path)
        .map_err(|error| format!("Read Claude config metadata: {error}"))?;
    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|error| format!("Parse Claude config metadata: {error}"))
}

fn insert_server_map(
    servers: &mut BTreeMap<String, NativeMcpServerSnapshot>,
    definitions: Option<&Value>,
    native_scope: &str,
    native_state: NativeMcpState,
) -> Result<(), String> {
    let Some(definitions) = definitions.and_then(Value::as_object) else {
        return Ok(());
    };
    for server_id in definitions.keys() {
        insert_snapshot(servers, server_id, native_scope, native_state)?;
    }
    Ok(())
}

fn insert_snapshot(
    servers: &mut BTreeMap<String, NativeMcpServerSnapshot>,
    server_id: &str,
    native_scope: &str,
    native_state: NativeMcpState,
) -> Result<(), String> {
    let key = McpServerKey::new(AgentHarnessKind::Claude, server_id.to_string())?;
    servers.insert(
        server_id.to_string(),
        NativeMcpServerSnapshot {
            key,
            native_scope: Some(native_scope.to_string()),
            native_state,
            known_tools: Vec::new(),
            diagnostic: None,
        },
    );
    Ok(())
}

fn string_set(parent: Option<&Value>, key: &str) -> BTreeSet<String> {
    parent
        .and_then(|value| value.get(key))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}
