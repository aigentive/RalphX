//! Linear webhook delivery + issue-link records and the durable store port.
//!
//! The reconciliation service in `application` decides what a delivery means;
//! this module owns only the records it persists and the port the SQLite store
//! implements.
//!
//! Note: this `ExternalIssueLink` is the Linear-webhook link record and is
//! deliberately NOT re-exported from `domain::integrations`, which owns a
//! different, ticketing-wide `ExternalIssueLink`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{ProjectId, SyncProvider, TaskId};
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalIssueLink {
    pub provider: SyncProvider,
    pub project_id: ProjectId,
    pub task_id: Option<TaskId>,
    pub external_id: String,
    pub external_key: Option<String>,
    pub external_url: Option<String>,
    pub last_external_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearDelivery {
    pub delivery_id: String,
    pub webhook_id: Option<String>,
    pub event_type: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinearDeliveryRecord {
    Recorded,
    Duplicate,
}

#[async_trait]
pub trait LinearWebhookStore: Send + Sync {
    async fn record_delivery(&self, delivery: LinearDelivery) -> AppResult<LinearDeliveryRecord>;

    async fn get_issue_link(&self, external_issue_id: &str)
        -> AppResult<Option<ExternalIssueLink>>;

    async fn upsert_issue_link(&self, link: ExternalIssueLink) -> AppResult<()>;

    async fn record_issue_activity(
        &self,
        delivery_id: &str,
        external_issue_id: &str,
        event_type: &str,
    ) -> AppResult<()>;
}
