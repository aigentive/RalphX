use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::integrations::linear_webhook::{
    ExternalIssueLink, LinearDelivery, LinearDeliveryRecord, LinearWebhookStore,
};
use crate::domain::entities::{ProjectId, SyncProvider, TaskId};
use crate::error::{AppError, AppResult};
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

#[cfg(test)]
#[path = "sqlite_linear_webhook_store_tests.rs"]
mod tests;

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
                    "SELECT local_project_id, local_object_id, external_key, external_url, local_state
                     FROM external_issue_links
                     WHERE provider = 'linear'
                        AND external_kind = 'issue'
                        AND external_id = ?1
                        AND local_object_kind = 'task'
                     ORDER BY updated_at DESC
                     LIMIT 1",
                    rusqlite::params![external_issue_id.clone()],
                    |row| {
                        let project_id: Option<String> = row.get(0)?;
                        let task_id: String = row.get(1)?;
                        Ok(ExternalIssueLink {
                            provider: SyncProvider::Linear,
                            project_id: ProjectId::from_string(project_id.unwrap_or_default()),
                            task_id: Some(TaskId::from_string(task_id)),
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
        let Some(task_id) = link
            .task_id
            .as_ref()
            .map(|task_id| task_id.as_str().to_string())
        else {
            return Err(AppError::Database(
                "Linear issue links must be attached to a task before persistence".to_string(),
            ));
        };
        let idempotency_key = format!("linear:issue:{}:task:{task_id}", link.external_id);
        let id = Uuid::new_v4().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO external_issue_links
                        (id, provider, external_kind, external_id, external_key, external_url,
                         local_object_kind, local_object_id, local_project_id, local_state,
                         idempotency_key, metadata_json, created_at, updated_at)
                     VALUES
                        (?1, 'linear', 'issue', ?2, ?3, ?4,
                         'task', ?5, ?6, ?7,
                         ?8, NULL, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
                     ON CONFLICT(provider, external_kind, external_id, local_object_kind, local_object_id)
                     DO UPDATE SET
                        external_key = excluded.external_key,
                        external_url = excluded.external_url,
                        local_project_id = excluded.local_project_id,
                        local_state = excluded.local_state,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        id,
                        link.external_id,
                        link.external_key,
                        link.external_url,
                        task_id,
                        link.project_id.as_str(),
                        link.last_external_status,
                        idempotency_key,
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
