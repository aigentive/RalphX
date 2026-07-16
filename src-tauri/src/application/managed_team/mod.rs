pub mod overlay_resolver;
#[cfg(test)]
mod overlay_resolver_tests;

pub use overlay_resolver::{
    validate_native_team_intent, NativeTeamOverlayError, ResolvedTeamOverlay,
};
