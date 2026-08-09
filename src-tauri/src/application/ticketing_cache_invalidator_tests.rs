use super::ticketing_cache_invalidator::{
    TicketingCacheInvalidator, TICKETING_CACHE_INVALIDATED_EVENT,
};
use ralphx_events::RecordingEventSink;

#[test]
fn linear_issue_webhook_payload_builds_ticketing_cache_event() {
    let body = serde_json::json!({
        "type": "Issue",
        "data": {
            "id": "issue-1",
            "identifier": "LIN-1"
        }
    })
    .to_string();

    let event = TicketingCacheInvalidator::linear_webhook_event(body.as_bytes(), "recorded_issue")
        .expect("issue event should be parsed");

    assert_eq!(event.provider, "linear");
    assert_eq!(event.ticket_id.as_deref(), Some("issue-1"));
    assert_eq!(event.ticket_key.as_deref(), Some("LIN-1"));
    assert_eq!(event.reason, "recorded_issue");
}

#[test]
fn linear_comment_webhook_payload_uses_nested_issue_identity() {
    let body = serde_json::json!({
        "type": "Comment",
        "data": {
            "id": "comment-1",
            "issue": {
                "id": "issue-2",
                "identifier": "LIN-2"
            }
        }
    })
    .to_string();

    let event =
        TicketingCacheInvalidator::linear_webhook_event(body.as_bytes(), "recorded_issue_activity")
            .expect("comment event should be parsed");

    assert_eq!(event.ticket_id.as_deref(), Some("issue-2"));
    assert_eq!(event.ticket_key.as_deref(), Some("LIN-2"));
}

#[test]
fn unsupported_webhook_payload_does_not_invalidate_ticketing_cache() {
    let body = serde_json::json!({
        "type": "Project",
        "data": { "id": "project-1" }
    })
    .to_string();

    let event =
        TicketingCacheInvalidator::linear_webhook_event(body.as_bytes(), "unsupported_event");

    assert!(event.is_none());
}

#[test]
fn linear_attachment_webhook_payload_uses_nested_issue_identity() {
    let body = serde_json::json!({
        "type": "Attachment",
        "data": {
            "id": "attachment-1",
            "issue": {
                "id": "issue-3",
                "identifier": "LIN-3"
            }
        }
    })
    .to_string();

    let event = TicketingCacheInvalidator::linear_webhook_event(
        body.as_bytes(),
        "recorded_issue_attachment",
    )
    .expect("attachment event should be parsed");

    assert_eq!(event.provider, "linear");
    assert_eq!(event.ticket_id.as_deref(), Some("issue-3"));
    assert_eq!(event.ticket_key.as_deref(), Some("LIN-3"));
    assert_eq!(event.reason, "recorded_issue_attachment");
}

#[test]
fn linear_issue_attachment_alias_uses_nested_issue_identity() {
    let body = serde_json::json!({
        "type": "IssueAttachment",
        "data": {
            "id": "attachment-2",
            "issue": {
                "id": "issue-4",
                "identifier": "LIN-4"
            }
        }
    })
    .to_string();

    let event =
        TicketingCacheInvalidator::linear_webhook_event(body.as_bytes(), "attachment_alias")
            .expect("issue attachment alias should be parsed");

    assert_eq!(event.ticket_id.as_deref(), Some("issue-4"));
    assert_eq!(event.ticket_key.as_deref(), Some("LIN-4"));
}

#[test]
fn linear_comment_prefers_top_level_issue_id_over_nested_issue() {
    let body = serde_json::json!({
        "type": "IssueComment",
        "data": {
            "id": "comment-9",
            "issueId": "issue-direct",
            "issue": {
                "id": "issue-nested",
                "identifier": "LIN-9"
            }
        }
    })
    .to_string();

    let event =
        TicketingCacheInvalidator::linear_webhook_event(body.as_bytes(), "issue_comment_direct_id")
            .expect("issue comment event should be parsed");

    assert_eq!(event.ticket_id.as_deref(), Some("issue-direct"));
    assert_eq!(event.ticket_key.as_deref(), Some("LIN-9"));
}

#[test]
fn malformed_webhook_payload_does_not_invalidate_ticketing_cache() {
    let event = TicketingCacheInvalidator::linear_webhook_event(b"not valid json", "garbage");
    assert!(event.is_none());
}

#[test]
fn invalidate_linear_webhook_without_app_handle_returns_event_without_emitting() {
    let body = serde_json::json!({
        "type": "Issue",
        "data": {
            "id": "issue-5",
            "identifier": "LIN-5"
        }
    })
    .to_string();

    let event = TicketingCacheInvalidator::invalidate_linear_webhook(
        None,
        body.as_bytes(),
        "recorded_issue",
    )
    .expect("issue event should be returned even without an app handle");

    assert_eq!(event.provider, "linear");
    assert_eq!(event.ticket_id.as_deref(), Some("issue-5"));
    assert_eq!(event.ticket_key.as_deref(), Some("LIN-5"));
    assert_eq!(event.reason, "recorded_issue");
}

#[test]
fn invalidate_linear_webhook_with_unsupported_payload_returns_none() {
    let body = serde_json::json!({
        "type": "Cycle",
        "data": { "id": "cycle-1" }
    })
    .to_string();

    let event = TicketingCacheInvalidator::invalidate_linear_webhook(
        None,
        body.as_bytes(),
        "unsupported_event",
    );

    assert!(event.is_none());
}

#[test]
fn invalidate_linear_webhook_with_sink_emits_the_parsed_ticket_identity() {
    let body = serde_json::json!({
        "type": "Issue",
        "data": {
            "id": "issue-sink",
            "identifier": "LIN-SINK"
        }
    })
    .to_string();
    let events = RecordingEventSink::new();

    let event = TicketingCacheInvalidator::invalidate_linear_webhook_with_sink(
        &events,
        body.as_bytes(),
        "recorded_by_sink",
    )
    .expect("supported issue webhook should invalidate the cache");

    assert_eq!(event.ticket_id.as_deref(), Some("issue-sink"));
    assert_eq!(event.ticket_key.as_deref(), Some("LIN-SINK"));
    let recorded = events.events();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].event, TICKETING_CACHE_INVALIDATED_EVENT);
    assert_eq!(recorded[0].payload["ticketId"], "issue-sink");
    assert_eq!(recorded[0].payload["ticketKey"], "LIN-SINK");
    assert_eq!(recorded[0].payload["reason"], "recorded_by_sink");
}

#[test]
fn invalidate_linear_webhook_with_sink_rejects_unsupported_payload_without_emitting() {
    let body = serde_json::json!({
        "type": "Cycle",
        "data": { "id": "cycle-sink" }
    })
    .to_string();
    let events = RecordingEventSink::new();

    let event = TicketingCacheInvalidator::invalidate_linear_webhook_with_sink(
        &events,
        body.as_bytes(),
        "unsupported_sink_event",
    );

    assert!(event.is_none());
    assert!(
        events.events().is_empty(),
        "unsupported webhook must not emit cache invalidation"
    );
}
