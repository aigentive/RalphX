use async_trait::async_trait;
use uuid::Uuid;

use crate::application::linear_webhook_reconciliation_service::{
    ExternalIssueLink, LinearDelivery, LinearDeliveryRecord, LinearWebhookStore,
};
use crate::domain::entities::{ProjectId, SyncProvider, TaskId};
use crate::error::AppResult;
use crate::infrastructure::sqlite::DbConnection;

pub struct SqliteLinearWebhookStore {
    db: DbConnection,
}

impl SqliteLinearWebhookStore {
    pub fn new(db: DbConnection) -> Self {
        Self { db }
    }

    pub async fn get_config(&self) -> AppResult<(bool, Option<String>)> {
        self.db
            .run(|conn| {
                let value = conn.query_row(
                    "SELECT enabled, signing_secret_ref FROM linear_webhook_config WHERE id = 'default'",
                    [],
                    |row| Ok((row.get::<_, i32>(0)? != 0, row.get::<_, Option<String>>(1)?)),
                )?;
                Ok(value)
            })
            .await
    }

    pub async fn get_signing_secret_ref(&self) -> AppResult<Option<String>> {
        self.get_config().await.map(|(_, secret_ref)| secret_ref)
    }

    pub async fn set_signing_secret_ref(
        &self,
        signing_secret_ref: Option<String>,
        enabled: bool,
    ) -> AppResult<()> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO linear_webhook_config (id, enabled, signing_secret_ref, updated_at)
                     VALUES ('default', ?1, ?2, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
                     ON CONFLICT(id) DO UPDATE SET
                        enabled = excluded.enabled,
                        signing_secret_ref = excluded.signing_secret_ref,
                        updated_at = excluded.updated_at",
                    rusqlite::params![if enabled { 1i32 } else { 0i32 }, signing_secret_ref],
                )?;
                Ok(())
            })
            .await
    }
}

#[async_trait]
impl LinearWebhookStore for SqliteLinearWebhookStore {
    async fn record_delivery(&self, delivery: LinearDelivery) -> AppResult<LinearDeliveryRecord> {
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "INSERT OR IGNORE INTO linear_webhook_deliveries
                        (delivery_id, webhook_id, event_type, received_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        delivery.delivery_id,
                        delivery.webhook_id,
                        delivery.event_type,
                        delivery.received_at.to_rfc3339()
                    ],
                )?;
                if rows == 0 {
                    Ok(LinearDeliveryRecord::Duplicate)
                } else {
                    Ok(LinearDeliveryRecord::Recorded)
                }
            })
            .await
    }

    async fn get_issue_link(
        &self,
        external_issue_id: &str,
    ) -> AppResult<Option<ExternalIssueLink>> {
        let external_issue_id = external_issue_id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT project_id, task_id, external_key, external_url, last_external_status
                     FROM external_issue_links
                     WHERE provider = 'linear' AND external_id = ?1",
                    rusqlite::params![external_issue_id.clone()],
                    |row| {
                        let task_id: Option<String> = row.get(1)?;
                        Ok(ExternalIssueLink {
                            provider: SyncProvider::Linear,
                            project_id: ProjectId::from_string(row.get::<_, String>(0)?),
                            task_id: task_id.map(TaskId::from_string),
                            external_id: external_issue_id.clone(),
                            external_key: row.get(2)?,
                            external_url: row.get(3)?,
                            last_external_status: row.get(4)?,
                        })
                    },
                )
            })
            .await
    }

    async fn upsert_issue_link(&self, link: ExternalIssueLink) -> AppResult<()> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO external_issue_links
                        (provider, external_id, project_id, task_id, external_key, external_url, last_external_status, updated_at)
                     VALUES ('linear', ?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
                     ON CONFLICT(provider, external_id) DO UPDATE SET
                        project_id = excluded.project_id,
                        task_id = excluded.task_id,
                        external_key = excluded.external_key,
                        external_url = excluded.external_url,
                        last_external_status = excluded.last_external_status,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        link.external_id,
                        link.project_id.as_str(),
                        link.task_id.as_ref().map(|task_id| task_id.as_str().to_string()),
                        link.external_key,
                        link.external_url,
                        link.last_external_status,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn record_issue_activity(
        &self,
        delivery_id: &str,
        external_issue_id: &str,
        event_type: &str,
    ) -> AppResult<()> {
        let id = Uuid::new_v4().to_string();
        let delivery_id = delivery_id.to_string();
        let external_issue_id = external_issue_id.to_string();
        let event_type = event_type.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO external_issue_sync_events
                        (id, provider, external_id, delivery_id, event_type, created_at)
                     VALUES (?1, 'linear', ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
                    rusqlite::params![id, external_issue_id, delivery_id, event_type],
                )?;
                Ok(())
            })
            .await
    }
}
