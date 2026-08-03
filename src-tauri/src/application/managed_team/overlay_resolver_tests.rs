use super::overlay_resolver::*;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    CoordinationMode, TeamIntent, TeamIntentStrategy, TeamMessageTarget, TeamMessageTargetKind,
};

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
        validate_native_team_intent(Some(&TeamIntent::default()), AgentHarnessKind::Codex).unwrap(),
        None
    );
}

#[test]
fn rx_native_team_resolves_for_standard_harnesses() {
    let intent = TeamIntent::rx_native(Some(TeamIntentStrategy::Execution));

    assert_eq!(
        validate_native_team_intent(Some(&intent), AgentHarnessKind::Claude).unwrap(),
        Some(ResolvedTeamOverlay {
            coordination_mode: CoordinationMode::RxNativeTeam,
            harness: AgentHarnessKind::Claude,
        })
    );
    assert_eq!(
        validate_native_team_intent(Some(&intent), AgentHarnessKind::Codex).unwrap(),
        Some(ResolvedTeamOverlay {
            coordination_mode: CoordinationMode::RxNativeTeam,
            harness: AgentHarnessKind::Codex,
        })
    );
}

#[test]
fn non_team_capabilities_do_not_activate_the_team_overlay() {
    for coordination_mode in [
        CoordinationMode::RxNativeWorkflow,
        CoordinationMode::CodexNativeUltra,
    ] {
        let intent = TeamIntent {
            coordination_mode,
            strategy: None,
        };
        assert_eq!(
            validate_native_team_intent(Some(&intent), AgentHarnessKind::Codex).unwrap(),
            None
        );
    }
}

#[test]
fn rx_native_team_rejects_harness_without_capability() {
    let intent = TeamIntent::rx_native(Some(TeamIntentStrategy::Execution));

    assert!(matches!(
        validate_native_team_intent_with_capabilities(
            Some(&intent),
            AgentHarnessKind::Codex,
            false
        ),
        Err(NativeTeamOverlayError::HarnessUnsupported {
            harness: AgentHarnessKind::Codex
        })
    ));
}

#[test]
fn root_lib_coverage_exercises_team_domain_request_contract() {
    for (mode, value) in [
        (CoordinationMode::Solo, "solo"),
        (CoordinationMode::RxNativeTeam, "rx_native_team"),
        (CoordinationMode::RxNativeWorkflow, "rx_native_workflow"),
        (CoordinationMode::CodexNativeUltra, "codex_native_ultra"),
    ] {
        assert_eq!(mode.to_string(), value);
        assert_eq!(value.parse::<CoordinationMode>().unwrap(), mode);
    }
    assert_eq!(
            "unexpected".parse::<CoordinationMode>().unwrap_err(),
            "Invalid coordination mode 'unexpected'. Valid values: solo, rx_native_team, rx_native_workflow, codex_native_ultra"
        );

    let solo_intent = TeamIntent::default();
    assert!(solo_intent.is_solo());
    assert_eq!(
        serde_json::to_value(&solo_intent).unwrap()["coordinationMode"],
        "solo"
    );

    for (strategy, value) in [
        (TeamIntentStrategy::Research, "research"),
        (TeamIntentStrategy::Debate, "debate"),
        (TeamIntentStrategy::Execution, "execution"),
    ] {
        let intent = TeamIntent::rx_native(Some(strategy));
        let json = serde_json::to_value(&intent).unwrap();
        assert_eq!(json["coordinationMode"], "rx_native_team");
        assert_eq!(json["strategy"], value);
    }

    let no_strategy = TeamIntent::rx_native(None);
    let no_strategy_json = serde_json::to_value(&no_strategy).unwrap();
    assert_eq!(no_strategy_json["coordinationMode"], "rx_native_team");
    assert!(no_strategy_json.get("strategy").is_none());

    for (kind, value) in [
        (TeamMessageTargetKind::Coordinator, "coordinator"),
        (TeamMessageTargetKind::Member, "member"),
        (TeamMessageTargetKind::Broadcast, "broadcast"),
    ] {
        let target = TeamMessageTarget {
            kind,
            member_name: Some("member one".to_string()),
        };
        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(json["kind"], value);
        assert_eq!(json["memberName"], "member one");
    }
}
