use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use ralphx_events::catalog::{AGENT_RUN_COMPLETED, AGENT_TURN_COMPLETED};
use ralphx_events::EventEnvelope;
use serde_json::json;

use super::agent_workspace_completion_dispatch::{
    dispatch_agent_workspace_completion, CompletionConsumers, CompletionDeliverySource,
    CompletionDispatchEvent, CompletionDispatchOutcome, ProcessedCompletionEvents,
};

fn manual_clock() -> (
    Arc<AtomicU64>,
    Arc<dyn Fn() -> Duration + Send + Sync + 'static>,
) {
    let now_ms = Arc::new(AtomicU64::new(0));
    let clock_now = Arc::clone(&now_ms);
    let clock = Arc::new(move || Duration::from_millis(clock_now.load(Ordering::SeqCst)));
    (now_ms, clock)
}

fn completion_envelope(event_name: &str) -> EventEnvelope {
    EventEnvelope::new(
        event_name,
        json!({
            "conversation_id": "conversation-1",
            "context_type": "project",
            "run_id": "11111111-1111-1111-1111-111111111111"
        }),
    )
}

fn counting_consumers() -> (
    CompletionConsumers,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let review_count = Arc::new(AtomicUsize::new(0));
    let publish_count = Arc::new(AtomicUsize::new(0));
    let supervision_count = Arc::new(AtomicUsize::new(0));

    let review_counter = Arc::clone(&review_count);
    let publish_counter = Arc::clone(&publish_count);
    let supervision_counter = Arc::clone(&supervision_count);
    let consumers = CompletionConsumers::new(
        Arc::new(move |_event: &CompletionDispatchEvent| {
            review_counter.fetch_add(1, Ordering::SeqCst);
        }),
        Arc::new(move |_event: &CompletionDispatchEvent| {
            publish_counter.fetch_add(1, Ordering::SeqCst);
        }),
        Arc::new(move |_event: &CompletionDispatchEvent| {
            supervision_counter.fetch_add(1, Ordering::SeqCst);
        }),
    );

    (consumers, review_count, publish_count, supervision_count)
}

#[test]
fn first_delivery_claims_and_late_duplicate_stays_suppressed() {
    let (_now, clock) = manual_clock();
    let processed = ProcessedCompletionEvents::with_limits(Duration::from_secs(10), 4, clock);
    let envelope = completion_envelope(AGENT_RUN_COMPLETED);

    assert!(processed.observe_and_claim(envelope.event_id, CompletionDeliverySource::Tauri));
    assert!(!processed.observe_and_claim(envelope.event_id, CompletionDeliverySource::Bus));
    assert!(!processed.observe_and_claim(envelope.event_id, CompletionDeliverySource::Tauri));
}

#[test]
fn expired_entries_are_purged_before_capacity_is_reused() {
    let (now_ms, clock) = manual_clock();
    let processed = ProcessedCompletionEvents::with_limits(Duration::from_secs(1), 1, clock);
    let first = completion_envelope(AGENT_RUN_COMPLETED);
    let second = completion_envelope(AGENT_RUN_COMPLETED);

    assert!(processed.observe_and_claim(first.event_id, CompletionDeliverySource::Bus));
    assert!(!processed.observe_and_claim(second.event_id, CompletionDeliverySource::Bus));

    now_ms.store(1_001, Ordering::SeqCst);

    assert!(processed.observe_and_claim(second.event_id, CompletionDeliverySource::Bus));
}

#[test]
fn ttl_boundary_expires_claim_identity() {
    let (now_ms, clock) = manual_clock();
    let processed = ProcessedCompletionEvents::with_limits(Duration::from_secs(1), 1, clock);
    let envelope = completion_envelope(AGENT_RUN_COMPLETED);

    assert!(processed.observe_and_claim(envelope.event_id, CompletionDeliverySource::Bus));
    now_ms.store(1_000, Ordering::SeqCst);

    assert!(processed.observe_and_claim(envelope.event_id, CompletionDeliverySource::Tauri));
}

#[test]
fn full_registry_fails_closed_without_evicting_live_claim() {
    let (_now, clock) = manual_clock();
    let processed = ProcessedCompletionEvents::with_limits(Duration::from_secs(60), 1, clock);
    let first = completion_envelope(AGENT_RUN_COMPLETED);
    let second = completion_envelope(AGENT_RUN_COMPLETED);

    assert!(processed.observe_and_claim(first.event_id, CompletionDeliverySource::Bus));
    assert!(!processed.observe_and_claim(second.event_id, CompletionDeliverySource::Tauri));
    assert!(!processed.observe_and_claim(first.event_id, CompletionDeliverySource::Tauri));
}

