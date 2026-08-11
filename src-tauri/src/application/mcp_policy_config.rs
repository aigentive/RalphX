use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Deserialize;

use crate::domain::agents::{
    validate_mcp_identifier, AgentHarnessKind, McpOverrideState, McpPolicyOverride, McpServerKey,
};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

const MAX_POLICY_DIAGNOSTICS: usize = 20;

#[derive(Debug, Default)]
pub struct McpPolicyConfigSnapshot {
    pub policies: HashMap<McpServerKey, McpPolicyOverride>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    #[serde(default)]
    mcp: RawMcp,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMcp {
    #[serde(default)]
    providers: HashMap<AgentHarnessKind, RawProvider>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    #[serde(default)]
    servers: BTreeMap<String, RawServer>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawServer {
    #[serde(default)]
    state: McpOverrideState,
    #[serde(default)]
    tools: BTreeMap<String, McpOverrideState>,
}

pub fn load_mcp_policy_file(
    owned_root: &Path,
    policy_path: &Path,
    project_id: Option<&str>,
) -> AppResult<McpPolicyConfigSnapshot> {
    let (owned_root, policy_path) = validate_mcp_policy_path(owned_root, policy_path)?;

    // codeql[rust/path-injection]
    if !policy_path.exists() {
        return Ok(McpPolicyConfigSnapshot::default());
    }

    let canonical_root = owned_root.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to resolve MCP policy root {}: {error}",
            owned_root.display()
        ))
    })?;
    let canonical_policy = policy_path.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to resolve MCP policy {}: {error}",
            policy_path.display()
        ))
    })?;
    if !canonical_policy.starts_with(&canonical_root) {
        return Err(AppError::Validation(format!(
            "MCP policy escapes its owned root: {}",
            policy_path.display()
        )));
    }

    // codeql[rust/path-injection]
    let contents = fs::read_to_string(&canonical_policy).map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to read MCP policy {}: {error}",
            policy_path.display()
        ))
    })?;
    parse_mcp_policy(&contents, project_id)
}

fn validate_mcp_policy_path(
    owned_root: &Path,
    policy_path: &Path,
) -> AppResult<(PathBuf, PathBuf)> {
    let owned_root = validate_absolute_non_root_path(owned_root, "MCP policy root")?;
    let policy_path = validate_absolute_non_root_path(policy_path, "MCP policy")?;
    let relative = policy_path.strip_prefix(&owned_root).map_err(|_| {
        AppError::Validation(format!(
            "MCP policy escapes its owned root: {}",
            policy_path.display()
        ))
    })?;
    if relative != Path::new("mcp.yaml") && relative != Path::new(".ralphx").join("mcp.yaml") {
        return Err(AppError::Validation(format!(
            "MCP policy must use mcp.yaml under its owned root: {}",
            policy_path.display()
        )));
    }
    Ok((owned_root, policy_path))
}

fn parse_mcp_policy(
    contents: &str,
    project_id: Option<&str>,
) -> AppResult<McpPolicyConfigSnapshot> {
    let raw = serde_yaml::from_str::<RawDocument>(contents).map_err(|error| {
        AppError::Validation(format!("Failed to parse MCP policy YAML: {error}"))
    })?;
    let mut snapshot = McpPolicyConfigSnapshot::default();
    for (provider, provider_policy) in raw.mcp.providers {
        for (server_id, server) in provider_policy.servers {
            if let Err(error) = validate_mcp_identifier("server", &server_id) {
                push_diagnostic(&mut snapshot.diagnostics, error);
                continue;
            }
            let mut tool_states = BTreeMap::new();
            for (tool_name, state) in server.tools {
                match validate_mcp_identifier("tool", &tool_name) {
                    Ok(()) => {
                        tool_states.insert(tool_name, state);
                    }
                    Err(error) => push_diagnostic(&mut snapshot.diagnostics, error),
                }
            }
            let key = McpServerKey::new(provider, server_id).map_err(AppError::Validation)?;
            let policy = McpPolicyOverride {
                project_id: project_id.map(str::to_string),
                key: key.clone(),
                server_state: server.state,
                tool_states,
                updated_at: Utc::now(),
            };
            match policy.validate() {
                Ok(()) => {
                    snapshot.policies.insert(key, policy);
                }
                Err(error) => push_diagnostic(&mut snapshot.diagnostics, error),
            }
        }
    }
    Ok(snapshot)
}

fn push_diagnostic(diagnostics: &mut Vec<String>, diagnostic: String) {
    if diagnostics.len() < MAX_POLICY_DIAGNOSTICS {
        diagnostics.push(diagnostic);
    }
}
