pub mod lifecycle;
pub mod overlay_resolver;
pub mod recovery;
pub mod service;
pub mod team_prompt_contract;
#[cfg(test)]
mod overlay_resolver_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod team_prompt_contract_tests;

pub use lifecycle::{new_coordinator_run_binding, new_team_session};
pub use overlay_resolver::{
    validate_native_team_intent, NativeTeamOverlayError, ResolvedTeamOverlay,
};
pub use recovery::ManagedTeamStartupBarrier;
pub use service::{ManagedTeamService, ManagedTeamStatus};
pub use team_prompt_contract::{apply_rx_native_team_contract, rx_native_team_contract};
