pub mod lifecycle;
pub mod messaging;
mod messaging_delivery;
#[cfg(test)]
mod messaging_tests;
pub mod members;
#[cfg(test)]
mod members_tests;
pub mod overlay_resolver;
#[cfg(test)]
mod overlay_resolver_tests;
pub mod recovery;
#[cfg(test)]
mod recovery_tests;
pub mod service;
#[cfg(test)]
mod service_tests;
pub mod team_prompt_contract;
#[cfg(test)]
mod team_prompt_contract_tests;
pub mod team_prompt;
#[cfg(test)]
mod team_prompt_tests;

pub use lifecycle::{new_coordinator_run_binding, new_team_session};
pub use messaging::{
    ManagedTeamMessageRequest, ManagedTeamMessageSender, ManagedTeamMessageTarget,
};
pub use members::{
    ManagedTeamAssignmentPlan, ManagedTeamAssignmentRequest, ManagedTeamMemberSpec,
    ManagedTeamWorkspaceRequest,
};
pub use overlay_resolver::{
    validate_native_team_intent, NativeTeamOverlayError, ResolvedTeamOverlay,
};
pub use recovery::ManagedTeamStartupBarrier;
pub use service::{ManagedTeamService, ManagedTeamStatus};
pub use team_prompt_contract::{apply_rx_native_team_contract, rx_native_team_contract};
pub use team_prompt::render_team_origin_message;
