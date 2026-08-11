use chrono::Utc;
use ralphx_events::{emit_serialized, EventSink};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

pub const TICKETING_CACHE_INVALIDATED_EVENT: &str = "ticketing:cache_invalidated";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingCacheInvalidatedEvent {
    pub provider: String,
    pub ticket_id: Option<String>,
    pub ticket_key: Option<String>,
    pub container_id: Option<String>,
    pub project_id: Option<String>,
    pub reason: String,
    pub invalidated_at: String,
}

#[derive(Debug, Deserialize)]
struct LinearWebhookCachePayload {
    #[serde(rename = "type")]
    event_type: String,
    data: Value,
}

pub struct TicketingCacheInvalidator;

impl TicketingCacheInvalidator {
    pub fn invalidate_linear_webhook_with_sink(
        events: &dyn EventSink,
        raw_body: &[u8],
        reason: &str,
    ) -> Option<TicketingCacheInvalidatedEvent> {
        let event = Self::linear_webhook_event(raw_body, reason)?;
        if let Err(error) = emit_serialized(events, TICKETING_CACHE_INVALIDATED_EVENT, &event) {
            tracing::warn!(
                error = %error,
                ticket_id = ?event.ticket_id,
                ticket_key = ?event.ticket_key,
                "failed to emit ticketing cache invalidation event"
            );
        }
        Some(event)
    }

    pub fn invalidate_linear_webhook(
        app_handle: Option<&AppHandle>,
        raw_body: &[u8],
        reason: &str,
    ) -> Option<TicketingCacheInvalidatedEvent> {
        let event = Self::linear_webhook_event(raw_body, reason)?;
        if let Some(app_handle) = app_handle {
            if let Err(error) = app_handle.emit(TICKETING_CACHE_INVALIDATED_EVENT, &event) {
                tracing::warn!(
                    error = %error,
                    ticket_id = ?event.ticket_id,
                    ticket_key = ?event.ticket_key,
                    "failed to emit ticketing cache invalidation event"
                );
            }
        }
        Some(event)
    }

    pub fn linear_webhook_event(
        raw_body: &[u8],
        reason: &str,
    ) -> Option<TicketingCacheInvalidatedEvent> {
        let payload: LinearWebhookCachePayload = serde_json::from_slice(raw_body).ok()?;
        let (ticket_id, ticket_key) = linear_ticket_identity(&payload)?;
        Some(TicketingCacheInvalidatedEvent {
            provider: "linear".to_string(),
            ticket_id,
            ticket_key,
            container_id: None,
            project_id: None,
            reason: reason.to_string(),
            invalidated_at: Utc::now().to_rfc3339(),
        })
    }
}

fn linear_ticket_identity(
    payload: &LinearWebhookCachePayload,
) -> Option<(Option<String>, Option<String>)> {
    match payload.event_type.as_str() {
        "Issue" => Some((
            payload.data.get("id").and_then(string_value),
            payload.data.get("identifier").and_then(string_value),
        )),
        "Comment" | "IssueComment" | "Attachment" | "IssueAttachment" => {
            let issue = payload.data.get("issue");
            Some((
                payload
                    .data
                    .get("issueId")
                    .and_then(string_value)
                    .or_else(|| {
                        issue
                            .and_then(|value| value.get("id"))
                            .and_then(string_value)
                    }),
                issue
                    .and_then(|value| value.get("identifier"))
                    .and_then(string_value),
            ))
        }
        _ => None,
    }
}

fn string_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
