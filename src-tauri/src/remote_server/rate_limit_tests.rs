use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use super::rate_limit::{
    auth_endpoint_key, RemoteRateLimitDecision, RemoteRateLimitKey, RemoteRateLimiter,
    REMOTE_RATE_LIMIT_DEFAULTS,
};
use super::settings::RemoteExposureMode;
use crate::domain::entities::RemoteDeviceId;

fn ip(value: &str) -> IpAddr {
    value.parse().expect("test address should parse")
}

/// The params must stay pinned to the external-MCP limiter (rate-limiter.ts:14-17).
#[test]
fn the_limiter_params_match_the_external_mcp_limiter() {
    let params = REMOTE_RATE_LIMIT_DEFAULTS;

    assert_eq!(params.requests_per_second, 10.0);
    assert_eq!(params.auth_failures_before_lockout, 5);
    assert_eq!(params.lockout, Duration::from_secs(30));
}

#[test]
fn the_token_bucket_admits_the_burst_then_refuses() {
    let limiter = RemoteRateLimiter::default();
    let key = RemoteRateLimitKey::Global;
    let now = Instant::now();

    let admitted = (0..10)
        .filter(|_| limiter.check_at(&key, now).is_allowed())
        .count();
    let eleventh = limiter.check_at(&key, now);

    assert_eq!(admitted, 10);
    assert!(matches!(
        eleventh,
        RemoteRateLimitDecision::RateLimited { .. }
    ));
    assert!(eleventh.retry_after_secs().is_some_and(|secs| secs >= 1));
}

#[test]
fn the_bucket_refills_over_time() {
    let limiter = RemoteRateLimiter::default();
    let key = RemoteRateLimitKey::Global;
    let start = Instant::now();
    for _ in 0..10 {
        limiter.check_at(&key, start);
    }

    let while_empty = limiter.check_at(&key, start);
    let after_refill = limiter.check_at(&key, start + Duration::from_secs(1));

    assert!(matches!(
        while_empty,
        RemoteRateLimitDecision::RateLimited { .. }
    ));
    assert!(after_refill.is_allowed());
}

/// P-8 / acceptance: the 6th failed pair attempt in the window is locked out.
#[test]
fn five_auth_failures_lock_the_identity_out_for_thirty_seconds() {
    let limiter = RemoteRateLimiter::default();
    let key = RemoteRateLimitKey::pairing_code("hash-of-code-a");
    let start = Instant::now();

    for _ in 0..5 {
        assert!(limiter.check_at(&key, start).is_allowed());
        limiter.record_failure_at(&key, start);
    }
    let sixth = limiter.check_at(&key, start);
    let during_lockout = limiter.check_at(&key, start + Duration::from_secs(29));
    let after_lockout = limiter.check_at(&key, start + Duration::from_secs(31));

    assert!(matches!(sixth, RemoteRateLimitDecision::LockedOut { .. }));
    assert!(matches!(
        during_lockout,
        RemoteRateLimitDecision::LockedOut { .. }
    ));
    assert!(
        after_lockout.is_allowed(),
        "the lockout must expire, not persist"
    );
}

/// P-8: under Serve two devices redeeming different codes are limited independently, so one
/// peer's brute force cannot lock the owner's other device out of pairing.
#[test]
fn under_serve_one_peers_lockout_does_not_reach_another_devices_pairing_code() {
    let limiter = RemoteRateLimiter::default();
    let attacked = RemoteRateLimitKey::pairing_code("hash-of-code-under-attack");
    let bystander = RemoteRateLimitKey::pairing_code("hash-of-a-different-code");
    let start = Instant::now();

    for _ in 0..6 {
        limiter.record_failure_at(&attacked, start);
    }

    assert!(matches!(
        limiter.check_at(&attacked, start),
        RemoteRateLimitDecision::LockedOut { .. }
    ));
    assert!(
        limiter.check_at(&bystander, start).is_allowed(),
        "a second device's pairing attempt must stay admissible"
    );
}

/// Post-auth the identity is the device, never the socket — same independence guarantee.
#[test]
fn per_device_buckets_are_independent() {
    let limiter = RemoteRateLimiter::default();
    let noisy = RemoteRateLimitKey::device(&RemoteDeviceId::from_string("device-noisy"));
    let quiet = RemoteRateLimitKey::device(&RemoteDeviceId::from_string("device-quiet"));
    let now = Instant::now();

    for _ in 0..10 {
        limiter.check_at(&noisy, now);
    }

    assert!(matches!(
        limiter.check_at(&noisy, now),
        RemoteRateLimitDecision::RateLimited { .. }
    ));
    assert!(limiter.check_at(&quiet, now).is_allowed());
}