#[test]
fn concurrent_sources_have_exactly_one_claim_winner() {
    let (_now, clock) = manual_clock();
    let processed = Arc::new(ProcessedCompletionEvents::with_limits(
        Duration::from_secs(60),
        8,
        clock,
    ));
    let event_id = completion_envelope(AGENT_RUN_COMPLETED).event_id;
    let barrier = Arc::new(Barrier::new(3));

    let handles = [
        CompletionDeliverySource::Tauri,
        CompletionDeliverySource::Bus,
    ]
    .into_iter()
    .map(|source| {
        let processed = Arc::clone(&processed);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            processed.observe_and_claim(event_id, source)
        })
    })
    .collect::<Vec<_>>();
    barrier.wait();

    let winners = handles
        .into_iter()
        .map(|handle| handle.join().expect("claim thread"))
        .filter(|won| *won)
        .count();
    assert_eq!(winners, 1);
}

#[test]
fn run_completion_schedules_all_consumers_once() {
    let (_now, clock) = manual_clock();
    let processed = ProcessedCompletionEvents::with_limits(Duration::from_secs(60), 8, clock);
    let (consumers, review, publish, supervision) = counting_consumers();
    let envelope = completion_envelope(AGENT_RUN_COMPLETED);

    assert_eq!(
        dispatch_agent_workspace_completion(
            &processed,
            CompletionDeliverySource::Bus,
            envelope.clone(),
            &consumers,
        ),
        CompletionDispatchOutcome::Scheduled
    );
    assert_eq!(
        dispatch_agent_workspace_completion(
            &processed,
            CompletionDeliverySource::Tauri,
            envelope,
            &consumers,
        ),
        CompletionDispatchOutcome::Duplicate
    );
    assert_eq!(review.load(Ordering::SeqCst), 1);
    assert_eq!(publish.load(Ordering::SeqCst), 1);
    assert_eq!(supervision.load(Ordering::SeqCst), 1);
}

#[test]
fn turn_completion_omits_supervision_recovery() {
    let (_now, clock) = manual_clock();
    let processed = ProcessedCompletionEvents::with_limits(Duration::from_secs(60), 8, clock);
    let (consumers, review, publish, supervision) = counting_consumers();

    assert_eq!(
        dispatch_agent_workspace_completion(
            &processed,
            CompletionDeliverySource::Tauri,
            completion_envelope(AGENT_TURN_COMPLETED),
            &consumers,
        ),
        CompletionDispatchOutcome::Scheduled
    );
    assert_eq!(review.load(Ordering::SeqCst), 1);
    assert_eq!(publish.load(Ordering::SeqCst), 1);
    assert_eq!(supervision.load(Ordering::SeqCst), 0);
}

#[test]
fn run_without_valid_run_id_still_schedules_review_and_publish() {
    let (_now, clock) = manual_clock();
    let processed = ProcessedCompletionEvents::with_limits(Duration::from_secs(60), 8, clock);
    let (consumers, review, publish, supervision) = counting_consumers();
    let envelope = EventEnvelope::new(
        AGENT_RUN_COMPLETED,
        json!({
            "conversation_id": "conversation-1",
            "context_type": "project",
            "run_id": "not-a-run-id"
        }),
    );

    assert_eq!(
        dispatch_agent_workspace_completion(
            &processed,
            CompletionDeliverySource::Bus,
            envelope,
            &consumers,
        ),
        CompletionDispatchOutcome::Scheduled
    );
    assert_eq!(review.load(Ordering::SeqCst), 1);
    assert_eq!(publish.load(Ordering::SeqCst), 1);
    assert_eq!(supervision.load(Ordering::SeqCst), 0);
}

#[test]
fn malformed_non_project_and_unrelated_events_schedule_nothing() {
    let (_now, clock) = manual_clock();
    let processed = ProcessedCompletionEvents::with_limits(Duration::from_secs(60), 8, clock);
    let (consumers, review, publish, supervision) = counting_consumers();
    let malformed = EventEnvelope::new(AGENT_RUN_COMPLETED, json!({"context_type": "project"}));
    let non_project = EventEnvelope::new(
        AGENT_RUN_COMPLETED,
        json!({"conversation_id": "conversation-1", "context_type": "task"}),
    );
    let unrelated = EventEnvelope::new(
        "agent:run_started",
        json!({"conversation_id": "conversation-1", "context_type": "project"}),
    );

    for envelope in [malformed, non_project, unrelated] {
        assert_eq!(
            dispatch_agent_workspace_completion(
                &processed,
                CompletionDeliverySource::Bus,
                envelope,
                &consumers,
            ),
            CompletionDispatchOutcome::Ignored
        );
    }
    assert_eq!(review.load(Ordering::SeqCst), 0);
    assert_eq!(publish.load(Ordering::SeqCst), 0);
    assert_eq!(supervision.load(Ordering::SeqCst), 0);
}
