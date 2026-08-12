use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ralphx_events::catalog::{AGENT_RUN_COMPLETED, AGENT_TURN_COMPLETED};
use ralphx_events::{EventEnvelope, InternalEventBus};
use serde::Deserialize;
use serde_json::Value;
use tauri::{Listener, Runtime};
use uuid::Uuid;

use crate::domain::entities::{AgentRunId, ChatContextType, ChatConversationId};
use crate::application::completion_correlation::{
    CompletionCorrelationRegistry, CompletionCorrelationSource,
};

type CompletionConsumer = Arc<dyn Fn(&CompletionDispatchEvent) + Send + Sync>;
type MonotonicClock = Arc<dyn Fn() -> Duration + Send + Sync>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionDeliverySource {
    Tauri,
    Bus,
}

impl CompletionDeliverySource {
    fn bit(self) -> u8 {
        match self {
            Self::Tauri => 0b01,
            Self::Bus => 0b10,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Tauri => "tauri",
            Self::Bus => "bus",
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AgentCompletionPayload {
    pub(crate) conversation_id: String,
    pub(crate) context_type: ChatContextType,
    pub(crate) run_id: Option<String>,
}
#[derive(Debug, Clone)]
pub(crate) struct CompletionDispatchEvent {
    pub(crate) event_id: Uuid,
    pub(crate) event_name: &'static str,
    pub(crate) conversation_id: ChatConversationId,
    pub(crate) run_id: Option<AgentRunId>,
    pub(crate) payload: AgentCompletionPayload,
}
#[derive(Clone)]
pub(crate) struct CompletionConsumers {
    auto_review: CompletionConsumer,
    auto_publish: CompletionConsumer,
    pr_supervision_recovery: CompletionConsumer,
}

impl CompletionConsumers {
    pub(crate) fn new(
        auto_review: CompletionConsumer,
        auto_publish: CompletionConsumer,
        pr_supervision_recovery: CompletionConsumer,
    ) -> Self {
        Self {
            auto_review,
            auto_publish,
            pr_supervision_recovery,
        }
    }
    fn schedule(&self, event: &CompletionDispatchEvent) {
        (self.auto_review)(event);
        (self.auto_publish)(event);
        if event.event_name == AGENT_RUN_COMPLETED && event.run_id.is_some() {
            (self.pr_supervision_recovery)(event);
        }
    }
}
fn completion_consumers_for_app_handle<R>(app_handle: tauri::AppHandle<R>) -> CompletionConsumers
where
    R: tauri::Runtime,
{
    let review_handle = app_handle.clone();
    let publish_handle = app_handle.clone();
    CompletionConsumers::new(
        Arc::new(move |event| {
            tracing::debug!(
                event_id = %event.event_id,
                event_name = event.event_name,
                conversation_id = %event.conversation_id,
                "Scheduling agent workspace auto-review from claimed completion"
            );
            super::agent_workspace_auto_review::spawn_auto_review_from_completion_payload(
                review_handle.clone(),
                event.event_name,
                event.conversation_id.clone(),
            );
        }),
        Arc::new(move |event| {
            super::agent_workspace_auto_publish::spawn_auto_publish_from_completion_payload(
                publish_handle.clone(),
                event.event_name,
                event.conversation_id.clone(),
            );
        }),
        Arc::new(move |event| {
            super::agent_workspace_auto_publish::
                spawn_pr_supervision_recovery_from_completion_payload(
                    app_handle.clone(),
                    event.payload.clone(),
                );
        }),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionDispatchOutcome {
    Scheduled,
    Duplicate,
    CapacityExhausted,
    Ignored,
}
#[derive(Debug, Clone)]
struct ProcessedCompletionEvent {
    event_id: Uuid,
    first_seen_at: Duration,
    seen_sources: u8,
    claimed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionClaimOutcome {
    Claimed,
    Duplicate,
    CapacityExhausted,
}
pub(crate) struct ProcessedCompletionEvents {
    entries: Mutex<VecDeque<ProcessedCompletionEvent>>,
    ttl: Duration,
    capacity: usize,
    clock: MonotonicClock,
}

impl ProcessedCompletionEvents {
    pub(crate) fn new() -> Self {
        let config = crate::infrastructure::agents::claude::stream_timeouts();
        let started_at = Instant::now();
        Self::with_limits(
            Duration::from_secs(config.agent_completion_processed_ttl_secs),
            config.agent_completion_processed_capacity,
            Arc::new(move || started_at.elapsed()),
        )
    }
    pub(crate) fn with_limits(ttl: Duration, capacity: usize, clock: MonotonicClock) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            ttl,
            capacity,
            clock,
        }
    }
    #[cfg(test)]
    pub(crate) fn observe_and_claim(
        &self,
        event_id: Uuid,
        source: CompletionDeliverySource,
    ) -> bool {
        matches!(
            self.observe_and_claim_outcome(event_id, source),
            CompletionClaimOutcome::Claimed
        )
    }
    fn observe_and_claim_outcome(
        &self,
        event_id: Uuid,
        source: CompletionDeliverySource,
    ) -> CompletionClaimOutcome {
        let now = (self.clock)();
        let Ok(mut entries) = self.entries.lock() else {
            tracing::error!(
                event_id = %event_id,
                source = source.as_str(),
                "Completion claim registry lock was poisoned; failing closed"
            );
            return CompletionClaimOutcome::CapacityExhausted;
        };

        entries.retain(|entry| now.saturating_sub(entry.first_seen_at) < self.ttl);
        if let Some(entry) = entries.iter_mut().find(|entry| entry.event_id == event_id) {
            entry.seen_sources |= source.bit();
            if entry.claimed {
                return CompletionClaimOutcome::Duplicate;
            }
            entry.claimed = true;
            return CompletionClaimOutcome::Claimed;
        }

        if entries.len() >= self.capacity {
            tracing::warn!(
                event_id = %event_id,
                source = source.as_str(),
                capacity = self.capacity,
                "Completion claim registry is full; failing closed"
            );
            return CompletionClaimOutcome::CapacityExhausted;
        }

        entries.push_back(ProcessedCompletionEvent {
            event_id,
            first_seen_at: now,
            seen_sources: source.bit(),
            claimed: true,
        });
        CompletionClaimOutcome::Claimed
    }
}
impl Default for ProcessedCompletionEvents {
    fn default() -> Self {
        Self::new()
    }
}

fn listen_for_tauri_completion<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    event_name: &'static str,
    correlation: Arc<CompletionCorrelationRegistry>,
    processed: Arc<ProcessedCompletionEvents>,
    consumers: CompletionConsumers,
) -> tauri::EventId {
    app_handle.listen_any(event_name, move |event| {
        let payload = match serde_json::from_str::<Value>(event.payload()) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(
                    event_name,
                    error = %error,
                    "Skipping Tauri completion delivery: payload could not be parsed"
                );
                return;
            }
        };
        let Some(event_id) = correlation.resolve_tauri(event_name, &payload) else {
            tracing::warn!(
                event_name,
                "Skipping Tauri completion delivery after correlation miss"
            );
            return;
        };
        dispatch_agent_workspace_completion(
            processed.as_ref(),
            CompletionDeliverySource::Tauri,
            EventEnvelope {
                event_id,
                name: event_name.to_string(),
                payload,
            },
            &consumers,
        );
    })
}

fn spawn_bus_completion_dispatch(
    bus: InternalEventBus,
    correlation: Arc<CompletionCorrelationRegistry>,
    processed: Arc<ProcessedCompletionEvents>,
    consumers: CompletionConsumers,
) -> tauri::async_runtime::JoinHandle<()> {
    let mut subscriber = bus.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match subscriber.recv().await {
                Ok(envelope) => {
                    if envelope.name != AGENT_RUN_COMPLETED && envelope.name != AGENT_TURN_COMPLETED
                    {
                        continue;
                    }
                    let correlated = correlation
                        .mark_source(envelope.event_id, CompletionCorrelationSource::Bus);
                    tracing::debug!(
                        event_name = envelope.name,
                        event_id = %envelope.event_id,
                        correlated,
                        "Observed completion event on internal bus"
                    );
                    dispatch_agent_workspace_completion(
                        processed.as_ref(),
                        CompletionDeliverySource::Bus,
                        envelope,
                        &consumers,
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        "Agent workspace completion bus receiver lagged; continuing"
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

pub(crate) fn install_completion_dispatch_with_consumers<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    bus: InternalEventBus,
    correlation: Arc<CompletionCorrelationRegistry>,
    consumers: CompletionConsumers,
) -> (Vec<tauri::EventId>, tauri::async_runtime::JoinHandle<()>) {
    let processed = Arc::new(ProcessedCompletionEvents::new());
    let listener_ids = vec![
        listen_for_tauri_completion(
            app_handle,
            AGENT_RUN_COMPLETED,
            Arc::clone(&correlation),
            Arc::clone(&processed),
            consumers.clone(),
        ),
        listen_for_tauri_completion(
            app_handle,
            AGENT_TURN_COMPLETED,
            Arc::clone(&correlation),
            Arc::clone(&processed),
            consumers.clone(),
        ),
    ];
    let bus_task = spawn_bus_completion_dispatch(bus, correlation, processed, consumers);
    (listener_ids, bus_task)
}

pub(crate) fn install_agent_workspace_completion_dispatch<R: Runtime>(
    app_handle: tauri::AppHandle<R>,
    bus: InternalEventBus,
    correlation: Arc<CompletionCorrelationRegistry>,
) {
    let consumers = completion_consumers_for_app_handle(app_handle.clone());
    let _ = install_completion_dispatch_with_consumers(&app_handle, bus, correlation, consumers);
}

pub(crate) fn dispatch_agent_workspace_completion(
    processed: &ProcessedCompletionEvents,
    source: CompletionDeliverySource,
    envelope: EventEnvelope,
    consumers: &CompletionConsumers,
) -> CompletionDispatchOutcome {
    let event_name = match envelope.name.as_str() {
        AGENT_RUN_COMPLETED => AGENT_RUN_COMPLETED,
        AGENT_TURN_COMPLETED => AGENT_TURN_COMPLETED,
        _ => return CompletionDispatchOutcome::Ignored,
    };
    let payload = match serde_json::from_value::<AgentCompletionPayload>(envelope.payload) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                event_name,
                event_id = %envelope.event_id,
                source = source.as_str(),
                error = %error,
                "Skipping agent workspace completion: payload could not be parsed"
            );
            return CompletionDispatchOutcome::Ignored;
        }
    };
    if payload.context_type != ChatContextType::Project {
        return CompletionDispatchOutcome::Ignored;
    }

    match processed.observe_and_claim_outcome(envelope.event_id, source) {
        CompletionClaimOutcome::Duplicate => {
            tracing::debug!(
                event_name,
                event_id = %envelope.event_id,
                source = source.as_str(),
                "Skipped duplicate agent workspace completion delivery"
            );
            return CompletionDispatchOutcome::Duplicate;
        }
        CompletionClaimOutcome::CapacityExhausted => {
            return CompletionDispatchOutcome::CapacityExhausted;
        }
        CompletionClaimOutcome::Claimed => {}
    }

    let event = CompletionDispatchEvent {
        event_id: envelope.event_id,
        event_name,
        conversation_id: ChatConversationId::from_string(payload.conversation_id.clone()),
        run_id: payload
            .run_id
            .as_deref()
            .and_then(|run_id| run_id.parse::<AgentRunId>().ok()),
        payload,
    };
    consumers.schedule(&event);
    CompletionDispatchOutcome::Scheduled
}
