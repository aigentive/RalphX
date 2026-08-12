use std::sync::Arc;
use std::time::Duration;

use ralphx_events::catalog::is_agent_completion_event;
use ralphx_events::{EventEnvelope, EventSink, InternalEventBus};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime};

use crate::application::completion_correlation::{
    CompletionCorrelationRegistry,
};
use crate::infrastructure::agents::claude::stream_timeouts;

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
#[doc(hidden)]
pub struct AgentCompletionEventRuntime {
    pub sink: Arc<dyn EventSink>,
    pub bus: InternalEventBus,
    pub(crate) correlation: Arc<CompletionCorrelationRegistry>,
}

#[doc(hidden)]
pub fn create_agent_completion_event_runtime<R: Runtime>(
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
