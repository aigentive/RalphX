use ralphx_remote_protocol::{
    Capability, ClientFrame, EnvironmentDescriptor, ErrorCode, EventClassification, EventDelivery,
    EventOrigin, ResetReason, RiskClass, ServerFrame, CAPABILITIES, ERROR_CODES,
    EVENT_CLASSIFICATIONS, PROTOCOL_VERSION, RESET_REASONS, RISK_CLASSES, SCOPES,
};
use serde::Serialize;
use serde_json::{json, Value};

fn value<T: Serialize>(input: T) -> Value {
    serde_json::to_value(input).expect("protocol values serialize")
}

#[test]
fn frame_json_contract_is_camel_case_and_preserves_null_seq() {
    let frames = vec![
        value(ServerFrame::Hello {
            protocol_version: PROTOCOL_VERSION,
            environment_id: "env-1".into(),
            stream_epoch: "epoch-1".into(),
            server_version: "0.81.0".into(),
            max_seq: 18_234,
            heartbeat_secs: 20,
        }),
        value(ServerFrame::Event {
            seq: Some(18_235),
            name: "task:status_changed".into(),
            payload: json!({"taskId": "task-1"}),
        }),
        value(ServerFrame::Event {
            seq: None,
            name: "agent:chunk".into(),
            payload: json!({"text": "hello"}),
        }),
        value(ServerFrame::ReplayDone {
            through_seq: 18_235,
        }),
        value(ServerFrame::Reset {
            reason: ResetReason::CursorPruned,
        }),
        value(ServerFrame::Heartbeat { t: 1_626_354_000 }),
        value(ServerFrame::Error {
            code: ErrorCode::RemoteForbidden,
            message: "scope required".into(),
        }),
        value(ClientFrame::Subscribe {
            after_seq: 18_100,
            stream_epoch: "epoch-1".into(),
        }),
        value(ClientFrame::CursorAck { seq: 18_230 }),
        value(ClientFrame::HeartbeatAck { t: 1_626_354_000 }),
    ];
    assert_eq!(
        value(frames.clone()),
        serde_json::from_str::<Value>(include_str!("snapshots/frames.json")).unwrap()
    );
    assert_eq!(frames[2]["seq"], Value::Null);
}

#[test]
fn descriptor_and_wire_enums_match_the_closed_contract() {
    let snapshot = json!({
        "descriptor": value(EnvironmentDescriptor {
            environment_id: "env-1".into(), app_version: "0.81.0".into(),
            protocol_version: PROTOCOL_VERSION, min_client_protocol: 1,
            platform: "macos".into(),
        }),
        "scopes": value(SCOPES), "riskClasses": value(RISK_CLASSES),
        "capabilities": value(CAPABILITIES), "resetReasons": value(RESET_REASONS),
        "errorCodes": value(ERROR_CODES),
    });
    assert_eq!(
        snapshot,
        serde_json::from_str::<Value>(include_str!("snapshots/vocabulary.json")).unwrap()
    );
    assert_eq!(SCOPES.len(), 4);
    assert_eq!(RISK_CLASSES.len(), 6);
    assert_eq!(CAPABILITIES.len(), 11);
    assert_eq!(RESET_REASONS.len(), 6);
    assert_eq!(ERROR_CODES.len(), 8);
}

#[test]
fn class_permits_rejects_every_capability_for_read_and_operate() {
    for capability in CAPABILITIES {
        assert!(!ralphx_remote_protocol::class_permits(
            RiskClass::Read,
            &[*capability]
        ));
        assert!(!ralphx_remote_protocol::class_permits(
            RiskClass::Operate,
            &[*capability]
        ));
    }
    assert!(!ralphx_remote_protocol::class_permits(
        RiskClass::Operate,
        &[Capability::SeedsSpawnTriggeringState]
    ));
}

