//! P-19: retention-lease correctness — the corrected inequality, both release paths, TTL expiry.

use std::time::{Duration, Instant};

use super::{RetentionLeaseRegistry, DEFAULT_LEASE_TTL};

fn registry() -> RetentionLeaseRegistry {
    RetentionLeaseRegistry::with_ttl(Duration::from_secs(60))
}

#[test]
fn an_unleased_registry_reports_an_unbounded_floor() {
    let registry = registry();
    assert_eq!(
        registry.lease_floor_at(Instant::now()),
        u64::MAX,
        "no lease means retention alone decides; the floor must not clamp to 0"
    );
    assert_eq!(registry.min_live_cursor_at(Instant::now()), None);
}

/// P-19(a): the floor is the LOWEST live cursor, so the pruner is bounded by the neediest
/// subscriber, not by the most advanced one.
#[test]
fn the_floor_is_the_lowest_live_cursor() {
    let registry = registry();
    let _fast = registry.acquire(900);
    let _slow = registry.acquire(100);
    let _mid = registry.acquire(400);

    assert_eq!(registry.min_live_cursor_at(Instant::now()), Some(100));
    assert_eq!(registry.lease_floor_at(Instant::now()), 100);
}

/// P-5(e) H-barrier: a subscription at `afterSeq = H` registers the lease AT `H`, so everything
/// `> H` — exactly the hydration delta — is above the floor and therefore undeletable.
#[test]
fn a_hydration_lease_pins_the_barrier_not_the_delta() {
    let registry = registry();
    let hydrating = registry.acquire(1_000);

    let floor = registry.lease_floor_at(Instant::now());
    assert_eq!(floor, 1_000);
    assert!(
        floor < 1_001,
        "the first row of the hydration delta must sit strictly above the prune floor"
    );
    assert_eq!(hydrating.cursor(), Some(1_000));
}

/// P-19(b) release path 1: `cursorAck` advances the lease, making the acked span prune-eligible.
#[test]
fn cursor_ack_advances_the_lease_and_releases_the_span_below_it() {
    let registry = registry();
    let lease = registry.acquire(100);

    lease.advance(250);

    assert_eq!(lease.cursor(), Some(250));
    assert_eq!(registry.lease_floor_at(Instant::now()), 250);
}

#[test]
fn a_backwards_or_repeated_ack_never_moves_the_cursor_down() {
    let registry = registry();
    let lease = registry.acquire(100);

    lease.advance(250);
    lease.advance(120);
    lease.advance(250);

    assert_eq!(lease.cursor(), Some(250));
}

/// P-19(b) release path 2: a dropped/heartbeat-dead/revoked connection releases immediately.
/// The release is `Drop`, so a session task that unwinds still releases.
#[test]
fn dropping_a_lease_releases_it_immediately() {
    let registry = registry();
    let held = registry.acquire(100);
    assert_eq!(registry.lease_floor_at(Instant::now()), 100);

    drop(held);

    assert_eq!(
        registry.lease_floor_at(Instant::now()),
        u64::MAX,
        "a dropped connection must stop pinning the log at once, not at TTL"
    );
    assert_eq!(registry.live_count_at(Instant::now()), 0);
}

/// P-19(c): a connected-but-never-acking client loses its lease at TTL, so one dead consumer
/// cannot pin `remote_event_log` growth forever. The socket may still be open — that is the point.
#[test]
fn a_never_acking_lease_expires_at_ttl_even_while_the_socket_is_open() {
    let registry = RetentionLeaseRegistry::with_ttl(Duration::from_secs(900));
    let start = Instant::now();
    let _wedged = registry.acquire_at(100, start);

    let inside_ttl = start + Duration::from_secs(899);
    assert_eq!(registry.lease_floor_at(inside_ttl), 100);

    let past_ttl = start + Duration::from_secs(901);
    assert_eq!(
        registry.lease_floor_at(past_ttl),
        u64::MAX,
        "past TTL the wedged consumer must stop clamping the pruner"
    );
    assert_eq!(registry.live_count_at(past_ttl), 0);
}

/// TTL is refreshed by ACK PROGRESS, not by liveness (§3.4). A heartbeat cannot keep a lease
/// alive; only a cursor that actually moves does.
#[test]
fn only_forward_ack_progress_refreshes_the_ttl() {
    let registry = RetentionLeaseRegistry::with_ttl(Duration::from_secs(100));
    let start = Instant::now();
    let lease = registry.acquire_at(100, start);

    // A re-ack of the same seq at t+90 is not progress and must not refresh.
    lease.advance_at(100, start + Duration::from_secs(90));
    assert_eq!(
        registry.lease_floor_at(start + Duration::from_secs(101)),
        u64::MAX,
        "a repeated ack is not progress and must not extend the lease"
    );

    let lease = registry.acquire_at(100, start);
    lease.advance_at(180, start + Duration::from_secs(90));
    assert_eq!(
        registry.lease_floor_at(start + Duration::from_secs(101)),
        180,
        "real forward progress refreshes the TTL"
    );
}

#[test]
fn an_expired_lease_stops_clamping_but_a_live_sibling_still_does() {
    let registry = RetentionLeaseRegistry::with_ttl(Duration::from_secs(100));
    let start = Instant::now();
    let _wedged = registry.acquire_at(10, start);
    let healthy = registry.acquire_at(50, start);

    let later = start + Duration::from_secs(90);
    healthy.advance_at(400, later);

    let past_wedged_ttl = start + Duration::from_secs(101);
    assert_eq!(
        registry.lease_floor_at(past_wedged_ttl),
        400,
        "the wedged lease expired; the healthy one still bounds the pruner"
    );
}

#[test]
fn the_default_ttl_matches_the_spec_window() {
    assert_eq!(DEFAULT_LEASE_TTL, Duration::from_secs(15 * 60));
}
