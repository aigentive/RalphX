//! In-memory live-session registry — the enforcement handle for revocation teardown (§4.4).
//!
//! `remote_sessions` is durable bookkeeping; **this** map is what actually tears a live
//! connection down. Revoking a device, disabling the listener, or narrowing a device's
//! agent-control grant fires the device's kill channels, and every WS task will `select!` on
//! its channel (PR 1.4) so the socket closes immediately rather than at the next HTTP call.

// The admission half of this contract (`register`, `unregister`, and the kill-channel
// receiver) has no production caller until PR 1.4 mounts the WebSocket upgrade. It ships
// with the teardown half deliberately: the cap, the channel, and the revocation path are one
// invariant and are tested together here rather than split across two PRs.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use ralphx_remote_protocol::ResetReason;
use tokio::sync::broadcast;

use crate::domain::entities::{RemoteDeviceId, RemoteSessionId};

/// Per-device cap on concurrent live sessions (§4.4 abuse controls).
///
/// Enforced at WS upgrade in PR 1.4; the admission decision lives here so the cap and the
/// registry can never disagree about how many sessions a device holds.
pub(crate) const MAX_SESSIONS_PER_DEVICE: usize = 8;

/// Kill-channel capacity. One slot is enough: teardown is terminal, not a stream.
const KILL_CHANNEL_CAPACITY: usize = 1;

/// The receiving half handed to a live session.
///
/// PR 1.4's WS task selects on [`RemoteSessionKillChannel::recv`]; when it resolves, the task
/// sends `error{code:"revoked"}` / `reset(host_disabled)` and closes.
pub(crate) struct RemoteSessionKillChannel {
    receiver: broadcast::Receiver<ResetReason>,
}

impl RemoteSessionKillChannel {
    /// Resolves when the host tears this session down.
    ///
    /// `None` means the sender was dropped without a reason — also a teardown, so callers
    /// must close either way rather than treating it as "keep running".
    pub(crate) async fn recv(&mut self) -> Option<ResetReason> {
        self.receiver.recv().await.ok()
    }

    /// Non-blocking probe, used by tests and by the heartbeat re-check path.
    pub(crate) fn try_recv(&mut self) -> Option<ResetReason> {
        self.receiver.try_recv().ok()
    }
}

/// Whether a session may join the registry.
pub(crate) enum RemoteSessionAdmission {
    Admitted(RemoteSessionKillChannel),
    /// The device already holds [`MAX_SESSIONS_PER_DEVICE`] live sessions.
    CapExceeded {
        limit: usize,
    },
}

/// `device_id → {session_id → kill channel}`.
///
/// Cloning shares the same map: the router, the Tauri revoke commands, and the listener
/// lifecycle all act on one registry, never on private copies.
#[derive(Clone, Default)]
pub(crate) struct RemoteSessionRegistry {
    sessions:
        Arc<DashMap<RemoteDeviceId, HashMap<RemoteSessionId, broadcast::Sender<ResetReason>>>>,
}

impl RemoteSessionRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Admits a session, returning its kill channel.
    ///
    /// Re-registering the same session id replaces its channel, so a reconnect under a reused
    /// id can never leave an orphaned sender that teardown would miss.
    pub(crate) fn register(
        &self,
        device_id: &RemoteDeviceId,
        session_id: &RemoteSessionId,
    ) -> RemoteSessionAdmission {
        let mut device_sessions = self.sessions.entry(device_id.clone()).or_default();
        if !device_sessions.contains_key(session_id)
            && device_sessions.len() >= MAX_SESSIONS_PER_DEVICE
        {
            return RemoteSessionAdmission::CapExceeded {
                limit: MAX_SESSIONS_PER_DEVICE,
            };
        }
        let (sender, receiver) = broadcast::channel(KILL_CHANNEL_CAPACITY);
        device_sessions.insert(session_id.clone(), sender);
        RemoteSessionAdmission::Admitted(RemoteSessionKillChannel { receiver })
    }

    /// Drops a session that closed on its own. Idempotent.
    pub(crate) fn unregister(&self, device_id: &RemoteDeviceId, session_id: &RemoteSessionId) {
        let mut empty = false;
        if let Some(mut device_sessions) = self.sessions.get_mut(device_id) {
            device_sessions.remove(session_id);
            empty = device_sessions.is_empty();
        }
        if empty {
            self.sessions
                .remove_if(device_id, |_, value| value.is_empty());
        }
    }

    /// Fires every kill channel for one device and forgets its sessions.
    ///
    /// Returns how many sessions were signalled. Callers must have already written the
    /// durable authority (`revoked_at`, narrowed scopes) — the registry is the *effect*,
    /// never the proof (§4.4 teardown order).
    pub(crate) fn kill_device(&self, device_id: &RemoteDeviceId, reason: ResetReason) -> usize {
        let Some((_, device_sessions)) = self.sessions.remove(device_id) else {
            return 0;
        };
        signal_all(device_sessions, reason)
    }

    /// Fires one session's kill channel and forgets it.
    ///
    /// Returns whether a live channel was actually signalled, so a caller can distinguish
    /// "torn down" from "already gone" instead of reporting success either way.
    pub(crate) fn kill_session(
        &self,
        device_id: &RemoteDeviceId,
        session_id: &RemoteSessionId,
        reason: ResetReason,
    ) -> bool {
        let mut signalled = false;
        let mut empty = false;
        if let Some(mut device_sessions) = self.sessions.get_mut(device_id) {
            if let Some(sender) = device_sessions.remove(session_id) {
                let _ = sender.send(reason);
                signalled = true;
            }
            empty = device_sessions.is_empty();
        }
        if empty {
            self.sessions
                .remove_if(device_id, |_, value| value.is_empty());
        }
        signalled
    }

    /// Fires every kill channel on the host (listener disable).
    pub(crate) fn kill_all(&self, reason: ResetReason) -> usize {
        let device_ids: Vec<RemoteDeviceId> = self
            .sessions
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        device_ids
            .into_iter()
            .map(|device_id| self.kill_device(&device_id, reason))
            .sum()
    }

    /// Live session ids for a device, in no particular order.
    pub(crate) fn live_sessions(&self, device_id: &RemoteDeviceId) -> Vec<RemoteSessionId> {
        self.sessions
            .get(device_id)
            .map(|entry| entry.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn device_session_count(&self, device_id: &RemoteDeviceId) -> usize {
        self.sessions
            .get(device_id)
            .map(|entry| entry.len())
            .unwrap_or(0)
    }

    pub(crate) fn live_session_count(&self) -> usize {
        self.sessions.iter().map(|entry| entry.len()).sum()
    }
}

/// Sends before dropping the senders so a receiver still observes the *reason* rather than a
/// bare channel close.
fn signal_all(
    device_sessions: HashMap<RemoteSessionId, broadcast::Sender<ResetReason>>,
    reason: ResetReason,
) -> usize {
    let mut signalled = 0;
    for (_, sender) in device_sessions.into_iter() {
        // A send error only means the session already dropped its receiver; the session is
        // gone either way, so it still counts as torn down.
        let _ = sender.send(reason);
        signalled += 1;
    }
    signalled
}
