use super::*;

#[test]
fn coordination_mode_serializes_snake_case() {
    let json = serde_json::to_string(&CoordinationMode::RxNativeTeam).unwrap();
    assert_eq!(json, "\"rx_native_team\"");

    let parsed: CoordinationMode = serde_json::from_str("\"legacy_claude_team\"").unwrap();
    assert_eq!(parsed, CoordinationMode::LegacyClaudeTeam);
}

#[test]
fn coordination_mode_display_and_from_str_cover_all_modes() {
    for (mode, value) in [
        (CoordinationMode::Solo, "solo"),
        (CoordinationMode::LegacyClaudeTeam, "legacy_claude_team"),
        (CoordinationMode::RxNativeTeam, "rx_native_team"),
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
fn team_message_target_serializes_native_ids() {
    let target = TeamMessageTarget {
        kind: TeamMessageTargetKind::Member,
        team_id: Some("team-1".to_string()),
        team_member_id: Some("member-1".to_string()),
        conversation_id: Some("conversation-1".to_string()),
    };
    let json = serde_json::to_value(&target).unwrap();

    assert_eq!(json["kind"], "member");
    assert_eq!(json["teamId"], "team-1");
    assert_eq!(json["teamMemberId"], "member-1");
    assert_eq!(json["conversationId"], "conversation-1");
}

#[test]
fn team_message_target_serializes_coordinator_and_broadcast_shapes() {
    let coordinator = TeamMessageTarget {
        kind: TeamMessageTargetKind::Coordinator,
        team_id: Some("team-1".to_string()),
        team_member_id: None,
        conversation_id: None,
    };
    let coordinator_json = serde_json::to_value(&coordinator).unwrap();
    assert_eq!(coordinator_json["kind"], "coordinator");
    assert_eq!(coordinator_json["teamId"], "team-1");
    assert!(coordinator_json.get("teamMemberId").is_none());
    assert!(coordinator_json.get("conversationId").is_none());

    let broadcast = TeamMessageTarget {
        kind: TeamMessageTargetKind::Broadcast,
        team_id: None,
        team_member_id: None,
        conversation_id: Some("conversation-1".to_string()),
    };
    let broadcast_json = serde_json::to_value(&broadcast).unwrap();
    assert_eq!(broadcast_json["kind"], "broadcast");
    assert_eq!(broadcast_json["conversationId"], "conversation-1");
    assert!(broadcast_json.get("teamId").is_none());
    assert!(broadcast_json.get("teamMemberId").is_none());
}

#[test]
fn team_session_id_generates_unique() {
    let id1 = TeamSessionId::new();
    let id2 = TeamSessionId::new();
    assert_ne!(id1, id2);
}

#[test]
fn team_session_id_from_string() {
    let id = TeamSessionId::from_string("test-id");
    assert_eq!(id.as_str(), "test-id");
}

#[test]
fn team_message_id_generates_unique() {
    let id1 = TeamMessageId::new();
    let id2 = TeamMessageId::new();
    assert_ne!(id1, id2);
}

#[test]
fn team_session_new_defaults() {
    let session = TeamSession::new("my-team", "ctx-123", "task");
    assert_eq!(session.team_name, "my-team");
    assert_eq!(session.context_id, "ctx-123");
    assert_eq!(session.context_type, "task");
    assert_eq!(session.phase, "forming");
    assert!(session.teammates.is_empty());
    assert!(session.disbanded_at.is_none());
    assert!(session.lead_name.is_none());
}

#[test]
fn team_message_record_new_defaults() {
    let session_id = TeamSessionId::new();
    let msg = TeamMessageRecord::new(session_id.clone(), "worker-1", "hello");
    assert_eq!(msg.team_session_id, session_id);
    assert_eq!(msg.sender, "worker-1");
    assert_eq!(msg.content, "hello");
    assert_eq!(msg.message_type, "teammate_message");
    assert!(msg.recipient.is_none());
}

#[test]
fn teammate_snapshot_serializes() {
    let snap = TeammateSnapshot {
        name: "worker-1".to_string(),
        color: "#ff6b35".to_string(),
        model: "sonnet".to_string(),
        role: "coder".to_string(),
        status: "idle".to_string(),
        cost: TeammateCost {
            input_tokens: 1000,
            output_tokens: 500,
            cache_creation_tokens: 200,
            cache_read_tokens: 100,
            estimated_usd: 0.05,
        },
        spawned_at: "2024-01-01T00:00:00Z".to_string(),
        last_activity_at: "2024-01-01T00:01:00Z".to_string(),
        conversation_id: Some("conv-123".to_string()),
    };
    let json = serde_json::to_string(&snap).unwrap();
    assert!(json.contains("worker-1"));

    let parsed: TeammateSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "worker-1");
    assert_eq!(parsed.color, "#ff6b35");
    assert_eq!(parsed.role, "coder");
    assert_eq!(parsed.cost.input_tokens, 1000);
    assert_eq!(parsed.conversation_id.as_deref(), Some("conv-123"));
}

#[test]
fn teammate_snapshot_deserializes_without_conversation_id() {
    // Existing JSON blobs in DB won't have conversation_id — #[serde(default)] handles this
    let json = r##"{
        "name": "worker-1",
        "color": "#ff6b35",
        "model": "sonnet",
        "role": "coder",
        "status": "idle",
        "cost": {"input_tokens":0,"output_tokens":0,"cache_creation_tokens":0,"cache_read_tokens":0,"estimated_usd":0.0},
        "spawned_at": "2024-01-01T00:00:00Z",
        "last_activity_at": "2024-01-01T00:01:00Z"
    }"##;
    let parsed: TeammateSnapshot = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.name, "worker-1");
    assert!(parsed.conversation_id.is_none());
}

#[test]
fn team_session_id_display() {
    let id = TeamSessionId::from_string("display-test");
    assert_eq!(format!("{}", id), "display-test");
}

#[test]
fn team_message_id_display() {
    let id = TeamMessageId::from_string("msg-display");
    assert_eq!(format!("{}", id), "msg-display");
}

#[test]
fn team_session_id_serializes() {
    let id = TeamSessionId::from_string("ser-test");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"ser-test\"");
}

#[test]
fn team_message_id_serializes() {
    let id = TeamMessageId::from_string("msg-ser");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"msg-ser\"");
}
