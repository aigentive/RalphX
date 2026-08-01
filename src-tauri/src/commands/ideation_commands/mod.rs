// Ideation commands module - aggregates all ideation-related submodules

use crate::domain::entities::TaskProposal;
use std::path::{Path, PathBuf};

/// Returns true if the proposal belongs to the local project (not a foreign cross-project proposal).
/// Uses canonicalized path comparison with fallback to raw PathBuf.
pub(crate) fn is_local_proposal(proposal: &TaskProposal, project_dir: &Path) -> bool {
    match &proposal.target_project {
        None => true,
        Some(tp) => {
            let tp_path = std::fs::canonicalize(tp).unwrap_or_else(|_| PathBuf::from(tp));
            tp_path == project_dir
        }
    }
}

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

// Re-export all types
pub use ideation_commands_types::*;

// Re-export all commands
pub use ideation_commands_agent_lanes::*;
pub use ideation_commands_append::*;
#[doc(hidden)]
pub use ideation_commands_apply::apply_proposals_core;
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
mod ideation_commands_apply_tests;
#[cfg(test)]
mod ideation_commands_cross_project_tests;
#[cfg(test)]
mod ideation_commands_orchestrator_tests;
#[cfg(test)]
mod ideation_commands_restart_tests;
