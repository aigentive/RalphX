//! Rate limiting and abuse controls for the remote listener (§4.4).
//!
//! Parameters mirror the external-MCP limiter (`plugins/app/ralphx-external-mcp/src/
//! rate-limiter.ts:14-17,45-114`): 10 rps token bucket, 5 consecutive auth failures → 30 s
//! lockout.
//!
//! **Serve identity boundary.** Under Tailscale Serve every request arrives from loopback, so
//! keying on the socket address would let one tailnet peer lock every device out and would
//! make `remote_addr` meaningless. Pre-auth limiting therefore keys on a global bucket plus
//! the *presented pairing code* (so two devices redeeming different codes lock out
//! independently), and post-auth limiting keys on `device_id`. Only direct-tailnet mode,
//! where the source really is the peer's `100.x` address, keys on the peer.
//!
//! **Lockouts never ride on the global key.** The `Global` bucket is shared by every peer, so
//! a punitive lockout stored there would deny pairing and ws-ticket minting host-wide. Auth
//! failures on the authenticated routes therefore accrue on the *presented token hash*
//! ([`RemoteRateLimitKey::BearerToken`]), never on `Global` (§4.4, P-8).

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

use crate::domain::entities::RemoteDeviceId;
use crate::remote_server::settings::RemoteExposureMode;

/// Tunables for the remote limiter, sourced from the external-MCP limiter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RemoteRateLimitParams {
    pub requests_per_second: f64,
    pub auth_failures_before_lockout: u32,
    pub lockout: Duration,
    pub max_in_flight_per_device: usize,
}

/// Soft cap on tracked identities. Pre-auth keying is by *presented pairing code*, so a
/// brute-forcer mints a fresh key per guess; without a bound the maps would grow without
/// limit. Exceeding the cap drops entries that are already idle — never a live lockout.
const IDENTITY_MAP_SOFT_CAP: usize = 4096;

pub(crate) const REMOTE_RATE_LIMIT_DEFAULTS: RemoteRateLimitParams = RemoteRateLimitParams {
    requests_per_second: 10.0,
    auth_failures_before_lockout: 5,
    lockout: Duration::from_secs(30),
    max_in_flight_per_device: 8,
};

/// What a bucket is keyed on. Never the socket address under Serve (§4.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RemoteRateLimitKey {
    /// Coarse flood ceiling shared by all pre-auth callers under Serve.
    ///
    /// **Bucket only.** Nothing may ever `record_failure` on this key: the pre-auth
    /// middleware consults it for every `/remote/v1/auth/*` request, so a lockout parked
    /// here would let one peer block pairing and ws-ticket minting for every device — the
    /// exact cross-device denial §4.4 keys the lockout on identity to prevent (P-8).
    Global,
    /// The SHA-256 of a presented pairing code — the identity that carries the lockout, so
    /// one peer's brute force cannot lock out a different device's pairing attempt.
    PairingCode(String),
    /// The SHA-256 of a presented **bearer token**: the post-auth failure identity.
    ///
    /// Separate namespace from [`Self::Global`] on purpose. Bearer rejections on the
    /// authenticated routes accrue here, so a garbage-bearer flood cannot spill its lockout
    /// onto the pre-auth endpoints. Guessing throughput stays bounded by the pre-auth
    /// bucket, not by this key.
    BearerToken(String),
    /// A paired device, post-auth.
    Device(String),
    /// A real tailnet peer address; direct-tailnet exposure only.
    Peer(String),
}

impl RemoteRateLimitKey {
    pub(crate) fn device(device_id: &RemoteDeviceId) -> Self {
        Self::Device(device_id.to_string())
    }

    pub(crate) fn pairing_code(code_hash: impl Into<String>) -> Self {
        Self::PairingCode(code_hash.into())
    }

    pub(crate) fn bearer_token(token_hash: impl Into<String>) -> Self {
        Self::BearerToken(token_hash.into())
    }
}

