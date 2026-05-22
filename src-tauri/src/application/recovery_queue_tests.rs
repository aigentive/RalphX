use std::time::Duration;

use chrono::Utc;

use crate::application::recovery_queue::{
    ProcessSummary, RecoveryItem, RecoveryPriority, RecoveryQueue, RECOVERY_CUTOFF_HOURS,
};

fn make_item(context_id: &str, priority: RecoveryPriority) -> RecoveryItem {
    RecoveryItem {
        context_type: "ideation".to_string(),
        context_id: context_id.to_string(),
        conversation_id: format!("conv-{context_id}"),
        priority,
        started_at: Utc::now(),
    }
}

// (a) Priority ordering: Verification items dequeue before Ideation items.
#[test]
fn test_priority_ordering_verification_before_ideation() {
    let mut queue = RecoveryQueue::new(Duration::from_millis(1));

    queue.enqueue(make_item("ideation-1", RecoveryPriority::Ideation));
    queue.enqueue(make_item("ideation-2", RecoveryPriority::Ideation));
    queue.enqueue(make_item("verify-1", RecoveryPriority::Verification));
    queue.enqueue(make_item("verify-2", RecoveryPriority::Verification));

    assert_eq!(queue.dequeue().unwrap().context_id, "verify-1");
    assert_eq!(queue.dequeue().unwrap().context_id, "verify-2");
    assert_eq!(queue.dequeue().unwrap().context_id, "ideation-1");
    assert_eq!(queue.dequeue().unwrap().context_id, "ideation-2");
    assert!(queue.dequeue().is_none());
}

// Interleaved enqueue — Verification inserted before existing Ideation items.
#[test]
fn test_priority_ordering_interleaved_enqueue() {
    let mut queue = RecoveryQueue::new(Duration::from_millis(1));

    queue.enqueue(make_item("ideation-1", RecoveryPriority::Ideation));
    queue.enqueue(make_item("verify-1", RecoveryPriority::Verification));
    queue.enqueue(make_item("ideation-2", RecoveryPriority::Ideation));

    assert_eq!(queue.dequeue().unwrap().context_id, "verify-1");
    assert_eq!(queue.dequeue().unwrap().context_id, "ideation-1");
    assert_eq!(queue.dequeue().unwrap().context_id, "ideation-2");
}

// (b) Dedup: same context_id rejected on second enqueue.
#[test]
fn test_dedup_rejects_duplicate_context_id() {
    let mut queue = RecoveryQueue::new(Duration::from_millis(1));

    let accepted = queue.enqueue(make_item("session-1", RecoveryPriority::Ideation));
    assert!(accepted, "first enqueue should be accepted");

    let rejected = queue.enqueue(make_item("session-1", RecoveryPriority::Ideation));
    assert!(!rejected, "second enqueue with same context_id should be rejected");

    assert_eq!(queue.len(), 1, "queue should contain exactly one item");
}

// Dedup across different priorities — same context_id still rejected.
#[test]
fn test_dedup_across_priorities() {
    let mut queue = RecoveryQueue::new(Duration::from_millis(1));

    queue.enqueue(make_item("ctx-1", RecoveryPriority::Ideation));
    let rejected = queue.enqueue(make_item("ctx-1", RecoveryPriority::Verification));
    assert!(!rejected, "duplicate should be rejected even if priority differs");
    assert_eq!(queue.len(), 1);
}

// (c) 24-hour cutoff: items with started_at > 24h ago are skipped during process().
#[tokio::test]
async fn test_24_hour_cutoff_skips_stale_items() {
    let mut queue = RecoveryQueue::new(Duration::from_millis(1));

    // Stale item: started_at is 25 hours in the past.
    let stale = RecoveryItem {
        context_type: "ideation".to_string(),
        context_id: "stale-session".to_string(),
        conversation_id: "conv-stale".to_string(),
        priority: RecoveryPriority::Ideation,
        started_at: Utc::now() - chrono::Duration::hours(RECOVERY_CUTOFF_HOURS + 1),
    };

    // Fresh item: started_at is now.
    let fresh = make_item("fresh-session", RecoveryPriority::Ideation);

    queue.enqueue(stale);
    queue.enqueue(fresh);

    let mut recovered_ids: Vec<String> = Vec::new();
    let summary = queue
        .process(|item| {
            let id = item.context_id.clone();
            recovered_ids.push(id);
            async move { Ok::<(), String>(()) }
        })
        .await;

    assert_eq!(summary.skipped, 1, "stale item should be skipped");
    assert_eq!(summary.recovered, 1, "fresh item should be recovered");
    assert_eq!(summary.failed, 0);
    assert_eq!(recovered_ids, vec!["fresh-session"]);
}

// (d) Empty queue: process() completes immediately with zero counts.
#[tokio::test]
async fn test_empty_queue_completes_immediately() {
    let mut queue = RecoveryQueue::new(Duration::from_secs(60)); // large interval — must not delay

    let summary = queue
        .process(|_item| async move { Ok::<(), String>(()) })
        .await;

    assert_eq!(
        summary,
        ProcessSummary {
            recovered: 0,
            skipped: 0,
            failed: 0
        }
    );
}

// (e) Error isolation: callback failure on one item doesn't halt queue processing.
#[tokio::test]
async fn test_error_isolation_callback_failure_continues_queue() {
    let mut queue = RecoveryQueue::new(Duration::from_millis(1));

    queue.enqueue(make_item("session-fail", RecoveryPriority::Ideation));
    queue.enqueue(make_item("session-ok", RecoveryPriority::Ideation));

    let mut processed: Vec<String> = Vec::new();

    let summary = queue
        .process(|item| {
            let id = item.context_id.clone();
            processed.push(id.clone());
            async move {
                if id == "session-fail" {
                    Err("simulated recovery error".to_string())
                } else {
                    Ok(())
                }
            }
        })
        .await;

    assert_eq!(summary.failed, 1, "one item should have failed");
    assert_eq!(summary.recovered, 1, "second item should still be recovered");
    assert_eq!(summary.skipped, 0);
    assert!(
        processed.contains(&"session-ok".to_string()),
        "queue should continue past failed item and process session-ok"
    );
}
