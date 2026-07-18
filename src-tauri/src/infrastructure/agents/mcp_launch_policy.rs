use std::path::Path;

use crate::domain::agents::{AgentHarnessKind, McpLaunchPolicy};

use super::claude::SpawnableCommand;

pub(crate) fn ensure_no_reserved_native_mcp_collision_at(
    provider: AgentHarnessKind,
    provider_config_root: &Path,
    project_root: Option<&Path>,
) -> Result<(), String> {
    let servers = match provider {
        AgentHarnessKind::Claude => super::claude::mcp_catalog::discover_native_mcp_servers(
            provider_config_root,
            project_root,
        )?,
        AgentHarnessKind::Codex => super::codex::mcp_catalog::discover_native_mcp_servers(
            provider_config_root,
            project_root,
        )?,
    };
    if let Some(collision) = servers
        .into_iter()
        .find(|server| server.key.is_ralphx_owned())
    {
        return Err(format!(
            "Provider-native MCP server '{}' collides with a reserved RalphX server ID; rename or remove the native server before launching",
            collision.key.server_id
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
