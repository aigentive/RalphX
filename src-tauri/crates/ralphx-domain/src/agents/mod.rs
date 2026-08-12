// Agents module - agentic AI client abstraction layer
// This module defines the trait and types for agent clients (Claude, Codex, etc.)
// Implementations live in infrastructure/agents/

pub mod agent_profile;
pub mod agentic_client;
pub mod atlassian_mcp_access;
pub mod capabilities;
pub mod error;
pub mod harness;
pub mod mcp_policy;
pub mod model_registry;
pub mod provider_settings;
pub mod routing_role;
mod routing_role_descriptions;
pub mod types;

// Re-export key types
pub use agent_profile::{
    AgentProfile, AutonomyLevel, BehaviorConfig, ClaudeCodeConfig, ExecutionConfig, IoConfig,
    Model, PermissionMode, ProfileRole,
};
pub use agentic_client::AgenticClient;
pub use atlassian_mcp_access::{
    default_atlassian_access, AtlassianMcpAccess, ATLASSIAN_READ_TOOLS, ATLASSIAN_WRITE_TOOLS,
};
pub use capabilities::{ClientCapabilities, ModelInfo};
pub use error::{AgentError, AgentResult};
pub use harness::{
    default_approval_policy_for_harness, default_sandbox_mode_for_harness,
    generic_harness_lane_defaults, generic_harness_role_defaults, standard_agent_lane_defaults,
    standard_harness_behavior, standard_harness_map, standard_harness_registry, AgentHarnessKind,
    AgentLane, AgentLaneSettings, HarnessBehavior, HarnessEffortStrategy,
    HarnessModelLabelStrategy, HarnessStreamMode, LogicalEffort, ProviderSessionRef,
    StoredAgentLaneSettings,
    StoredWorkspaceReviewRuntimeSettings, WorkspaceReviewRuntimeSettings,
    CLAUDE_DEFAULT_ALLOW_DANGEROUSLY_SKIP_PERMISSIONS, CLAUDE_DEFAULT_DANGEROUSLY_SKIP_PERMISSIONS,
    CLAUDE_DEFAULT_PERMISSION_MODE, CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE,
    DEFAULT_AGENT_HARNESS, STANDARD_AGENT_HARNESSES,
};
pub use model_registry::{
    built_in_agent_models, default_effort_for_provider, default_efforts_for_provider,
    default_model_for_provider, lightweight_model_for_provider, plan_judge_model_for_provider,
    AgentModelDefinition, AgentModelRegistrySnapshot, AgentModelSource,
};
pub use mcp_policy::{
    validate_mcp_identifier, EffectiveMcpServerPolicy, McpLaunchPolicy, McpOverrideState,
    McpPolicyOverride, McpPolicySource, McpRepairStatus, McpServerKey, McpSetupConflictKind,
    McpSetupPreflightFailure, NativeMcpServerSnapshot, NativeMcpState,
    MCP_SETUP_PREFLIGHT_MARKER, RALPHX_MCP_SERVER_IDS,
};
pub use provider_settings::{AgentProviderCliManagementMode, AgentProviderSettings};
pub use routing_role::{
    ManualRoleDefault, ManualRoleRuntimeOverride, ManualServiceTier, RoutingRole,
    RoutingRoleFamily, RoutingRoleMetadata, StoredManualRoleDefault, ROUTING_ROLE_COUNT,
    ROUTING_ROLE_FAMILIES, ROUTING_ROLES,
};
pub use types::{
    AgentConfig, AgentHandle, AgentOutput, AgentResponse, AgentRole, ClientType, ResponseChunk,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod dependency_tests;

#[cfg(test)]
#[path = "routing_role_tests.rs"]
mod routing_role_tests;

#[cfg(test)]
#[path = "mcp_policy_tests.rs"]
mod mcp_policy_tests;

#[cfg(test)]
#[path = "atlassian_mcp_access_tests.rs"]
mod atlassian_mcp_access_tests;
