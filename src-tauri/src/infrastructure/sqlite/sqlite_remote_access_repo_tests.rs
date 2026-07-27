use super::*;
use crate::domain::entities::{RemoteScopeSet, RemoteWsTicket};
use crate::domain::services::key_crypto::{generate_prefixed_key, hash_key};
use crate::testing::SqliteTestDb;
use ralphx_remote_protocol::Scope;

const NOW: &str = "2026-07-27T12:00:00+00:00";
const LATER: &str = "2026-07-27T12:10:00+00:00";
const MUCH_LATER: &str = "2026-07-27T13:00:00+00:00";

fn repo(db: &SqliteTestDb) -> SqliteRemoteAccessRepository {
    SqliteRemoteAccessRepository::from_db(DbConnection::from_shared(db.shared_conn()))
}

fn pairing_code(raw: &str, scopes: RemoteScopeSet, expires_at: &str) -> RemotePairingCode {
    RemotePairingCode {
        id: RemotePairingCodeId::new(),
        code_hash: hash_key(raw),
        scopes,
        created_at: NOW.to_string(),
        expires_at: expires_at.to_string(),
        consumed_at: None,
    }
}

fn redemption(raw_code: &str, now: &str) -> RemotePairingRedemption {
    let token = generate_prefixed_key("rxd_live_");
    RemotePairingRedemption {
        code_hash: hash_key(raw_code),
        device_id: RemoteDeviceId::new(),
        device_name: "laptop".to_string(),
        token_hash: hash_key(&token),
        token_prefix: token.chars().take(13).collect(),
        requested_scopes: None,
        now: now.to_string(),
    }
}

/// C-1: every repository method here goes through `DbConnection::run` /
/// `run_transaction`. A direct `conn.lock().await` would silently reintroduce blocking
/// access, so the source itself is the assertion.
#[test]
fn the_repository_never_locks_the_connection_directly() {
    let source = include_str!("sqlite_remote_access_repo.rs");

    assert!(!source.contains("lock().await"));
    assert!(!source.contains("blocking_lock()"));
    assert!(source.contains(".run(move |conn|"));
    assert!(source.contains(".run_transaction(move |conn|"));
}

#[tokio::test]
async fn redeeming_a_code_mints_a_device_and_consumes_the_code_in_one_transaction() {
    let db = SqliteTestDb::new("remote-access-redeem");
    let repo = repo(&db);
    let raw = generate_prefixed_key("rxp_");
    repo.create(pairing_code(
        &raw,
        RemoteScopeSet::default_pairing_grant(),
        LATER,
    ))
    .await
    .expect("code should insert");

    let outcome = repo
        .redeem(redemption(&raw, NOW))
        .await
        .expect("redemption should complete");

    let RemotePairingOutcome::Paired(device) = outcome else {
        panic!("expected a paired device, got {outcome:?}");
    };
    assert_eq!(device.scopes, RemoteScopeSet::default_pairing_grant());
    assert!(!device.agent_control_granted());
    assert!(device.is_active());
    assert!(repo
        .list_outstanding(NOW)
        .await
        .expect("outstanding codes should read")
        .is_empty());
}

/// A-9: only hashes reach the tables — a raw code or token never appears at rest.
#[tokio::test]
async fn codes_and_tokens_are_stored_hashed() {
    let db = SqliteTestDb::new("remote-access-hash-at-rest");
    let repo = repo(&db);
    let raw = generate_prefixed_key("rxp_");
    repo.create(pairing_code(
        &raw,
        RemoteScopeSet::default_pairing_grant(),
        LATER,
    ))
    .await
    .expect("code should insert");
    let token = generate_prefixed_key("rxd_live_");
    let mut redemption = redemption(&raw, NOW);
    redemption.token_hash = hash_key(&token);

    repo.redeem(redemption).await.expect("redemption completes");

    db.with_connection(|conn| {
        let stored_code: String = conn
            .query_row("SELECT code_hash FROM remote_pairing_codes", [], |row| {
                row.get(0)
            })
            .expect("code hash should read");
        let stored_token: String = conn
            .query_row("SELECT token_hash FROM remote_devices", [], |row| {
                row.get(0)
            })
            .expect("token hash should read");
        assert_ne!(stored_code, raw);
        assert_eq!(stored_code, hash_key(&raw));
        assert_ne!(stored_token, token);
        assert_eq!(stored_token, hash_key(&token));
    });
}

