//! Request idempotency for the :3849 invoke facade (§4.3, A-16).
//!
//! Two layers, deliberately with different lifetimes:
//!
//! 1. **In-flight reservation (in memory, `DashMap`).** Claimed atomically before dispatch,
//!    keyed `(device_id, request_id)` and bound to a hash of `cmd` + args. It dies with the
//!    process BY DESIGN: a crashed host has no in-flight dispatch left to coalesce onto, so
//!    resurrecting the claim would only manufacture a permanent 409.
//! 2. **Completed outcomes (durable SQLite).** Consulted FIRST, so a replay after a host
//!    restart returns the cached outcome instead of re-executing.
//!
//! ## The guarantee, stated honestly (A-16)
//!
//! At-most-once **within the TTL**, EXCEPT a host crash in the
//! effect-committed → record-unwritten window of a non-transactional command. The client's
//! `REMOTE_TIMEOUT_UNKNOWN` reconcile path covers that window. This is never unconditional
//! exactly-once, and no code here should be read as claiming it is.
//!
//! ## Fail-closed reads
//!
//! [`RemoteRequestDedupLookup`] is a tri-state, not an `Option`. A store outage surfaces as
//! `Err` and becomes a 500 — it must NEVER be observed as "no record", because "no record" is
//! exactly the state that PERMITS execution.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use ralphx_remote_protocol::{ErrorCode, RiskClass};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::entities::{RemoteDeviceId, RemoteRequestDedupRecord};
use crate::domain::repositories::{RemoteRequestDedupLookup, RemoteRequestDedupRepository};
use crate::remote_server::registry::RemoteInvokeError;

/// How long a completed outcome stays replayable.
///
/// Ten minutes covers a mobile client's reconnect-and-retry window (backgrounded app, tunnel
/// flap) without letting the table become a permanent request archive.
pub(crate) const REMOTE_DEDUP_TTL_SECONDS: i64 = 600;

/// Minimum gap between opportunistic purges.
///
/// Purging is folded into `record` rather than a background task: it needs no runtime handle,
/// cannot outlive the router, and a throttle keeps it off the hot path of a request burst.
pub(crate) const REMOTE_DEDUP_PURGE_INTERVAL_SECONDS: i64 = 60;

/// Maximum `/invoke` request body (C-16).
///
/// Deliberately small: `/invoke` carries command arguments, never payload bytes. Attachments
/// have their own endpoint with its own, larger cap, so raising this would only widen the
/// pre-auth-adjacent parse surface for no functional gain.
pub(crate) const REMOTE_INVOKE_BODY_LIMIT_BYTES: usize = 256 * 1024;

/// Whether a class participates in dedup at all.
///
/// `Read` is exempt by spec: it has no side effect to make at-most-once, and caching reads
/// would serve stale data under an id the client is entitled to reuse.
pub(crate) const fn class_requires_dedup(class: RiskClass) -> bool {
    !matches!(class, RiskClass::Read)
}

/// Canonicalizes a JSON value so key ORDER cannot change the hash.
///
/// `{"a":1,"b":2}` and `{"b":2,"a":1}` are the same request; a client that re-serializes its
/// retry from a hash map must not be told it reused an id. Arrays keep their order — order is
/// semantic there.
///
/// Alias handling is deliberately NOT normalized: the registry accepts both the exact and the
/// camelCase spelling of a parameter (`registry.rs`), but hashing the RAW args means a retry
/// that switches spelling is treated as a DIFFERENT request and rejected with
/// `REMOTE_REQUEST_ID_REUSED` rather than silently served the other spelling's cached result.
/// A client that retries by resending its own recorded request — which the wire contract
/// requires — is stable under this.
fn canonical_json(value: &Value) -> String {
    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort_unstable();
                out.push('{');
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&Value::String(key.clone()).to_string());
                    out.push(':');
                    write(&map[key], out);
                }
                out.push('}');
            }
            Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }

    let mut out = String::new();
    write(value, &mut out);
    out
}

