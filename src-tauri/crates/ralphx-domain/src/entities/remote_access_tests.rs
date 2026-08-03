use super::*;

fn set(scopes: &[Scope]) -> RemoteScopeSet {
    RemoteScopeSet::from_scopes(scopes.iter().copied())
}

#[test]
fn default_pairing_grant_includes_agent_control_but_never_elevated() {
    // Owner decision 2026-08-03: new pairings receive agent control from the start.
    let grant = RemoteScopeSet::default_pairing_grant();

    assert!(grant.contains(Scope::UiRead));
    assert!(grant.contains(Scope::UiOperate));
    assert!(grant.contains(Scope::UiAgent));
    assert!(!grant.contains(Scope::UiElevated));
    validate_pairing_grant(&grant).expect("the default grant must be mintable");
}

#[test]
fn a_pairing_grant_may_carry_agent_but_never_elevated_scope() {
    validate_pairing_grant(&set(&[Scope::UiRead, Scope::UiAgent]))
        .expect("ui:agent is grantable at pairing");
    assert_eq!(
        validate_pairing_grant(&set(&[Scope::UiElevated])),
        Err(RemoteScopeError::NotGrantable(Scope::UiElevated))
    );
}

#[test]
fn scope_sets_serialize_canonically_and_round_trip() {
    let unordered = set(&[
        Scope::UiAgent,
        Scope::UiRead,
        Scope::UiRead,
        Scope::UiOperate,
    ]);

    let json = unordered.to_json().expect("scopes should serialize");

    assert_eq!(json, r#"["ui:read","ui:operate","ui:agent"]"#);
    assert_eq!(
        RemoteScopeSet::from_json(&json).expect("scopes should parse"),
        unordered
    );
}

/// Fail closed on reads: a row a newer build wrote must not silently degrade to a narrower
/// (or wider) grant — the read is an error the caller has to handle.
#[test]
fn an_unrecognized_stored_scope_is_an_error_not_a_silent_drop() {
    let error = RemoteScopeSet::from_json(r#"["ui:read","ui:teleport"]"#)
        .expect_err("an unknown scope must not parse");

    assert!(matches!(error, RemoteScopeError::Malformed(_)));
}

#[test]
fn requested_scopes_must_be_a_subset_of_the_pairing_grant() {
    let grant = RemoteScopeSet::default_pairing_grant();

    assert_eq!(
        effective_pairing_scopes(&grant, None).expect("absent request takes the whole grant"),
        grant
    );
    assert_eq!(
        effective_pairing_scopes(&grant, Some(&set(&[Scope::UiRead])))
            .expect("a narrower request is honoured"),
        set(&[Scope::UiRead])
    );
    assert_eq!(
        effective_pairing_scopes(&grant, Some(&set(&[Scope::UiElevated]))),
        Err(RemoteScopeError::NotGranted(Scope::UiElevated)),
        "asking for more than the grant must fail, never quietly intersect"
    );
}

#[test]
fn agent_control_can_be_revoked_and_regranted_for_a_paired_device() {
    let device = RemoteDevice {
        id: RemoteDeviceId::new(),
        name: "laptop".to_string(),
        token_hash: "hash".to_string(),
        token_prefix: "rxd_live_aaaa".to_string(),
        scopes: RemoteScopeSet::default_pairing_grant(),
        created_at: "2026-07-27T00:00:00Z".to_string(),
        last_seen_at: None,
        revoked_at: None,
    };

    assert!(device.is_active());
    assert!(device.agent_control_granted());

    let revoked = RemoteDevice {
        scopes: device.scopes.without(Scope::UiAgent),
        ..device.clone()
    };
    assert!(!revoked.agent_control_granted());

    let regranted = RemoteDevice {
        scopes: revoked.scopes.with(Scope::UiAgent),
        ..revoked
    };
    assert!(regranted.agent_control_granted());
    assert_eq!(
        regranted.scopes,
        RemoteScopeSet::default_pairing_grant(),
        "regranting agent control must restore the default pairing grant"
    );
}

#[test]
fn a_device_serialization_never_carries_the_token_hash() {
    let device = RemoteDevice {
        id: RemoteDeviceId::from_string("device-1"),
        name: "laptop".to_string(),
        token_hash: "sha256-of-the-token".to_string(),
        token_prefix: "rxd_live_aaaa".to_string(),
        scopes: RemoteScopeSet::default_pairing_grant(),
        created_at: "2026-07-27T00:00:00Z".to_string(),
        last_seen_at: None,
        revoked_at: None,
    };

    let json = serde_json::to_string(&device).expect("device should serialize");

    assert!(!json.contains("sha256-of-the-token"));
    assert!(!json.contains("tokenHash"));
    assert!(json.contains("tokenPrefix"));
}

#[test]
fn audit_actions_have_stable_distinct_db_values() {
    let actions = [
        RemoteAuditAction::PairingCodeCreated,
        RemoteAuditAction::PairingCodeRevoked,
        RemoteAuditAction::PairingSucceeded,
        RemoteAuditAction::PairingRejected,
        RemoteAuditAction::AuthAccepted,
        RemoteAuditAction::AuthRejected,
        RemoteAuditAction::AuthStoreError,
        RemoteAuditAction::RateLimited,
        RemoteAuditAction::WsTicketIssued,
        RemoteAuditAction::WsTicketConsumed,
        RemoteAuditAction::WsTicketRejected,
        RemoteAuditAction::SessionOpened,
        RemoteAuditAction::SessionClosed,
        RemoteAuditAction::DeviceRevoked,
        RemoteAuditAction::AgentControlGranted,
        RemoteAuditAction::AgentControlRevoked,
        RemoteAuditAction::ListenerDisabled,
    ];

    let values: std::collections::BTreeSet<&str> =
        actions.iter().map(|action| action.as_db_value()).collect();

    assert_eq!(
        values.len(),
        actions.len(),
        "audit actions must be distinct"
    );
    assert_eq!(
        RemoteAuditAction::AuthStoreError.as_db_value(),
        "auth_store_error"
    );
}
