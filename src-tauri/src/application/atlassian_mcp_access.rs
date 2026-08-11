//! Single authority for the Atlassian MCP tier a run actually gets.
//!
//! The tier is derived, never persisted: spawn wiring uses it to decide which
//! tools an agent can see, and the HTTP handlers re-derive it per request so a
//! lowered role or a disabled integration takes effect immediately for
//! in-flight sessions.
//!
//! Every failure path resolves to [`AtlassianMcpAccess::None`]. A repository
//! error, an unresolvable project, or an absent routing role must never widen
//! access (fail-closed progress reads).

use std::path::Path;

use crate::domain::agents::{AtlassianMcpAccess, RoutingRole};

use super::atlassian_integration_service::AtlassianIntegrationService;
use super::manual_role_default_service::ManualRoleDefaultService;

/// Resolve the effective Atlassian MCP tier for a routing role.
///
/// Returns [`AtlassianMcpAccess::None`] unless the Atlassian integration is
/// usable (enabled **and** validated) and a routing role is known.
pub async fn effective_atlassian_mcp_access(
    integration: &AtlassianIntegrationService,
    role_defaults: &ManualRoleDefaultService,
    role: Option<RoutingRole>,
    project_id: Option<&str>,
    project_root: Option<&Path>,
) -> AtlassianMcpAccess {
    // Without an authoritative routing role there is nothing to authorize
    // against, so deny rather than guess.
    let Some(role) = role else {
        return AtlassianMcpAccess::None;
    };
    if !integration.is_usable().await {
        return AtlassianMcpAccess::None;
    }
    role_defaults
        .resolve_atlassian_access(project_id, project_root, role)
        .await
        .unwrap_or(AtlassianMcpAccess::None)
}

/// Bare snake_case MCP tool names to inject into a spawn for this role.
///
/// Returns an empty vector for [`AtlassianMcpAccess::None`], which keeps the
/// spawn-time allowlist untouched.
pub async fn atlassian_mcp_tools_for_spawn(
    integration: &AtlassianIntegrationService,
    role_defaults: &ManualRoleDefaultService,
    role: Option<RoutingRole>,
    project_id: Option<&str>,
    project_root: Option<&Path>,
) -> Vec<String> {
    effective_atlassian_mcp_access(integration, role_defaults, role, project_id, project_root)
        .await
        .granted_tools()
        .into_iter()
        .map(str::to_string)
        .collect()
}