/// P-7: two concurrent redemptions of one code — exactly one may pair.
#[tokio::test]
async fn concurrent_redemptions_of_one_code_pair_exactly_once() {
    let db = SqliteTestDb::new("remote-access-single-use");
    let seeder = repo(&db);
    let raw = generate_prefixed_key("rxp_");
    seeder
        .create(pairing_code(
            &raw,
            RemoteScopeSet::default_pairing_grant(),
            LATER,
        ))
        .await
        .expect("code should insert");
    let first = SqliteRemoteAccessRepository::new(db.new_connection());
    let second = SqliteRemoteAccessRepository::new(db.new_connection());

    let (left, right) = tokio::join!(
        first.redeem(redemption(&raw, NOW)),
        second.redeem(redemption(&raw, NOW)),
    );

    let paired = [&left, &right]
        .iter()
        .filter(|result| matches!(result, Ok(RemotePairingOutcome::Paired(_))))
        .count();
    assert_eq!(
        paired, 1,
        "exactly one redemption may pair: {left:?} / {right:?}"
    );
    db.with_connection(|conn| {
        let devices: i64 = conn
            .query_row("SELECT COUNT(*) FROM remote_devices", [], |row| row.get(0))
            .expect("device count should read");
        let consumed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM remote_pairing_codes WHERE consumed_at IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("consumed count should read");
        assert_eq!(devices, 1);
        assert_eq!(consumed, 1);
    });
}

#[tokio::test]
async fn a_replayed_code_is_rejected_as_already_consumed() {
    let db = SqliteTestDb::new("remote-access-replay");
    let repo = repo(&db);
    let raw = generate_prefixed_key("rxp_");
    repo.create(pairing_code(
        &raw,
        RemoteScopeSet::default_pairing_grant(),
        LATER,
    ))
    .await
    .expect("code should insert");
    repo.redeem(redemption(&raw, NOW))
        .await
        .expect("first redemption completes");

    let replay = repo
        .redeem(redemption(&raw, NOW))
        .await
        .expect("replay completes");

    assert_eq!(replay, RemotePairingOutcome::AlreadyConsumed);
}

#[tokio::test]
async fn an_expired_or_unknown_code_never_pairs() {
    let db = SqliteTestDb::new("remote-access-ttl");
    let repo = repo(&db);
    let raw = generate_prefixed_key("rxp_");
    repo.create(pairing_code(
        &raw,
        RemoteScopeSet::default_pairing_grant(),
        LATER,
    ))
    .await
    .expect("code should insert");

    let expired = repo
        .redeem(redemption(&raw, MUCH_LATER))
        .await
        .expect("expired redemption completes");
    let unknown = repo
        .redeem(redemption("rxp_never-minted", NOW))
        .await
        .expect("unknown redemption completes");

    assert_eq!(expired, RemotePairingOutcome::Expired);
    assert_eq!(unknown, RemotePairingOutcome::Unknown);
    db.with_connection(|conn| {
        let devices: i64 = conn
            .query_row("SELECT COUNT(*) FROM remote_devices", [], |row| row.get(0))
            .expect("device count should read");
        assert_eq!(devices, 0, "a failed redemption must not mint a device");
    });
}

#[tokio::test]
async fn requesting_a_scope_outside_the_grant_is_refused_without_consuming_the_code() {
    let db = SqliteTestDb::new("remote-access-scope-subset");
    let repo = repo(&db);
    let raw = generate_prefixed_key("rxp_");
    repo.create(pairing_code(
        &raw,
        RemoteScopeSet::from_scopes([Scope::UiRead]),
        LATER,
    ))
    .await
    .expect("code should insert");
    let mut redemption = redemption(&raw, NOW);
    redemption.requested_scopes = Some(RemoteScopeSet::from_scopes([
        Scope::UiRead,
        Scope::UiOperate,
    ]));

    let outcome = repo.redeem(redemption).await.expect("redemption completes");

    assert_eq!(
        outcome,
        RemotePairingOutcome::ScopeNotGranted(Scope::UiOperate)
    );
    assert_eq!(
        repo.list_outstanding(NOW)
            .await
            .expect("outstanding codes should read")
            .len(),
        1,
        "a refused scope request must leave the code redeemable"
    );
}

