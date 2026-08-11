// Ideation commands module - aggregates all ideation-related submodules

mod ideation_commands_agent_lanes;
mod ideation_commands_append;
mod ideation_commands_apply;
mod ideation_commands_chat;
mod ideation_commands_cross_project;
mod ideation_commands_dependencies;
pub mod ideation_commands_effort;
pub mod ideation_commands_export;
mod ideation_commands_harness_availability;
pub mod ideation_commands_model;
mod ideation_commands_orchestrator;
mod ideation_commands_proposals;
mod ideation_commands_restart;
mod ideation_commands_session;
mod ideation_commands_types;

// Re-export types from application ideation_apply_service (descended from commands layer)
pub use crate::application::ideation_apply_service::{
    apply_pending_proposals_core, apply_proposals_core, ApplyProposalsInput,
    ApplyProposalsResult, TaskProposalResponse,
};
pub(crate) use crate::application::ideation_apply_service::is_local_proposal;

// Re-export all types
pub use ideation_commands_types::*;

// Re-export all commands
pub use ideation_commands_agent_lanes::*;
pub use ideation_commands_append::*;
pub use ideation_commands_apply::*;
pub use ideation_commands_chat::*;
pub use ideation_commands_cross_project::*;
pub use ideation_commands_dependencies::*;
pub use ideation_commands_effort::*;
pub use ideation_commands_export::*;
pub use ideation_commands_harness_availability::*;
pub use ideation_commands_model::*;
pub use ideation_commands_orchestrator::*;
pub use ideation_commands_proposals::*;
#[doc(hidden)]
pub use ideation_commands_restart::restart_ideation_implementation_core;
pub use ideation_commands_restart::*;
#[doc(hidden)]
pub use ideation_commands_session::create_ideation_session_impl;
pub use ideation_commands_session::*;

// Re-export helper function for tests
#[doc(hidden)]
pub use ideation_commands_dependencies::analyze_dependencies_for_session;
pub use ideation_commands_dependencies::build_dependency_graph;

#[cfg(test)]
mod ideation_commands_append_tests;
#[cfg(test)]
mod ideation_commands_cross_project_tests;
#[cfg(test)]
mod ideation_commands_apply_tests;
#[cfg(test)]
mod ideation_commands_orchestrator_tests;
#[cfg(test)]
mod ideation_commands_restart_tests;
