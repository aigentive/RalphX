use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// Conversation coordination mode projected to clients.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationMode {
    #[default]
    Solo,
    RxNativeTeam,
    RxNativeWorkflow,
    CodexNativeUltra,
}

impl fmt::Display for CoordinationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Solo => write!(f, "solo"),
            Self::RxNativeTeam => write!(f, "rx_native_team"),
            Self::RxNativeWorkflow => write!(f, "rx_native_workflow"),
            Self::CodexNativeUltra => write!(f, "codex_native_ultra"),
        }
    }
}

impl FromStr for CoordinationMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "solo" => Ok(Self::Solo),
            "rx_native_team" => Ok(Self::RxNativeTeam),
            "rx_native_workflow" => Ok(Self::RxNativeWorkflow),
            "codex_native_ultra" => Ok(Self::CodexNativeUltra),
            other => Err(format!(
                "Invalid coordination mode '{}'. Valid values: solo, rx_native_team, rx_native_workflow, codex_native_ultra",
                other
            )),
        }
    }
}

/// Optional strategy hint for a native team request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamIntentStrategy {
    Research,
    Debate,
    Execution,
}

/// Explicit native team-mode request supplied by the UI or compatibility bridge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamIntent {
    pub coordination_mode: CoordinationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<TeamIntentStrategy>,
}

/// Provider-neutral capability request. `TeamIntent` remains the compatibility
/// name on existing transport surfaces while callers migrate.
pub type CapabilityIntent = TeamIntent;

impl TeamIntent {
    pub fn rx_native(strategy: Option<TeamIntentStrategy>) -> Self {
        Self {
            coordination_mode: CoordinationMode::RxNativeTeam,
            strategy,
        }
    }

    pub fn is_solo(&self) -> bool {
        self.coordination_mode == CoordinationMode::Solo
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageTargetKind {
    Coordinator,
    Member,
    Broadcast,
}

/// Native mailbox target for team messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMessageTarget {
    pub kind: TeamMessageTargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_name: Option<String>,
}