/// The negation loop above proves nothing about the classes that DO permit capabilities: a
/// refactor making `PathScoped` reject its own capability, or `Denied` accept an empty set,
/// would pass it. PR 1.3's ledger and compile gates build directly on these answers.
#[test]
fn class_permits_accepts_exactly_the_capabilities_each_class_owns() {
    assert!(ralphx_remote_protocol::class_permits(
        RiskClass::PathScoped,
        &[Capability::WritesArbitraryPath]
    ));
    assert!(!ralphx_remote_protocol::class_permits(
        RiskClass::PathScoped,
        &[Capability::WritesArbitraryPath, Capability::SpawnsProcess]
    ));
    assert!(ralphx_remote_protocol::class_permits(
        RiskClass::AgentControl,
        &[
            Capability::AgentControl,
            Capability::SeedsSpawnTriggeringState,
            Capability::MutatesAgentConsumedContent
        ]
    ));
    assert!(!ralphx_remote_protocol::class_permits(
        RiskClass::AgentControl,
        &[Capability::TouchesCredentials]
    ));
    assert!(ralphx_remote_protocol::class_permits(
        RiskClass::Elevated,
        CAPABILITIES
    ));
    assert!(ralphx_remote_protocol::class_permits(RiskClass::Read, &[]));
    assert!(ralphx_remote_protocol::class_permits(
        RiskClass::Operate,
        &[]
    ));
    // `Denied` registers nothing, not even a capability-free command.
    assert!(!ralphx_remote_protocol::class_permits(
        RiskClass::Denied,
        &[]
    ));
}

#[test]
fn event_classification_is_exact_and_snapshotted() {
    assert_eq!(
        value(EVENT_CLASSIFICATIONS),
        serde_json::from_str::<Value>(include_str!("snapshots/event-classifications.json"))
            .unwrap()
    );
    assert!(EventClassification::find("agent:chunk:suffix").is_none());
    assert_eq!(
        EventClassification::find("agent:chunk").unwrap().delivery,
        EventDelivery::Transient
    );
    assert_eq!(
        EventClassification::find("agent_terminal:event")
            .unwrap()
            .excluded_from_v1,
        true
    );
    assert_eq!(
        EventClassification::find("task:updated").unwrap().origin,
        EventOrigin::Webview
    );
    // Backend-emitted host chrome is Local-only with a truthful backend origin; the capture
    // bank drops it on delivery, so nothing has to lie about where it came from.
    for chrome in [
        "ralphx://check-for-updates",
        "ralphx://show-release-notes",
        "gh-auth:login_prompt",
    ] {
        let entry = EventClassification::find(chrome).unwrap();
        assert_eq!(entry.delivery, EventDelivery::LocalOnly, "{chrome}");
        assert_eq!(entry.origin, EventOrigin::Backend, "{chrome}");
    }
    assert_eq!(
        EventClassification::find("notification:created")
            .unwrap()
            .delivery,
        EventDelivery::Durable
    );
    assert_eq!(
        EventClassification::find("permission:resolved")
            .unwrap()
            .delivery,
        EventDelivery::Transient
    );
    assert_eq!(
        EventClassification::find("plan_artifact:approved")
            .unwrap()
            .delivery,
        EventDelivery::Durable
    );
    for stale_name in [
        "task:deleted",
        "team:message",
        "team:status_changed",
        "automation:run_updated",
        // Invented chrome names with no emit site and no consumer anywhere in the repo, plus a
        // JSDoc `@example` string. Local-only rows are exempt from the emit-site assertion, so
        // only this test keeps the allowlist from accumulating phantoms.
        "my:event",
        "window:focus",
        "dock:updated",
        "updater:status",
    ] {
        assert!(
            EventClassification::find(stale_name).is_none(),
            "{stale_name} has no production emitter or UI consumer and must not reserve a remote event classification"
        );
    }
}

const _: () = assert!(!ralphx_remote_protocol::class_permits(
    RiskClass::Operate,
    &[Capability::SeedsSpawnTriggeringState],
));
