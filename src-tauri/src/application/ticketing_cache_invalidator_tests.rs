use super::ticketing_cache_invalidator::TicketingCacheInvalidator;

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