/// Binds a request id to the work it asked for.
///
/// The command name is length-prefixed so `cmd`/`args` boundaries cannot be shifted to make
/// two different requests hash alike.
pub(crate) fn args_hash(command: &str, args: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update((command.len() as u64).to_be_bytes());
    hasher.update(command.as_bytes());
    hasher.update(canonical_json(args).as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ReservationKey {
    device_id: String,
    request_id: String,
}

/// What consulting the durable table and the in-flight map decided.
pub(crate) enum DedupDecision {
    /// Nothing has this id; the caller holds the reservation and must dispatch.
    Proceed(RemoteDedupReservation),
    /// A completed outcome exists — replay it, do not dispatch.
    Replay(RemoteRequestDedupRecord),
}

/// An owned in-flight reservation.
///
/// Dropping it releases the exact `(device_id, request_id)` slot, including when the owning
/// dispatch task is cancelled or panics.
pub(crate) struct RemoteDedupReservation {
    state: Arc<RemoteDedupState>,
    device_id: RemoteDeviceId,
    request_id: String,
}

impl RemoteDedupReservation {
    /// Records a completed command outcome. The reservation remains held through the durable
    /// write and is released by `Drop` when this method returns.
    pub(crate) async fn complete(self, command: &str, args: &Value, record: RecordedOutcome) {
        self.state
            .complete(&self.device_id, &self.request_id, command, args, record)
            .await;
    }
}

impl Drop for RemoteDedupReservation {
    fn drop(&mut self) {
        self.state.release(&self.device_id, &self.request_id);
    }
}

/// The in-flight reservation map plus the durable store.
pub(crate) struct RemoteDedupState {
    in_flight: DashMap<ReservationKey, String>,
    store: Arc<dyn RemoteRequestDedupRepository>,
    ttl_seconds: i64,
    /// Unix seconds of the last opportunistic purge; 0 means "never".
    last_purge_epoch: AtomicI64,
}

impl RemoteDedupState {
    pub(crate) fn new(store: Arc<dyn RemoteRequestDedupRepository>) -> Self {
        Self {
            in_flight: DashMap::new(),
            store,
            ttl_seconds: REMOTE_DEDUP_TTL_SECONDS,
            last_purge_epoch: AtomicI64::new(0),
        }
    }

    /// Consults the durable table, then atomically claims the in-flight slot.
    ///
    /// Order matters: durable FIRST. A completed outcome outranks any live claim, so a replay
    /// arriving while a stale reservation is somehow still held still gets its answer.
    pub(crate) async fn begin(
        self: &Arc<Self>,
        device_id: &RemoteDeviceId,
        request_id: &str,
        command: &str,
        args: &Value,
    ) -> Result<DedupDecision, RemoteInvokeError> {
        if request_id.is_empty() {
            return Err(RemoteInvokeError {
                code: ErrorCode::RemoteInvalidArguments,
                message: "requestId must be a non-empty string.".to_string(),
            });
        }
        let hash = args_hash(command, args);
        let now = now_rfc3339();

        // Fail closed: a store error is a 500, never "no record → execute".
        let lookup = self
            .store
            .lookup(device_id, request_id, &now)
            .await
            .map_err(|error| {
                tracing::error!(
                    device_id = %device_id.0,
                    %request_id,
                    %error,
                    "remote dedup lookup failed; refusing to dispatch"
                );
                RemoteInvokeError::internal(
                    "The idempotency store is unavailable; the request was not executed.",
                )
            })?;

        match lookup {
            RemoteRequestDedupLookup::Fresh(record) => {
                if record.args_hash != hash {
                    return Err(reused_id_error());
                }
                return Ok(DedupDecision::Replay(record));
            }
            // Past the TTL, or never seen: both mean "execute", and both still have to win the
            // in-flight claim below.
            RemoteRequestDedupLookup::Expired | RemoteRequestDedupLookup::Absent => {}
        }

        let key = ReservationKey {
            device_id: device_id.0.clone(),
            request_id: request_id.to_string(),
        };
        match self.in_flight.entry(key) {
            Entry::Occupied(occupied) => {
                if occupied.get() == &hash {
                    // Coalescing policy: report in-progress immediately rather than awaiting the
                    // winner. `AgentControl` dispatches can run for minutes; parking a second
                    // connection on them would convert a duplicate tap into a held socket and a
                    // client-side timeout that looks like a host fault. 409 is retryable and the
                    // client already knows how to back off.
                    tracing::warn!(
                        device_id = %device_id.0,
                        %request_id,
                        "remote request retry arrived while the original dispatch is in progress"
                    );
                    Err(RemoteInvokeError {
                        code: ErrorCode::RemoteRequestInProgress,
                        message: "This requestId is already executing on the host.".to_string(),
                    })
                } else {
                    Err(reused_id_error())
                }
            }
            Entry::Vacant(vacant) => {
                vacant.insert(hash);
                let reservation = RemoteDedupReservation {
                    state: Arc::clone(self),
                    device_id: device_id.clone(),
                    request_id: request_id.to_string(),
                };

                // A completion can land after the first lookup returned `Absent` but before this
                // request won the in-flight claim. Re-check while holding the claim so that stale
                // observation cannot authorize a second dispatch.
                let lookup = self
                    .store
                    .lookup(device_id, request_id, &now_rfc3339())
                    .await
                    .map_err(|error| {
                        tracing::error!(
                            device_id = %device_id.0,
                            %request_id,
                            %error,
                            "remote dedup double-check failed; refusing to dispatch"
                        );
                        RemoteInvokeError::internal(
                            "The idempotency store is unavailable; the request was not executed.",
                        )
                    })?;
                match lookup {
                    RemoteRequestDedupLookup::Fresh(record) => {
                        if record.args_hash != args_hash(command, args) {
                            return Err(reused_id_error());
                        }
                        Ok(DedupDecision::Replay(record))
                    }
                    RemoteRequestDedupLookup::Expired | RemoteRequestDedupLookup::Absent => {
                        Ok(DedupDecision::Proceed(reservation))
                    }
                }
            }
        }
    }

    /// Records a COMPLETED outcome and releases the reservation.
    ///
    /// Called for both `DispatchOutcome::Ok` and `DispatchOutcome::Err`: a command-level
    /// business error is a completed outcome, and replaying it must return that error rather
    /// than run the command a second time.
    ///
    /// A `record` failure is logged and swallowed — the effect ALREADY happened, so failing the
    /// response would tell the client nothing ran. The honest residual is that such a replay
    /// re-executes; that is the same window as a crash before the write, and it is why the
    /// guarantee above is stated as at-most-once-with-an-exception.
    pub(crate) async fn complete(
        &self,
        device_id: &RemoteDeviceId,
        request_id: &str,
        command: &str,
        args: &Value,
        record: RecordedOutcome,
    ) {
        let now = now_rfc3339();
        let row = RemoteRequestDedupRecord {
            device_id: device_id.clone(),
            request_id: request_id.to_string(),
            args_hash: args_hash(command, args),
            outcome: record.kind,
            response: record.body,
            created_at: now.clone(),
            expires_at: rfc3339_offset_seconds(self.ttl_seconds),
        };
        if let Err(error) = self.store.record(row).await {
            tracing::error!(
                device_id = %device_id.0,
                %request_id,
                %error,
                "remote dedup record failed after the command already executed; a replay of \
                 this requestId will re-execute"
            );
        }
        self.purge_if_due(&now).await;
    }

    /// Releases a reservation WITHOUT writing a durable record.
    ///
    /// Used for facade errors (`REMOTE_FORBIDDEN`, `REMOTE_COMMAND_UNAVAILABLE`, bad args):
    /// nothing executed, so caching the rejection would trap the client — a corrected retry
    /// with the same id (the natural client behaviour after a 400/403) must be allowed to run.
    pub(crate) fn release(&self, device_id: &RemoteDeviceId, request_id: &str) {
        self.in_flight.remove(&ReservationKey {
            device_id: device_id.0.clone(),
            request_id: request_id.to_string(),
        });
    }

    pub(crate) fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    async fn purge_if_due(&self, now: &str) {
        let epoch = chrono::Utc::now().timestamp();
        let previous = self.last_purge_epoch.load(Ordering::Relaxed);
        if previous != 0 && epoch - previous < REMOTE_DEDUP_PURGE_INTERVAL_SECONDS {
            return;
        }
        if self
            .last_purge_epoch
            .compare_exchange(previous, epoch, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            // Another request just claimed the purge slot.
            return;
        }
        if let Err(error) = self.store.purge_expired(now).await {
            tracing::warn!(%error, "remote dedup purge failed; rows will be retried later");
        }
    }
}

/// The completed outcome to persist.
pub(crate) struct RecordedOutcome {
    pub kind: crate::domain::entities::RemoteDedupOutcomeKind,
    pub body: String,
}

fn reused_id_error() -> RemoteInvokeError {
    RemoteInvokeError {
        code: ErrorCode::RemoteRequestIdReused,
        message: "This requestId was already used for a different command or arguments."
            .to_string(),
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn rfc3339_offset_seconds(seconds: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(seconds))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}
