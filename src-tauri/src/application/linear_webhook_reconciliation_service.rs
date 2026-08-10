use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use tokio::sync::Mutex;

use crate::domain::entities::{InternalStatus, SyncProvider, TaskId, WorkflowSchema};
use crate::domain::repositories::WorkflowRepository;
use crate::error::{AppError, AppResult};

// Delivery/link records and the store port are domain contracts; re-exported
// here so existing `application::linear_webhook_reconciliation_service`
// importers keep resolving.
pub use crate::domain::integrations::linear_webhook::{
    ExternalIssueLink, LinearDelivery, LinearDeliveryRecord, LinearWebhookStore,
};

type HmacSha256 = Hmac<Sha256>;

const LINEAR_WEBHOOK_FRESHNESS_WINDOW: Duration = Duration::seconds(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearWebhookHeaders {
    pub signature: Option<String>,
    pub delivery: Option<String>,
    pub event: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearWebhookRequest {
    pub headers: LinearWebhookHeaders,
    pub raw_body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinearWebhookAction {
    TransitionedTask {
        task_id: TaskId,
        target_status: InternalStatus,
    },
    RecordedIssue,
    RecordedIssueActivity,
    NoLinkedTask,
    NoMappedStatus,
    UnsupportedEvent,
    DuplicateDelivery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearWebhookOutcome {
    pub delivery_id: String,
    pub duplicate: bool,
    pub action: LinearWebhookAction,
}

#[derive(Debug, thiserror::Error)]
pub enum LinearWebhookError {
    #[error("missing Linear-Signature header")]
    MissingSignature,
    #[error("invalid Linear-Signature header")]
    InvalidSignature,
    #[error("malformed Linear webhook body: {0}")]
    MalformedBody(String),
    #[error("stale Linear webhook timestamp")]
    StaleTimestamp,
    #[error("missing Linear delivery id")]
    MissingDeliveryId,
    #[error("Linear webhook secret is not configured")]
    MissingSecret,
    #[error("Linear webhook reconciliation failed: {0}")]
    Reconciliation(String),
}

impl LinearWebhookError {
    pub fn is_missing_signature(&self) -> bool {
        matches!(self, Self::MissingSignature)
    }

    pub fn is_invalid_signature(&self) -> bool {
        matches!(self, Self::InvalidSignature)
    }

    pub fn is_malformed_body(&self) -> bool {
        matches!(self, Self::MalformedBody(_))
    }

    pub fn is_stale_timestamp(&self) -> bool {
        matches!(self, Self::StaleTimestamp)
    }
}

pub struct LinearWebhookReconciliationService {
    signing_secret: String,
    store: Arc<dyn LinearWebhookStore>,
    workflow_repo: Arc<dyn WorkflowRepository>,
}

impl LinearWebhookReconciliationService {
    pub fn new(
        signing_secret: String,
        store: Arc<dyn LinearWebhookStore>,
        workflow_repo: Arc<dyn WorkflowRepository>,
    ) -> Self {
        Self {
            signing_secret,
            store,
            workflow_repo,
        }
    }

    pub async fn handle(
        &self,
        request: LinearWebhookRequest,
        now: DateTime<Utc>,
    ) -> Result<LinearWebhookOutcome, LinearWebhookError> {
        if self.signing_secret.trim().is_empty() {
            return Err(LinearWebhookError::MissingSecret);
        }
        verify_signature(
            request.headers.signature.as_deref(),
            &request.raw_body,
            &self.signing_secret,
        )?;

        let payload: LinearWebhookPayload = serde_json::from_slice(&request.raw_body)
            .map_err(|error| LinearWebhookError::MalformedBody(error.to_string()))?;

        ensure_fresh_timestamp(payload.webhook_timestamp, now)?;

        let delivery_id = request
            .headers
            .delivery
            .clone()
            .or_else(|| payload.webhook_id.clone())
            .ok_or(LinearWebhookError::MissingDeliveryId)?;
        let event_type = request
            .headers
            .event
            .clone()
            .unwrap_or_else(|| payload.event_type.clone());

        let delivery = LinearDelivery {
            delivery_id: delivery_id.clone(),
            webhook_id: payload.webhook_id.clone(),
            event_type: event_type.clone(),
            received_at: now,
        };

        match self
            .store
            .record_delivery(delivery)
            .await
            .map_err(reconciliation_error)?
        {
            LinearDeliveryRecord::Duplicate => {
                return Ok(LinearWebhookOutcome {
                    delivery_id,
                    duplicate: true,
                    action: LinearWebhookAction::DuplicateDelivery,
                });
            }
            LinearDeliveryRecord::Recorded => {}
        }

        let action = match payload.event_type.as_str() {
            "Issue" => self.reconcile_issue(&delivery_id, &payload).await?,
            "Comment" | "IssueComment" | "Attachment" | "IssueAttachment" => {
                self.reconcile_issue_activity(&delivery_id, &payload, &event_type)
                    .await?
            }
            _ => LinearWebhookAction::UnsupportedEvent,
        };

        Ok(LinearWebhookOutcome {
            delivery_id,
            duplicate: false,
            action,
        })
    }

    async fn reconcile_issue(
        &self,
        delivery_id: &str,
        payload: &LinearWebhookPayload,
    ) -> Result<LinearWebhookAction, LinearWebhookError> {
        let issue = LinearIssueData::from_payload(payload)?;
        let existing_link = self
            .store
            .get_issue_link(&issue.id)
            .await
            .map_err(reconciliation_error)?;

        let Some(mut link) = existing_link else {
            self.store
                .record_issue_activity(delivery_id, &issue.id, &payload.event_type)
                .await
                .map_err(reconciliation_error)?;
            return Ok(LinearWebhookAction::RecordedIssue);
        };

        link.external_key = issue.identifier.or(link.external_key);
        link.external_url = issue.url.or(link.external_url);
        link.last_external_status = issue.state_name.clone();
        self.store
            .upsert_issue_link(link.clone())
            .await
            .map_err(reconciliation_error)?;
        self.store
            .record_issue_activity(delivery_id, &issue.id, &payload.event_type)
            .await
            .map_err(reconciliation_error)?;

        let Some(status_name) = issue.state_name.as_deref() else {
            return Ok(LinearWebhookAction::NoMappedStatus);
        };
        let Some(target_status) = self.mapped_status_for(status_name).await? else {
            return Ok(LinearWebhookAction::NoMappedStatus);
        };
        let Some(task_id) = link.task_id else {
            return Ok(LinearWebhookAction::NoLinkedTask);
        };

        Ok(LinearWebhookAction::TransitionedTask {
            task_id,
            target_status,
        })
    }

    async fn reconcile_issue_activity(
        &self,
        delivery_id: &str,
        payload: &LinearWebhookPayload,
        event_type: &str,
    ) -> Result<LinearWebhookAction, LinearWebhookError> {
        let issue_id = payload
            .data
            .get("issueId")
            .and_then(|value| value.as_str())
            .or_else(|| {
                payload
                    .data
                    .get("issue")
                    .and_then(|issue| issue.get("id"))
                    .and_then(|value| value.as_str())
            });
        if let Some(issue_id) = issue_id {
            self.store
                .record_issue_activity(delivery_id, issue_id, event_type)
                .await
                .map_err(reconciliation_error)?;
            return Ok(LinearWebhookAction::RecordedIssueActivity);
        }

        Ok(LinearWebhookAction::UnsupportedEvent)
    }

    async fn mapped_status_for(
        &self,
        external_status: &str,
    ) -> Result<Option<InternalStatus>, LinearWebhookError> {
        let workflow = self
            .workflow_repo
            .get_default()
            .await
            .map_err(reconciliation_error)?
            .unwrap_or_else(WorkflowSchema::default_ralphx);
        let Some(external_sync) = workflow.external_sync else {
            return Ok(None);
        };
        if external_sync.provider != SyncProvider::Linear {
            return Ok(None);
        }
        Ok(external_sync
            .mapping
            .get(external_status)
            .map(|mapping| mapping.internal_status))
    }
}

fn reconciliation_error(error: AppError) -> LinearWebhookError {
    LinearWebhookError::Reconciliation(error.to_string())
}

fn verify_signature(
    signature: Option<&str>,
    raw_body: &[u8],
    signing_secret: &str,
) -> Result<(), LinearWebhookError> {
    let signature = signature.ok_or(LinearWebhookError::MissingSignature)?;
    let decoded = decode_hex_signature(signature).ok_or(LinearWebhookError::InvalidSignature)?;
    let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes())
        .map_err(|_| LinearWebhookError::InvalidSignature)?;
    mac.update(raw_body);
    mac.verify_slice(&decoded)
        .map_err(|_| LinearWebhookError::InvalidSignature)
}

fn decode_hex_signature(signature: &str) -> Option<Vec<u8>> {
    if signature.len() != 64 {
        return None;
    }
    let mut bytes = Vec::with_capacity(signature.len() / 2);
    for pair in signature.as_bytes().chunks_exact(2) {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn ensure_fresh_timestamp(timestamp_ms: i64, now: DateTime<Utc>) -> Result<(), LinearWebhookError> {
    let delta = now
        .timestamp_millis()
        .checked_sub(timestamp_ms)
        .map(|diff| {
            if diff < 0 {
                diff.checked_neg().unwrap_or(i64::MAX)
            } else {
                diff
            }
        })
        .unwrap_or(i64::MAX);
    if delta > LINEAR_WEBHOOK_FRESHNESS_WINDOW.num_milliseconds() {
        return Err(LinearWebhookError::StaleTimestamp);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct LinearWebhookPayload {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(rename = "webhookTimestamp")]
    webhook_timestamp: i64,
    #[serde(rename = "webhookId")]
    webhook_id: Option<String>,
    data: serde_json::Value,
}

struct LinearIssueData {
    id: String,
    identifier: Option<String>,
    url: Option<String>,
    state_name: Option<String>,
}

impl LinearIssueData {
    fn from_payload(payload: &LinearWebhookPayload) -> Result<Self, LinearWebhookError> {
        let id = payload
            .data
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LinearWebhookError::MalformedBody("missing issue id".to_string()))?
            .to_string();
        let identifier = payload
            .data
            .get("identifier")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let url = payload
            .data
            .get("url")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let state_name = payload
            .data
            .get("state")
            .and_then(|state| state.get("name"))
            .and_then(|value| value.as_str())
            .map(str::to_string);

        Ok(Self {
            id,
            identifier,
            url,
            state_name,
        })
    }
}

#[derive(Default)]
struct MemoryLinearWebhookStoreState {
    deliveries: HashMap<String, LinearDelivery>,
    issue_links: HashMap<String, ExternalIssueLink>,
    activities: Vec<(String, String, String)>,
}

#[derive(Default)]
pub struct MemoryLinearWebhookStore {
    state: Mutex<MemoryLinearWebhookStoreState>,
}

impl MemoryLinearWebhookStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn delivery_count(&self) -> usize {
        self.state.lock().await.deliveries.len()
    }

    pub async fn activity_count(&self) -> usize {
        self.state.lock().await.activities.len()
    }
}

#[async_trait]
impl LinearWebhookStore for MemoryLinearWebhookStore {
    async fn record_delivery(&self, delivery: LinearDelivery) -> AppResult<LinearDeliveryRecord> {
        let mut state = self.state.lock().await;
        if state.deliveries.contains_key(&delivery.delivery_id) {
            return Ok(LinearDeliveryRecord::Duplicate);
        }
        state
            .deliveries
            .insert(delivery.delivery_id.clone(), delivery);
        Ok(LinearDeliveryRecord::Recorded)
    }

    async fn get_issue_link(
        &self,
        external_issue_id: &str,
    ) -> AppResult<Option<ExternalIssueLink>> {
        Ok(self
            .state
            .lock()
            .await
            .issue_links
            .get(external_issue_id)
            .cloned())
    }

    async fn upsert_issue_link(&self, link: ExternalIssueLink) -> AppResult<()> {
        self.state
            .lock()
            .await
            .issue_links
            .insert(link.external_id.clone(), link);
        Ok(())
    }

    async fn record_issue_activity(
        &self,
        delivery_id: &str,
        external_issue_id: &str,
        event_type: &str,
    ) -> AppResult<()> {
        self.state.lock().await.activities.push((
            delivery_id.to_string(),
            external_issue_id.to_string(),
            event_type.to_string(),
        ));
        Ok(())
    }
}

#[cfg(test)]
#[path = "linear_webhook_reconciliation_service_tests.rs"]
mod tests;