#[tokio::test]
async fn token_lookup_distinguishes_active_revoked_and_unknown() {
    let db = SqliteTestDb::new("remote-access-lookup");
    let repo = repo(&db);
    let raw = generate_prefixed_key("rxp_");
    repo.create(pairing_code(
        &raw,
        RemoteScopeSet::default_pairing_grant(),
        LATER,
    ))
    .await
    .expect("code should insert");
    let token = generate_prefixed_key("rxd_live_");
    let mut redemption = redemption(&raw, NOW);
    redemption.token_hash = hash_key(&token);
    let RemotePairingOutcome::Paired(device) =
        repo.redeem(redemption).await.expect("redemption completes")
    else {
        panic!("device should pair");
    };

    let active = repo
        .lookup_by_token_hash(&hash_key(&token))
        .await
        .expect("lookup completes");
    let unknown = repo
        .lookup_by_token_hash(&hash_key("rxd_live_not-a-real-token"))
        .await
        .expect("lookup completes");
    repo.revoke(&device.id, LATER)
        .await
        .expect("revoke completes");
    let revoked = repo
        .lookup_by_token_hash(&hash_key(&token))
        .await
        .expect("lookup completes");

    assert!(matches!(active, RemoteDeviceLookup::Active(_)));
    assert_eq!(unknown, RemoteDeviceLookup::Unknown);
    let RemoteDeviceLookup::Revoked(revoked) = revoked else {
        panic!("a revoked device must resolve to Revoked, not Unknown");
    };
    assert_eq!(revoked.revoked_at.as_deref(), Some(LATER));
}

#[tokio::test]
async fn revocation_is_idempotent_and_keeps_the_first_timestamp() {
    let db = SqliteTestDb::new("remote-access-revoke-idempotent");
    let repo = repo(&db);
    let device = paired_device(&repo).await;

    repo.revoke(&device.id, LATER)
        .await
        .expect("first revoke completes");
    let second = repo
        .revoke(&device.id, MUCH_LATER)
        .await
        .expect("second revoke completes")
        .expect("device should still exist");

    assert_eq!(second.revoked_at.as_deref(), Some(LATER));
}

#[tokio::test]
async fn agent_control_scopes_can_be_granted_and_narrowed_but_never_on_a_revoked_device() {
    let db = SqliteTestDb::new("remote-access-agent-control");
    let repo = repo(&db);
    let device = paired_device(&repo).await;

    let granted = repo
        .set_scopes(&device.id, &device.scopes.with(Scope::UiAgent))
        .await
        .expect("grant completes")
        .expect("device exists");
    let narrowed = repo
        .set_scopes(&device.id, &granted.scopes.without(Scope::UiAgent))
        .await
        .expect("narrow completes")
        .expect("device exists");
    repo.revoke(&device.id, LATER)
        .await
        .expect("revoke completes");
    let after_revoke = repo
        .set_scopes(&device.id, &narrowed.scopes.with(Scope::UiAgent))
        .await
        .expect("post-revoke set completes")
        .expect("device exists");

    assert!(granted.agent_control_granted());
    assert!(!narrowed.agent_control_granted());
    assert_eq!(narrowed.scopes, RemoteScopeSet::default_pairing_grant());
    assert!(
        !after_revoke.agent_control_granted(),
        "a revoked device must not be re-widened"
    );
}

#[tokio::test]
async fn ws_tickets_are_single_use_device_bound_and_expiring() {
    let db = SqliteTestDb::new("remote-access-ws-tickets");
    let repo = repo(&db);
    let device = paired_device(&repo).await;
    let raw = generate_prefixed_key("rxt_");
    let expired_raw = generate_prefixed_key("rxt_");
    repo.issue(&hash_key(&raw), &device.id, LATER)
        .await
        .expect("ticket should issue");
    repo.issue(&hash_key(&expired_raw), &device.id, LATER)
        .await
        .expect("second ticket should issue");

    let first = repo
        .consume(&hash_key(&raw), NOW)
        .await
        .expect("consume completes");
    let replay = repo
        .consume(&hash_key(&raw), NOW)
        .await
        .expect("replay completes");
    let expired = repo
        .consume(&hash_key(&expired_raw), MUCH_LATER)
        .await
        .expect("expired consume completes");
    let unknown = repo
        .consume(&hash_key("rxt_never-issued"), NOW)
        .await
        .expect("unknown consume completes");

    assert_eq!(first, RemoteWsTicketOutcome::Consumed(device.id.clone()));
    assert_eq!(replay, RemoteWsTicketOutcome::AlreadyConsumed);
    assert_eq!(expired, RemoteWsTicketOutcome::Expired);
    assert_eq!(unknown, RemoteWsTicketOutcome::Unknown);
}

