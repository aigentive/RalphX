use ralphx_remote_protocol::{
    Capability, ClientFrame, EnvironmentDescriptor, ErrorCode, EventClassification, EventDelivery,
    EventOrigin, ResetReason, RiskClass, Scope, ServerFrame, CAPABILITIES, ERROR_CODES,
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
    assert_eq!(ERROR_CODES.len(), 10);
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

/// P-11 batch B0 — the third disposition. Every ledger row resolves against the v1 facade as
/// exactly one of: registerable, or one of three manifest-classified refusals. The drift scan
/// consumes this derivation through the rendered manifest, so the mapping is contract, not a
/// scan-local heuristic.
#[test]
fn v1_resolution_partitions_every_class_capability_pair() {
    use ralphx_remote_protocol::{v1_resolution, V1Resolution, V1_FACADE_SCOPES};

    // `Denied` has no scope at any capability set — `scope_for_class` yields `None`.
    for capabilities in [&[][..], &[Capability::SpawnsProcess][..], CAPABILITIES] {
        assert_eq!(
            v1_resolution(RiskClass::Denied, capabilities),
            V1Resolution::HostDenied,
        );
    }

    // `SpawnsProcess` outranks the class label: `class_permits` admits it only under
    // `Elevated`, and `ui:elevated` is a v1 non-goal, so it is unexposable at any v1 scope.
    assert_eq!(
        v1_resolution(RiskClass::Elevated, &[Capability::SpawnsProcess]),
        V1Resolution::HostDeniedSpawnsProcess,
    );
    assert_eq!(
        v1_resolution(
            RiskClass::Elevated,
            &[Capability::PtyControl, Capability::SpawnsProcess]
        ),
        V1Resolution::HostDeniedSpawnsProcess,
    );

    // Ledgered `Elevated` without `SpawnsProcess` is deferred, not denied.
    assert_eq!(
        v1_resolution(RiskClass::Elevated, &[Capability::TouchesCredentials]),
        V1Resolution::V1Deferred,
    );
    assert_eq!(
        v1_resolution(RiskClass::Elevated, &[]),
        V1Resolution::V1Deferred,
    );

    // Everything a v1 scope can carry stays a registration candidate.
    for class in [
        RiskClass::Read,
        RiskClass::Operate,
        RiskClass::PathScoped,
        RiskClass::AgentControl,
    ] {
        assert_eq!(v1_resolution(class, &[]), V1Resolution::Registerable);
    }

    // `ui:elevated` is excluded from the v1 facade (§1); the constant is what the derivation
    // and the census both read, so pin it.
    assert_eq!(
        V1_FACADE_SCOPES,
        &[Scope::UiRead, Scope::UiOperate, Scope::UiAgent]
    );
}

/// The wire spelling the manifest renders. The drift scan matches these literals, so a rename
/// is a breaking change to the ratchet and must fail here first.
#[test]
fn v1_resolution_wire_spelling_is_kebab_case_and_closed() {
    use ralphx_remote_protocol::{V1Resolution, V1_RESOLUTIONS};

    assert_eq!(
        V1_RESOLUTIONS
            .iter()
            .map(|resolution| value(resolution))
            .collect::<Vec<_>>(),
        vec![
            json!("registerable"),
            json!("host-denied"),
            json!("host-denied-spawns-process"),
            json!("v1-deferred"),
            json!("v1-audit-refused"),
        ]
    );
    assert_eq!(V1_RESOLUTIONS.len(), 5);
    assert!(V1_RESOLUTIONS.contains(&V1Resolution::Registerable));
}

/// PR 3.1-b batch 9 — the audit-refusal overlay, and the two properties that keep it honest.
///
/// `V1AuditRefused` is the only resolution not derived from `(class, capabilities)`. That makes
/// it the only one a human can grant by writing a table row, so the derivation must guarantee
/// (a) it can never mask a mechanically proven refusal, and (b) it can never appear without the
/// caller explicitly asserting a refusal was recorded.
#[test]
fn audit_refusal_overlays_registerable_and_never_masks_a_mechanical_refusal() {
    use ralphx_remote_protocol::{
        v1_resolution, v1_resolution_with_audit, AuditRefusalReason, V1Resolution,
        AUDIT_REFUSAL_REASONS,
    };

    // (b) Without a recorded refusal the overlay is the identity function, across every pair.
    for class in [
        RiskClass::Read,
        RiskClass::Operate,
        RiskClass::PathScoped,
        RiskClass::AgentControl,
        RiskClass::Elevated,
        RiskClass::Denied,
    ] {
        for capabilities in [&[][..], &[Capability::SpawnsProcess][..], CAPABILITIES] {
            assert_eq!(
                v1_resolution_with_audit(class, capabilities, false),
                v1_resolution(class, capabilities),
                "the overlay must not move a row that has no recorded refusal"
            );
        }
    }

    // (a) With one, only `Registerable` moves. Every mechanical refusal survives untouched —
    // otherwise a hand-written table row could downgrade a proven denial to a softer class.
    assert_eq!(
        v1_resolution_with_audit(RiskClass::AgentControl, &[], true),
        V1Resolution::V1AuditRefused,
    );
    assert_eq!(
        v1_resolution_with_audit(RiskClass::Read, &[], true),
        V1Resolution::V1AuditRefused,
    );
    for (class, capabilities, expected) in [
        (RiskClass::Denied, &[][..], V1Resolution::HostDenied),
        (
            RiskClass::Elevated,
            &[Capability::SpawnsProcess][..],
            V1Resolution::HostDeniedSpawnsProcess,
        ),
        (
            RiskClass::Denied,
            &[Capability::SpawnsProcess][..],
            V1Resolution::HostDenied,
        ),
        (
            RiskClass::Elevated,
            &[Capability::TouchesCredentials][..],
            V1Resolution::V1Deferred,
        ),
    ] {
        assert_eq!(
            v1_resolution_with_audit(class, capabilities, true),
            expected,
            "an audit row must not mask the mechanical refusal for {class:?}/{capabilities:?}"
        );
    }

    // The reason vocabulary is closed and kebab-cased; the manifest renders these literals.
    assert_eq!(
        AUDIT_REFUSAL_REASONS
            .iter()
            .map(|reason| value(reason))
            .collect::<Vec<_>>(),
        vec![
            json!("fail-open-until-fixed"),
            json!("constructs-spawn-capable-service"),
            json!("seam-resolved-via-remote-twin"),
            json!("reaches-corrective-transition"),
        ]
    );
    // Deliberately absent, and the absence is the point: there is no reason code for "it arms",
    // "it steers" or "it writes". The facade serves `agentControl` ops carrying exactly those
    // capabilities, so such a refusal is a batch's scope limit, not a host denial.
    //
    // `reaches-corrective-transition` (batch 14) is NOT a breach of that: it names one specific
    // mechanism that is separately CI-enforced by
    // `no_registered_facade_target_reaches_a_corrective_transition`, not a general class of
    // authority. Extending a closed vocabulary with a reviewed, specific, falsifiable code is
    // the sanctioned move when the alternative is filing a false reason; minting a generic
    // arming/steering code would not be.
    // 5 -> 4: WP4 (a) retired `transport-shape-deferred`. Every row that ever used it cited a
    // non-`Serialize` `AppError` that has been `Serialize` since `96ce527a9`, so the code
    // recorded a misreading rather than a limitation. Shrinking a closed vocabulary when its
    // only evidence is disproven is the same discipline as refusing to widen it without any.
    assert_eq!(AUDIT_REFUSAL_REASONS.len(), 4);
    assert!(AUDIT_REFUSAL_REASONS.contains(&AuditRefusalReason::FailOpenUntilFixed));
}