#[test]
fn a_successful_auth_clears_the_failure_streak() {
    let limiter = RemoteRateLimiter::default();
    let key = RemoteRateLimitKey::pairing_code("hash-of-code-a");
    let start = Instant::now();
    for _ in 0..4 {
        limiter.record_failure_at(&key, start);
    }

    limiter.record_success(&key);
    for _ in 0..4 {
        limiter.record_failure_at(&key, start);
    }

    assert!(
        limiter.check_at(&key, start).is_allowed(),
        "four failures after a success must not trip the five-failure lockout"
    );
}

/// Serve collapses every peer onto loopback, so the socket address must not become the key.
#[test]
fn serve_mode_never_keys_pre_auth_limiting_on_the_socket_address() {
    let loopback = auth_endpoint_key(RemoteExposureMode::Serve, Some(ip("127.0.0.1")));
    let spoofed_peer = auth_endpoint_key(RemoteExposureMode::Serve, Some(ip("100.64.0.7")));
    let unknown = auth_endpoint_key(RemoteExposureMode::Serve, None);

    assert_eq!(loopback, RemoteRateLimitKey::Global);
    assert_eq!(spoofed_peer, RemoteRateLimitKey::Global);
    assert_eq!(unknown, RemoteRateLimitKey::Global);
}

#[test]
fn direct_tailnet_mode_keys_on_the_real_peer_address() {
    let peer = auth_endpoint_key(
        RemoteExposureMode::TailnetDirect,
        Some(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 7))),
    );
    let loopback = auth_endpoint_key(RemoteExposureMode::TailnetDirect, Some(ip("127.0.0.1")));
    let unknown = auth_endpoint_key(RemoteExposureMode::TailnetDirect, None);

    assert_eq!(peer, RemoteRateLimitKey::Peer("100.64.0.7".to_string()));
    assert_eq!(
        loopback,
        RemoteRateLimitKey::Global,
        "a loopback source in direct mode is a proxy, not a peer identity"
    );
    assert_eq!(unknown, RemoteRateLimitKey::Global);
}

#[test]
fn a_device_cannot_exceed_its_http_concurrency_cap() {
    let limiter = RemoteRateLimiter::default();
    let device = RemoteDeviceId::from_string("device-1");
    let cap = REMOTE_RATE_LIMIT_DEFAULTS.max_in_flight_per_device;

    let slots: Vec<_> = (0..cap)
        .map(|_| {
            limiter
                .acquire_device_slot(&device)
                .expect("slots below the cap should be granted")
        })
        .collect();
    let over_cap = limiter.acquire_device_slot(&device);

    assert!(over_cap.is_none());
    assert_eq!(limiter.in_flight_for(&device), cap);
    drop(slots);
    assert_eq!(limiter.in_flight_for(&device), 0);
    assert!(limiter.acquire_device_slot(&device).is_some());
}

#[test]
fn one_devices_concurrency_cap_does_not_block_another_device() {
    let limiter = RemoteRateLimiter::default();
    let saturated = RemoteDeviceId::from_string("device-saturated");
    let other = RemoteDeviceId::from_string("device-other");
    let _slots: Vec<_> = (0..REMOTE_RATE_LIMIT_DEFAULTS.max_in_flight_per_device)
        .map(|_| {
            limiter
                .acquire_device_slot(&saturated)
                .expect("slots below the cap should be granted")
        })
        .collect();

    assert!(limiter.acquire_device_slot(&saturated).is_none());
    assert!(limiter.acquire_device_slot(&other).is_some());
}

/// Pre-auth keys are per *pairing code*, so a brute-forcer mints a fresh identity per guess.
/// Pruning must bound that growth without ever releasing a live lockout.
#[test]
fn idle_identities_are_pruned_while_live_lockouts_survive() {
    let limiter = RemoteRateLimiter::default();
    let start = Instant::now();
    let locked = RemoteRateLimitKey::pairing_code("hash-of-a-locked-code");
    for _ in 0..5 {
        limiter.record_failure_at(&locked, start);
    }
    for index in 0..6000 {
        limiter.record_failure_at(
            &RemoteRateLimitKey::pairing_code(format!("guess-{index}")),
            start,
        );
    }

    // Well past every one-off guess's lockout, one more failure triggers the sweep.
    let later = start + Duration::from_secs(600);
    limiter.record_failure_at(
        &RemoteRateLimitKey::pairing_code("hash-of-a-fresh-code"),
        later,
    );
    for _ in 0..5 {
        limiter.record_failure_at(&locked, later);
    }
    let (failures, _) = limiter.identity_count();

    assert!(
        failures < 6000,
        "idle identities must be swept, {failures} remained"
    );
    assert!(
        matches!(
            limiter.check_at(&locked, later),
            RemoteRateLimitDecision::LockedOut { .. }
        ),
        "a live lockout must survive pruning"
    );
}
