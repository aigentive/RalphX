use std::fmt;

use crate::application::agent_capability_gate::AgentCapabilityGate;
use crate::application::harness_runtime_registry::probe_harness;
use crate::domain::agents::{AgentHarnessKind, ManualRoleDefault, ManualServiceTier};
use crate::domain::entities::CoordinationMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCapabilityError {
    TeamDisabled,
    WorkflowsDisabled,
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

pub fn codex_fast_support_for_model(
    harness: AgentHarnessKind,
    model: Option<&str>,
) -> Option<bool> {
    if harness != AgentHarnessKind::Codex {
        return None;
    }
    let probe = probe_harness(AgentHarnessKind::Codex);
    Some(codex_fast_support_for_probe(model, &probe))
}

pub(crate) fn codex_fast_support_for_probe(
    model: Option<&str>,
    probe: &crate::application::harness_runtime_registry::HarnessRuntimeProbe,
) -> bool {
    if !probe.probe_succeeded || !probe.supports_fast_mode {
        return false;
    }
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return true;
    };
    probe.fast_mode_supported_models.is_empty()
        || probe
            .fast_mode_supported_models
            .iter()
            .any(|supported| supported == model)
}

pub fn validate_manual_role_runtime_capabilities(
    value: &ManualRoleDefault,
    gate: &AgentCapabilityGate,
) -> Result<(), String> {
    if let Some(mode) = value.coordination_mode {
        validate_agent_capability(
            mode,
            value.harness,
            gate,
            codex_ultra_support_for_model(value.harness, value.model.as_deref()),
        )
        .map_err(|error| error.to_string())?;
    }
    if value.service_tier != ManualServiceTier::Fast {
        return Ok(());
    }
    match codex_fast_support_for_model(value.harness, value.model.as_deref()) {
        Some(true) => Ok(()),
        Some(false) if value.harness == AgentHarnessKind::Codex => {
            let model = value
                .model
                .as_deref()
                .map(str::trim)
                .filter(|model| !model.is_empty());
            if let Some(model) = model {
                let probe = probe_harness(AgentHarnessKind::Codex);
                if probe.supports_fast_mode && !probe.fast_mode_supported_models.is_empty() {
                    return Err(format!(
                        "Codex Fast mode is not available for model {model}."
                    ));
                }
            }
            Err(
                "Codex Fast mode is not supported by the selected Codex CLI or model catalog."
                    .to_string(),
            )
        }
        _ => Err("Fast speed requires the Codex provider".to_string()),
    }
}
