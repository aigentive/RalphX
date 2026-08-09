use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ralphx_events::catalog::is_agent_completion_event;
use ralphx_events::{EventEnvelope, EventSink, InternalEventBus};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

use crate::infrastructure::agents::claude::stream_timeouts;

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

pub(crate) trait TauriCompletionEventEmitter: Clone + Send + Sync + 'static {
    fn emit_completion_event(&self, event: &str, payload: &Value) -> Result<(), String>;
}

impl<R: Runtime> TauriCompletionEventEmitter for AppHandle<R> {
    fn emit_completion_event(&self, event: &str, payload: &Value) -> Result<(), String> {
        self.emit(event, payload).map_err(|error| error.to_string())
    }
}

/// Emits unchanged frontend events and one shared envelope for the internal bus.
pub(crate) struct CorrelatedTauriBusEventSink<E: TauriCompletionEventEmitter> {
    emitter: E,
    bus: InternalEventBus,
    correlation: Arc<CompletionCorrelationRegistry>,
}

impl<E: TauriCompletionEventEmitter> CorrelatedTauriBusEventSink<E> {
    pub(crate) fn new(
        emitter: E,
        bus: InternalEventBus,
        correlation: Arc<CompletionCorrelationRegistry>,
    ) -> Self {
        Self {
            emitter,
            bus,
            correlation,
        }
    }
}

impl<E: TauriCompletionEventEmitter> EventSink for CorrelatedTauriBusEventSink<E> {
    fn emit(&self, event: &str, payload: Value) {
        let envelope = EventEnvelope::new(event, payload.clone());
        let reservation = is_agent_completion_event(event)
            .then(|| {
                self.correlation
                    .reserve_existing(envelope.event_id, event, &payload)
                    .then_some(envelope.event_id)
            })
            .flatten();

        if let Err(error) = self.emitter.emit_completion_event(event, &payload) {
            if let Some(event_id) = reservation {
                let removed = self.correlation.remove_tauri_reservation(event_id);
                tracing::warn!(
                    %event_id,
                    removed,
                    %error,
                    "Tauri completion delivery failed; removed correlation reservation"
                );
            } else {
                tracing::warn!(event, %error, "Tauri event delivery failed");
            }
        } else if is_agent_completion_event(event) && reservation.is_none() {
            tracing::warn!(
                event,
                "Completion correlation reservation refused; Tauri automation delivery is unavailable"
            );
        }

        if let Err(unpublished) = self.bus.publish(envelope) {
            tracing::debug!(
                event = unpublished.name,
                event_id = %unpublished.event_id,
                "Internal event bus had no subscribers"
            );
        }
    }
}

/// Shared production event infrastructure for the Tauri and HTTP AppState graphs.
pub(crate) struct AgentCompletionEventRuntime {
    pub(crate) sink: Arc<dyn EventSink>,
    pub(crate) bus: InternalEventBus,
    pub(crate) correlation: Arc<CompletionCorrelationRegistry>,
}

pub(crate) fn create_agent_completion_event_runtime<R: Runtime>(
    app_handle: AppHandle<R>,
) -> AgentCompletionEventRuntime {
    let config = stream_timeouts();
    let bus = InternalEventBus::new();
    let correlation = Arc::new(CompletionCorrelationRegistry::new(
        Duration::from_secs(config.agent_completion_correlation_ttl_secs),
        config.agent_completion_correlation_capacity,
    ));
    let sink: Arc<dyn EventSink> = Arc::new(CorrelatedTauriBusEventSink::new(
        app_handle,
        bus.clone(),
        Arc::clone(&correlation),
    ));
    AgentCompletionEventRuntime {
        sink,
        bus,
        correlation,
    }
}
