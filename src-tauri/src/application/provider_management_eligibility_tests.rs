use std::collections::HashMap;

use chrono::Utc;

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};

use super::harness_runtime_registry::HarnessRuntimeProbe;
use super::provider_management_eligibility::resolve_provider_management_eligibility_with_probes;

fn probe(available: bool) -> HarnessRuntimeProbe {
    HarnessRuntimeProbe {
        binary_path: None,
        binary_found: available,
        probe_succeeded: available,
        available,
        missing_core_exec_features: Vec::new(),
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: false,
        fast_mode_supported_models: Vec::new(),
        error: None,
    }
}

#[test]
fn ready_provider_passes_guard_and_disabled_rows_are_ignored() {
    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.enabled = true;
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = false;
    let probes = HashMap::from([
        (AgentHarnessKind::Claude, probe(true)),
        (AgentHarnessKind::Codex, probe(true)),
    ]);
    let eligibility =
        resolve_provider_management_eligibility_with_probes(&[claude, codex], &probes, Utc::now());

    assert_eq!(eligibility.providers, vec![AgentHarnessKind::Claude]);
    assert!(eligibility.ensure_ready(AgentHarnessKind::Claude).is_ok());
    assert!(eligibility.ensure_ready(AgentHarnessKind::Codex).is_err());
}

#[test]
fn enabled_but_unavailable_provider_is_not_manageable_or_defaulted() {
    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.enabled = true;
    claude.is_default = true;
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    let probes = HashMap::from([
        (AgentHarnessKind::Claude, probe(false)),
        (AgentHarnessKind::Codex, probe(true)),
    ]);

    let eligibility =
        resolve_provider_management_eligibility_with_probes(&[claude, codex], &probes, Utc::now());
    assert_eq!(eligibility.providers, vec![AgentHarnessKind::Codex]);
    assert_eq!(eligibility.default_provider, None);
    assert!(eligibility.ensure_ready(AgentHarnessKind::Claude).is_err());
}

#[test]
fn eligible_default_wins_while_fallback_order_is_deterministic() {
    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.enabled = true;
    claude.is_default = true;
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    let probes = HashMap::from([
        (AgentHarnessKind::Claude, probe(true)),
        (AgentHarnessKind::Codex, probe(true)),
    ]);
    let eligibility =
        resolve_provider_management_eligibility_with_probes(&[claude, codex], &probes, Utc::now());

    assert_eq!(
        eligibility.providers,
        vec![AgentHarnessKind::Claude, AgentHarnessKind::Codex]
    );
    assert_eq!(eligibility.default_provider, Some(AgentHarnessKind::Claude));
}
