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
use std::sync::Arc;

use crate::domain::agents::{AtlassianMcpAccess, RoutingRole};
use crate::domain::entities::{AgentRunId, ProjectId};
use crate::domain::repositories::{AgentRunRepository, ProjectRepository};

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

/// Resolve the Atlassian MCP tool grant for a resumed/recovered spawn.
///
/// Resume, queue, and recovery seams do not re-derive a routing role (that
/// would risk drifting from the tier the originating spawn actually
/// resolved); instead they read the **persisted** `routing_role`/`project_id`
/// off the [`AgentRun`](crate::domain::entities::AgentRun) row being
/// continued. A missing run, missing role, or missing service all resolve to
/// an empty grant (fail-closed).
pub async fn atlassian_mcp_tools_for_resumed_run(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    project_repo: &Arc<dyn ProjectRepository>,
    integration: Option<&Arc<AtlassianIntegrationService>>,
    role_defaults: Option<&Arc<ManualRoleDefaultService>>,
    agent_run_id: Option<&str>,
) -> Vec<String> {
    let (Some(integration), Some(role_defaults), Some(agent_run_id)) =
        (integration, role_defaults, agent_run_id)
    else {
        return Vec::new();
    };
    let Ok(Some(run)) = agent_run_repo
        .get_by_id(&AgentRunId::from_string(agent_run_id.to_string()))
        .await
    else {
        return Vec::new();
    };
    let Some(role) = run.routing_role else {
        return Vec::new();
    };
    let project_root = match run.project_id.as_deref() {
        Some(project_id) => project_repo
            .get_by_id(&ProjectId::from_string(project_id.to_string()))
            .await
            .ok()
            .flatten()
            .map(|project| std::path::PathBuf::from(project.working_directory)),
        None => None,
    };
    atlassian_mcp_tools_for_spawn(
        integration,
        role_defaults,
        Some(role),
        run.project_id.as_deref(),
        project_root.as_deref(),
    )
    .await
}