#[tokio::test]
async fn revoking_a_device_can_invalidate_its_outstanding_tickets() {
    let db = SqliteTestDb::new("remote-access-ticket-sweep");
    let repo = repo(&db);
    let device = paired_device(&repo).await;
    let raw = generate_prefixed_key("rxt_");
    repo.issue(&hash_key(&raw), &device.id, LATER)
        .await
        .expect("ticket should issue");

    let swept = repo
        .consume_all_for_device(&device.id, LATER)
        .await
        .expect("sweep completes");
    let after = repo
        .consume(&hash_key(&raw), NOW)
        .await
        .expect("consume completes");

    assert_eq!(swept, 1);
    assert_eq!(after, RemoteWsTicketOutcome::AlreadyConsumed);
}

#[tokio::test]
async fn sessions_close_per_device_and_globally() {
    let db = SqliteTestDb::new("remote-access-sessions");
    let repo = repo(&db);
    let device = paired_device(&repo).await;
    let other = paired_device(&repo).await;
    for owner in [&device, &other] {
        repo.open(RemoteSession {
            id: RemoteSessionId::new(),
            device_id: owner.id.clone(),
            connected_at: NOW.to_string(),
            last_active_at: NOW.to_string(),
            remote_addr: "127.0.0.1:51000".to_string(),
            closed_at: None,
        })
        .await
        .expect("session opens");
    }

    let closed_for_device = repo
        .close_all_for_device(&device.id, LATER)
        .await
        .expect("device close completes");
    let open_after = repo.list_open().await.expect("open sessions read");
    let closed_globally = repo
        .close_all(MUCH_LATER)
        .await
        .expect("global close completes");

    assert_eq!(closed_for_device, 1);
    assert_eq!(open_after.len(), 1);
    assert_eq!(open_after[0].device_id, other.id);
    assert_eq!(closed_globally, 1);
    assert!(repo
        .list_open()
        .await
        .expect("open sessions read")
        .is_empty());
}

#[tokio::test]
async fn audit_rows_are_appended_newest_first() {
    let db = SqliteTestDb::new("remote-access-audit");
    let repo = repo(&db);
    let device = paired_device(&repo).await;

    repo.record(
        Some(&device.id),
        RemoteAuditAction::AuthAccepted,
        Some("GET /remote/v1/session"),
        NOW,
    )
    .await
    .expect("audit row writes");
    repo.record(None, RemoteAuditAction::PairingRejected, None, LATER)
        .await
        .expect("anonymous audit row writes");

    let entries = repo.list_recent(Some(10)).await.expect("audit log reads");

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].action, "pairing_rejected");
    assert_eq!(entries[0].device_id, None);
    assert_eq!(entries[1].action, "auth_accepted");
    assert_eq!(entries[1].device_id.as_ref(), Some(&device.id));
    assert_eq!(entries[1].detail.as_deref(), Some("GET /remote/v1/session"));
}

/// A malformed scope column must surface as a store error rather than an empty grant.
#[tokio::test]
async fn a_corrupt_scope_column_fails_the_read_instead_of_narrowing_the_grant() {
    let db = SqliteTestDb::new("remote-access-corrupt-scopes");
    let repo = repo(&db);
    let device = paired_device(&repo).await;
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE remote_devices SET scopes = '[\"ui:teleport\"]' WHERE id = ?1",
            rusqlite::params![device.id.as_str()],
        )
        .expect("corrupt scopes should write");
    });

    let error = repo
        .get(&device.id)
        .await
        .expect_err("a malformed grant must not read as an empty grant");

    assert!(matches!(error, AppError::Database(_)));
}

#[test]
fn the_ws_ticket_entity_carries_only_a_hash() {
    let ticket = RemoteWsTicket {
        ticket_hash: hash_key("rxt_example"),
        device_id: RemoteDeviceId::from_string("device-1"),
        expires_at: LATER.to_string(),
        consumed_at: None,
    };

    assert_eq!(ticket.ticket_hash.len(), 64);
    assert!(!ticket.ticket_hash.starts_with("rxt_"));
}

async fn paired_device(repo: &SqliteRemoteAccessRepository) -> RemoteDevice {
    let raw = generate_prefixed_key("rxp_");
    repo.create(pairing_code(
        &raw,
        RemoteScopeSet::default_pairing_grant(),
        LATER,
    ))
    .await
    .expect("code should insert");
    match repo
        .redeem(redemption(&raw, NOW))
        .await
        .expect("redemption completes")
    {
        RemotePairingOutcome::Paired(device) => device,
        other => panic!("expected a paired device, got {other:?}"),
    }
}
