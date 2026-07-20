use std::fmt;

use crate::application::chat_service::{
    harness_supports_rx_native_team, harness_supports_team_mode,
};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{CoordinationMode, TeamIntent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTeamOverlay {
    pub coordination_mode: CoordinationMode,
    pub harness: AgentHarnessKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeTeamOverlayError {
    Disabled,
    LegacyReadOnly,
    HarnessUnsupported { harness: AgentHarnessKind },
}

impl NativeTeamOverlayError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "team_mode_disabled",
            Self::LegacyReadOnly => "legacy_team_read_only",
            Self::HarnessUnsupported { .. } => "harness_unsupported",
        }
    }
}

impl fmt::Display for NativeTeamOverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(
                f,
                "team_mode_disabled: RX-native team mode is disabled in this build"
            ),
            Self::LegacyReadOnly => write!(
                f,
                "legacy_team_read_only: legacy Claude team mode is read-only; use Team mode"
            ),
            Self::HarnessUnsupported { harness } => write!(
                f,
                "harness_unsupported: harness '{}' does not support RX-native team mode",
                harness
            ),
        }
    }
}

impl std::error::Error for NativeTeamOverlayError {}

pub fn validate_native_team_intent(
    team_intent: Option<&TeamIntent>,
    harness: AgentHarnessKind,
) -> Result<Option<ResolvedTeamOverlay>, NativeTeamOverlayError> {
    validate_native_team_intent_with_capabilities(
        team_intent,
        harness,
        harness_supports_team_mode(harness),
        harness_supports_rx_native_team(harness),
    )
}

pub(super) fn validate_native_team_intent_with_capabilities(
    team_intent: Option<&TeamIntent>,
    harness: AgentHarnessKind,
    _supports_legacy_team: bool,
    supports_rx_native_team: bool,
) -> Result<Option<ResolvedTeamOverlay>, NativeTeamOverlayError> {
    let Some(team_intent) = team_intent else {
        return Ok(None);
    };
    if team_intent.is_solo() {
        return Ok(None);
    }
    if team_intent.coordination_mode == CoordinationMode::LegacyClaudeTeam {
        return Err(NativeTeamOverlayError::LegacyReadOnly);
    }
    if team_intent.coordination_mode != CoordinationMode::RxNativeTeam {
        return Ok(None);
    }
    if !supports_rx_native_team {
        return Err(NativeTeamOverlayError::HarnessUnsupported { harness });
    }
    Ok(Some(ResolvedTeamOverlay {
        coordination_mode: CoordinationMode::RxNativeTeam,
        harness,
    }))
}
