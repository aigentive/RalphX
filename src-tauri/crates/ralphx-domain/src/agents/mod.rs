// Agents module - agentic AI client abstraction layer
// This module defines the trait and types for agent clients (Claude, Codex, etc.)
// Implementations live in infrastructure/agents/

pub mod agent_profile;
pub mod agentic_client;
pub mod capabilities;
pub mod error;
pub mod harness;
pub mod model_registry;
pub mod types;

// Re-export key types
pub use agent_profile::{
    AgentProfile, AutonomyLevel, BehaviorConfig, ClaudeCodeConfig, ExecutionConfig, IoConfig,
    Model, PermissionMode, ProfileRole,
};
pub use agentic_client::AgenticClient;
pub use capabilities::{ClientCapabilities, ModelInfo};
pub use error::{AgentError, AgentResult};
pub use harness::{
    generic_harness_lane_defaults, standard_agent_lane_defaults, standard_harness_behavior,
    standard_harness_map, standard_harness_registry, AgentHarnessKind, AgentLane,
    AgentLaneSettings, HarnessBehavior, HarnessEffortStrategy, HarnessModelLabelStrategy,
    HarnessStreamMode, LogicalEffort, ProviderSessionRef, StoredAgentLaneSettings,
    DEFAULT_AGENT_HARNESS, STANDARD_AGENT_HARNESSES,
};
pub use model_registry::{
    built_in_agent_models, default_effort_for_provider, default_efforts_for_provider,
    default_model_for_provider, lightweight_model_for_provider, AgentModelDefinition,
    AgentModelRegistrySnapshot, AgentModelSource,
};
pub use types::{
    AgentConfig, AgentHandle, AgentOutput, AgentResponse, AgentRole, ClientType, ResponseChunk,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod dependency_tests;