/// Chooses the pre-auth limiting identity for the active exposure mode.
///
/// Serve collapses every peer onto loopback, so the socket address is deliberately ignored
/// there; the punitive lockout rides on the pairing-code identity instead.
pub(crate) fn auth_endpoint_key(
    exposure_mode: RemoteExposureMode,
    peer: Option<IpAddr>,
) -> RemoteRateLimitKey {
    match (exposure_mode, peer) {
        (RemoteExposureMode::TailnetDirect, Some(peer)) if !peer.is_loopback() => {
            RemoteRateLimitKey::Peer(peer.to_string())
        }
        _ => RemoteRateLimitKey::Global,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteRateLimitDecision {
    Allowed,
    /// The token bucket is empty.
    RateLimited {
        retry_after_secs: u64,
    },
    /// Consecutive auth failures tripped the lockout.
    LockedOut {
        retry_after_secs: u64,
    },
}

impl RemoteRateLimitDecision {
    pub(crate) fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }

    pub(crate) fn retry_after_secs(self) -> Option<u64> {
        match self {
            Self::Allowed => None,
            Self::RateLimited { retry_after_secs } | Self::LockedOut { retry_after_secs } => {
                Some(retry_after_secs)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BucketState {
    tokens: f64,
    last_refill_at: Instant,
}

#[derive(Debug, Clone, Copy, Default)]
struct FailureState {
    failure_count: u32,
    locked_until: Option<Instant>,
    /// When the streak last grew. Kept so pruning can retain a *sub-lockout* streak: an
    /// attacker minting fresh identities must not be able to overflow the map and wipe the
    /// 4-failures-so-far state it is about to trip.
    last_failure_at: Option<Instant>,
}

/// RAII guard for a device's HTTP concurrency slot.
///
/// Releasing on drop means a panicking or cancelled handler cannot leak capacity.
pub(crate) struct RemoteDeviceSlot {
    in_flight: Arc<DashMap<String, usize>>,
    device_id: String,
}

impl Drop for RemoteDeviceSlot {
    fn drop(&mut self) {
        if let Some(mut count) = self.in_flight.get_mut(&self.device_id) {
            *count = count.saturating_sub(1);
        }
        self.in_flight
            .remove_if(&self.device_id, |_, count| *count == 0);
    }
}

/// Token buckets, failure lockouts, and per-device concurrency for the remote listener.
///
/// Cloning shares state — the router holds one limiter for the listener's lifetime.
#[derive(Clone)]
pub(crate) struct RemoteRateLimiter {
    params: RemoteRateLimitParams,
    buckets: Arc<DashMap<RemoteRateLimitKey, BucketState>>,
    failures: Arc<DashMap<RemoteRateLimitKey, FailureState>>,
    in_flight: Arc<DashMap<String, usize>>,
}

impl Default for RemoteRateLimiter {
    fn default() -> Self {
        Self::new(REMOTE_RATE_LIMIT_DEFAULTS)
    }
}

impl RemoteRateLimiter {
    pub(crate) fn new(params: RemoteRateLimitParams) -> Self {
        Self {
            params,
            buckets: Arc::new(DashMap::new()),
            failures: Arc::new(DashMap::new()),
            in_flight: Arc::new(DashMap::new()),
        }
    }

    /// Checks lockout then the token bucket for `key`, consuming one token when allowed.
    pub(crate) fn check(&self, key: &RemoteRateLimitKey) -> RemoteRateLimitDecision {
        self.check_at(key, Instant::now())
    }

    /// Deterministic variant used by tests; production passes `Instant::now()`.
    pub(crate) fn check_at(
        &self,
        key: &RemoteRateLimitKey,
        now: Instant,
    ) -> RemoteRateLimitDecision {
        if let Some(retry_after_secs) = self.lockout_remaining_at(key, now) {
            return RemoteRateLimitDecision::LockedOut { retry_after_secs };
        }

        let mut bucket = self.buckets.entry(key.clone()).or_insert(BucketState {
            tokens: self.params.requests_per_second,
            last_refill_at: now,
        });
        let elapsed = now.saturating_duration_since(bucket.last_refill_at);
        bucket.tokens = (bucket.tokens + elapsed.as_secs_f64() * self.params.requests_per_second)
            .min(self.params.requests_per_second);
        bucket.last_refill_at = now;

        if bucket.tokens < 1.0 {
            // Time to earn one whole token back, rounded up to a whole second.
            let deficit = 1.0 - bucket.tokens;
            let seconds = (deficit / self.params.requests_per_second).ceil() as u64;
            return RemoteRateLimitDecision::RateLimited {
                retry_after_secs: seconds.max(1),
            };
        }
        bucket.tokens -= 1.0;
        RemoteRateLimitDecision::Allowed
    }

    /// Records a rejected auth attempt for `key`, tripping the lockout at the threshold.
    pub(crate) fn record_failure(&self, key: &RemoteRateLimitKey) {
        self.record_failure_at(key, Instant::now());
    }

    pub(crate) fn record_failure_at(&self, key: &RemoteRateLimitKey, now: Instant) {
        debug_assert!(
            !matches!(key, RemoteRateLimitKey::Global),
            "the pre-auth Global key is a bucket only; a lockout here denies every device"
        );
        self.prune_idle_identities(now);
        let mut state = self.failures.entry(key.clone()).or_default();
        // Clear an expired lockout first so a stale one does not compound.
        if state.locked_until.is_some_and(|until| now >= until) {
            *state = FailureState::default();
        }
        state.failure_count = state.failure_count.saturating_add(1);
        state.last_failure_at = Some(now);
        if state.failure_count >= self.params.auth_failures_before_lockout {
            state.locked_until = Some(now + self.params.lockout);
        }
    }

    /// Clears the failure streak after a successful auth.
    pub(crate) fn record_success(&self, key: &RemoteRateLimitKey) {
        self.failures.remove(key);
    }

    fn lockout_remaining_at(&self, key: &RemoteRateLimitKey, now: Instant) -> Option<u64> {
        let mut state = self.failures.get_mut(key)?;
        let locked_until = state.locked_until?;
        if now >= locked_until {
            *state = FailureState::default();
            return None;
        }
        Some(
            locked_until
                .saturating_duration_since(now)
                .as_secs()
                .saturating_add(1),
        )
    }

    /// Reserves one of the device's in-flight HTTP slots.
    ///
    /// `None` means the device is at its concurrency cap; the caller must refuse rather than
    /// queue, so a single device cannot pin the listener's request capacity.
    pub(crate) fn acquire_device_slot(
        &self,
        device_id: &RemoteDeviceId,
    ) -> Option<RemoteDeviceSlot> {
        let key = device_id.to_string();
        let mut count = self.in_flight.entry(key.clone()).or_insert(0);
        if *count >= self.params.max_in_flight_per_device {
            return None;
        }
        *count += 1;
        drop(count);
        Some(RemoteDeviceSlot {
            in_flight: Arc::clone(&self.in_flight),
            device_id: key,
        })
    }

    /// Bounds memory by dropping identities that hold no live lockout, no recent failure,
    /// and no drained bucket. An identity currently locked out, mid-streak within the
    /// lockout window, or still short of tokens is always kept, so pruning can never hand a
    /// flooder a fresh budget.
    fn prune_idle_identities(&self, now: Instant) {
        if self.failures.len() > IDENTITY_MAP_SOFT_CAP {
            let lockout = self.params.lockout;
            self.failures.retain(|_, state| {
                state.locked_until.is_some_and(|until| now < until)
                    // A sub-lockout streak is live state too: dropping it would reset the
                    // flooder's own budget, which is what the cap must never do.
                    || state
                        .last_failure_at
                        .is_some_and(|at| now.saturating_duration_since(at) < lockout)
            });
        }
        if self.buckets.len() > IDENTITY_MAP_SOFT_CAP {
            let capacity = self.params.requests_per_second;
            let rate = self.params.requests_per_second;
            self.buckets.retain(|_, bucket| {
                let refilled = bucket.tokens
                    + now
                        .saturating_duration_since(bucket.last_refill_at)
                        .as_secs_f64()
                        * rate;
                refilled < capacity
            });
        }
    }

    #[cfg(test)]
    pub(crate) fn identity_count(&self) -> (usize, usize) {
        (self.failures.len(), self.buckets.len())
    }

    #[cfg(test)]
    pub(crate) fn in_flight_for(&self, device_id: &RemoteDeviceId) -> usize {
        self.in_flight
            .get(&device_id.to_string())
            .map(|count| *count)
            .unwrap_or(0)
    }
}
