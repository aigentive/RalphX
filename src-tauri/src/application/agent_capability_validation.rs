use std::fmt;

use crate::application::agent_capability_gate::AgentCapabilityGate;
use crate::application::harness_runtime_registry::probe_harness;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::CoordinationMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCapabilityError {
    TeamDisabled,
    WorkflowsDisabled,
    LegacyReadOnly,
    UltraRequiresCodex,
    UltraUnavailable,
}

impl fmt::Display for AgentCapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TeamDisabled => write!(
                f,
                "Team is disabled. Enable it in Settings > Capabilities or switch this conversation to Defaults."
            ),
            Self::WorkflowsDisabled => write!(
                f,
                "Workflows are disabled. Enable them in Settings > Capabilities or switch this conversation to Defaults."
            ),
            Self::LegacyReadOnly => write!(
                f,
                "Legacy Claude team mode is read-only; switch this conversation to Defaults or Team."
            ),
            Self::UltraRequiresCodex => {
                write!(f, "Codex Ultra is available only with the Codex provider.")
            }
            Self::UltraUnavailable => write!(
                f,
                "Codex Ultra is unavailable for the selected model and Codex account."
            ),
        }
    }
}

impl std::error::Error for AgentCapabilityError {}

pub fn validate_agent_capability(
    mode: CoordinationMode,
    harness: AgentHarnessKind,
    gate: &AgentCapabilityGate,
    codex_ultra_supported: Option<bool>,
) -> Result<(), AgentCapabilityError> {
    match mode {
        CoordinationMode::Solo => Ok(()),
        CoordinationMode::LegacyClaudeTeam => Err(AgentCapabilityError::LegacyReadOnly),
        CoordinationMode::RxNativeTeam if !gate.team_enabled() => {
            Err(AgentCapabilityError::TeamDisabled)
        }
        CoordinationMode::RxNativeTeam => Ok(()),
        CoordinationMode::RxNativeWorkflow if !gate.workflows_enabled() => {
            Err(AgentCapabilityError::WorkflowsDisabled)
        }
        CoordinationMode::RxNativeWorkflow => Ok(()),
        CoordinationMode::CodexNativeUltra if harness != AgentHarnessKind::Codex => {
            Err(AgentCapabilityError::UltraRequiresCodex)
        }
        CoordinationMode::CodexNativeUltra if codex_ultra_supported != Some(true) => {
            Err(AgentCapabilityError::UltraUnavailable)
        }
        CoordinationMode::CodexNativeUltra => Ok(()),
    }
}

pub fn codex_ultra_support_for_model(
    harness: AgentHarnessKind,
    model: Option<&str>,
) -> Option<bool> {
    if harness != AgentHarnessKind::Codex {
        return None;
    }
    let model = model?.trim();
    if model.is_empty() {
        return None;
    }
    let probe = probe_harness(AgentHarnessKind::Codex);
    probe.probe_succeeded.then(|| {
        probe
            .ultra_supported_models
            .iter()
            .any(|supported| supported == model)
    })
}
