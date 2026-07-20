use std::path::Path;

use crate::domain::agents::{AgentHarnessKind, McpLaunchPolicy, McpSetupPreflightFailure};

use super::claude::SpawnableCommand;

#[derive(Debug)]
pub(crate) enum McpLaunchPreflightError {
    Catalog,
    Collision(McpSetupPreflightFailure),
}

impl McpLaunchPreflightError {
    pub(crate) fn safe_message(&self) -> String {
        match self {
            Self::Catalog => "MCP provider configuration could not be read safely".to_string(),
            Self::Collision(failure) => failure.to_start_error_marker(),
        }
    }

    #[cfg(test)]
    pub(crate) fn collision(&self) -> Option<&McpSetupPreflightFailure> {
        match self {
            Self::Collision(failure) => Some(failure),
            Self::Catalog => None,
        }
    }
}

pub(crate) fn ensure_no_reserved_native_mcp_collision_at(
    provider: AgentHarnessKind,
    provider_config_root: &Path,
    project_root: Option<&Path>,
) -> Result<(), McpLaunchPreflightError> {
    let servers = match provider {
        AgentHarnessKind::Claude => super::claude::mcp_catalog::discover_native_mcp_servers(
            provider_config_root,
            project_root,
        )
        .map_err(|_| McpLaunchPreflightError::Catalog)?,
        AgentHarnessKind::Codex => super::codex::mcp_catalog::discover_native_mcp_servers(
            provider_config_root,
            project_root,
        )
        .map_err(|_| McpLaunchPreflightError::Catalog)?,
    };
    if let Some(collision) = servers
        .into_iter()
        .find(|server| server.key.is_ralphx_owned())
    {
        return Err(McpLaunchPreflightError::Collision(
            McpSetupPreflightFailure::ambiguous(
                provider,
                collision.key.server_id,
                collision.native_scope,
            ),
        ));
    }
    Ok(())
}

pub fn apply_mcp_launch_policy(
    command: &mut SpawnableCommand,
    provider: AgentHarnessKind,
    policy: &McpLaunchPolicy,
) {
    match provider {
        AgentHarnessKind::Claude => {
            let denied = policy.claude_disallowed_tools();
            if !denied.is_empty() {
                command.arg("--disallowedTools").arg(&denied.join(","));
            }
        }
        AgentHarnessKind::Codex => {
            for config in policy.codex_config_overrides() {
                command.arg("-c").arg(&config);
            }
        }
    }
}
