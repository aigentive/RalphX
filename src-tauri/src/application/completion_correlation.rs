//! Deduplication state for agent-completion events delivered twice.
//!
//! Completion events arrive over both the Tauri event channel and the internal
//! event bus. This registry correlates the two deliveries so consumers act
//! once. It holds no Tauri types, so the shell runtime and the command-layer
//! dispatcher can both depend on it without an upward import.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompletionCorrelationSource {
    Tauri,
    Bus,
}

#[derive(Default)]
struct SeenSources {
    tauri: bool,
    bus: bool,
}

impl SeenSources {
    fn mark(&mut self, source: CompletionCorrelationSource) {
        match source {
            CompletionCorrelationSource::Tauri => self.tauri = true,
            CompletionCorrelationSource::Bus => self.bus = true,
        }
    }

    fn is_complete(&self) -> bool {
        self.tauri && self.bus
    }
}

struct CorrelationEntry {
    event_id: Uuid,
    event: String,
    payload: Value,
    created_at: Instant,
    seen_sources: SeenSources,
}

struct CorrelationState {
    entries: VecDeque<CorrelationEntry>,
}

/// Bounded side channel that maps unchanged Tauri payloads back to bus envelope IDs.
pub(crate) struct CompletionCorrelationRegistry {
    state: Mutex<CorrelationState>,
    ttl: Duration,
    capacity: usize,
    now: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl CompletionCorrelationRegistry {
    pub(crate) fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            state: Mutex::new(CorrelationState {
                entries: VecDeque::new(),
            }),
            ttl,
            capacity,
            now: Arc::new(Instant::now),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_clock(
        ttl: Duration,
        capacity: usize,
        now: Arc<dyn Fn() -> Instant + Send + Sync>,
    ) -> Self {
        Self {
            state: Mutex::new(CorrelationState {
                entries: VecDeque::new(),
            }),
            ttl,
            capacity,
            now,
        }
    }

    /// Reserves a correlation ID in FIFO order. A full live registry fails closed.
    #[cfg(test)]
    pub(crate) fn reserve(&self, event: &str, payload: &Value) -> Option<Uuid> {
        let event_id = Uuid::new_v4();
        self.reserve_existing(event_id, event, payload)
            .then_some(event_id)
    }

    /// Reserves the already-created envelope identity before either transport receives it.
    pub(crate) fn reserve_existing(&self, event_id: Uuid, event: &str, payload: &Value) -> bool {
        let now = (self.now)();
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        Self::purge_expired(&mut state.entries, now, self.ttl);
        if state.entries.len() >= self.capacity {
            return false;
        }

        state.entries.push_back(CorrelationEntry {
            event_id,
            event: event.to_string(),
            payload: payload.clone(),
            created_at: now,
            seen_sources: SeenSources::default(),
        });
        true
    }

    /// Resolves the earliest matching Tauri callback and records its source.
    pub(crate) fn resolve_tauri(&self, event: &str, payload: &Value) -> Option<Uuid> {
        let now = (self.now)();
        let mut state = self.state.lock().ok()?;
        Self::purge_expired(&mut state.entries, now, self.ttl);
        let index = state.entries.iter().position(|entry| {
            entry.event == event && entry.payload == *payload && !entry.seen_sources.tauri
        })?;
        let entry = state.entries.get_mut(index)?;
        entry.seen_sources.mark(CompletionCorrelationSource::Tauri);
        let event_id = entry.event_id;
        if entry.seen_sources.is_complete() {
            state.entries.remove(index);
        }
        Some(event_id)
    }

    /// Records receipt from the bus. It intentionally does not create new entries.
    pub(crate) fn mark_source(&self, event_id: Uuid, source: CompletionCorrelationSource) -> bool {
        let now = (self.now)();
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        Self::purge_expired(&mut state.entries, now, self.ttl);
        let Some(index) = state
            .entries
            .iter()
            .position(|entry| entry.event_id == event_id)
        else {
            return false;
        };
        let entry = &mut state.entries[index];
        entry.seen_sources.mark(source);
        if entry.seen_sources.is_complete() {
            state.entries.remove(index);
        }
        true
    }

    /// Removes a reservation after its Tauri delivery failed.
    pub(crate) fn remove_tauri_reservation(&self, event_id: Uuid) -> bool {
        let now = (self.now)();
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        Self::purge_expired(&mut state.entries, now, self.ttl);
        let Some(index) = state
            .entries
            .iter()
            .position(|entry| entry.event_id == event_id)
        else {
            return false;
        };
        state.entries.remove(index);
        true
    }

    pub(crate) fn len(&self) -> usize {
        let now = (self.now)();
        let Ok(mut state) = self.state.lock() else {
            return 0;
        };
        Self::purge_expired(&mut state.entries, now, self.ttl);
        state.entries.len()
    }

    fn purge_expired(entries: &mut VecDeque<CorrelationEntry>, now: Instant, ttl: Duration) {
        entries.retain(|entry| {
            now.checked_duration_since(entry.created_at)
                .is_none_or(|age| age < ttl)
        });
    }
}
