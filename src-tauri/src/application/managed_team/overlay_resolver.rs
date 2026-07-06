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
    HarnessUnsupported { harness: AgentHarnessKind },
}

impl NativeTeamOverlayError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "team_mode_disabled",
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
    let Some(team_intent) = team_intent else {
        return Ok(None);
    };
    if team_intent.is_solo() {
        return Ok(None);
    }
    if team_intent.coordination_mode == CoordinationMode::LegacyClaudeTeam {
        if !harness_supports_team_mode(harness) {
            return Err(NativeTeamOverlayError::HarnessUnsupported { harness });
        }
        return Ok(Some(ResolvedTeamOverlay {
            coordination_mode: CoordinationMode::LegacyClaudeTeam,
            harness,
        }));
    }
    if !harness_supports_rx_native_team(harness) {
        return Err(NativeTeamOverlayError::HarnessUnsupported { harness });
    }
    Err(NativeTeamOverlayError::Disabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::TeamIntentStrategy;

    #[test]
    fn native_team_overlay_error_codes_and_messages() {
        let disabled = NativeTeamOverlayError::Disabled;
        assert_eq!(disabled.code(), "team_mode_disabled");
        assert_eq!(
            disabled.to_string(),
            "team_mode_disabled: RX-native team mode is disabled in this build"
        );

        let unsupported = NativeTeamOverlayError::HarnessUnsupported {
            harness: AgentHarnessKind::Codex,
        };
        assert_eq!(unsupported.code(), "harness_unsupported");
        assert_eq!(
            unsupported.to_string(),
            "harness_unsupported: harness 'codex' does not support RX-native team mode"
        );
    }

    #[test]
    fn omitted_or_solo_team_intent_is_noop() {
        assert_eq!(
            validate_native_team_intent(None, AgentHarnessKind::Claude).unwrap(),
            None
        );
        assert_eq!(
            validate_native_team_intent(Some(&TeamIntent::default()), AgentHarnessKind::Codex)
                .unwrap(),
            None
        );
    }

    #[test]
    fn rx_native_team_fails_closed_for_standard_harnesses() {
        let intent = TeamIntent::rx_native(Some(TeamIntentStrategy::Execution));

        assert!(matches!(
            validate_native_team_intent(Some(&intent), AgentHarnessKind::Claude),
            Err(NativeTeamOverlayError::HarnessUnsupported {
                harness: AgentHarnessKind::Claude
            })
        ));
        assert!(matches!(
            validate_native_team_intent(Some(&intent), AgentHarnessKind::Codex),
            Err(NativeTeamOverlayError::HarnessUnsupported {
                harness: AgentHarnessKind::Codex
            })
        ));
    }

    #[test]
    fn legacy_claude_team_intent_is_adapter_overlay() {
        let intent = TeamIntent {
            coordination_mode: CoordinationMode::LegacyClaudeTeam,
            strategy: None,
        };
        let resolved =
            validate_native_team_intent(Some(&intent), AgentHarnessKind::Claude).unwrap();

        assert_eq!(
            resolved,
            Some(ResolvedTeamOverlay {
                coordination_mode: CoordinationMode::LegacyClaudeTeam,
                harness: AgentHarnessKind::Claude,
            })
        );
    }

    #[test]
    fn legacy_claude_team_rejects_non_legacy_harness() {
        let intent = TeamIntent {
            coordination_mode: CoordinationMode::LegacyClaudeTeam,
            strategy: None,
        };

        assert!(matches!(
            validate_native_team_intent(Some(&intent), AgentHarnessKind::Codex),
            Err(NativeTeamOverlayError::HarnessUnsupported {
                harness: AgentHarnessKind::Codex
            })
        ));
    }
}
