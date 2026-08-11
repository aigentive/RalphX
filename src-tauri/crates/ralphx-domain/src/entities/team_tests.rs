use super::*;

#[test]
fn coordination_mode_serializes_snake_case() {
    let json = serde_json::to_string(&CoordinationMode::RxNativeTeam).unwrap();
    assert_eq!(json, "\"rx_native_team\"");

    assert!(serde_json::from_str::<CoordinationMode>("\"legacy_claude_team\"").is_err());
}

#[test]
fn coordination_mode_display_and_from_str_cover_all_modes() {
    for (mode, value) in [
        (CoordinationMode::Solo, "solo"),
        (CoordinationMode::RxNativeTeam, "rx_native_team"),
        (CoordinationMode::RxNativeWorkflow, "rx_native_workflow"),
        (CoordinationMode::CodexNativeUltra, "codex_native_ultra"),
    ] {
        assert_eq!(mode.to_string(), value);
        assert_eq!(value.parse::<CoordinationMode>().unwrap(), mode);
    }

    let error = "unknown".parse::<CoordinationMode>().unwrap_err();
    assert!(error.contains("Invalid coordination mode 'unknown'"));
}

#[test]
fn team_intent_serializes_camel_case_request_shape() {
    let intent = TeamIntent::rx_native(Some(TeamIntentStrategy::Research));
    let json = serde_json::to_value(&intent).unwrap();

    assert_eq!(json["coordinationMode"], "rx_native_team");
    assert_eq!(json["strategy"], "research");
}

#[test]
fn team_intent_defaults_to_solo_and_serializes_all_strategies() {
    let intent = TeamIntent::default();
    assert!(intent.is_solo());
    assert_eq!(
        serde_json::to_value(&intent).unwrap()["coordinationMode"],
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
}

#[test]
fn team_message_target_serializes_member_name() {
    let target = TeamMessageTarget {
        kind: TeamMessageTargetKind::Member,
        member_name: Some("worker one".to_string()),
    };
    let json = serde_json::to_value(&target).unwrap();

    assert_eq!(json["kind"], "member");
    assert_eq!(json["memberName"], "worker one");
}

#[test]
fn team_message_target_serializes_coordinator_and_broadcast_shapes() {
    let coordinator = TeamMessageTarget {
        kind: TeamMessageTargetKind::Coordinator,
        member_name: None,
    };
    let coordinator_json = serde_json::to_value(&coordinator).unwrap();
    assert_eq!(coordinator_json["kind"], "coordinator");
    assert!(coordinator_json.get("memberName").is_none());

    let broadcast = TeamMessageTarget {
        kind: TeamMessageTargetKind::Broadcast,
        member_name: None,
    };
    let broadcast_json = serde_json::to_value(&broadcast).unwrap();
    assert_eq!(broadcast_json["kind"], "broadcast");
    assert!(broadcast_json.get("memberName").is_none());
}
