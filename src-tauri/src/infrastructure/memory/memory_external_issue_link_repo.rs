use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::domain::integrations::{
    ExternalIssueLink, ExternalIssueLinkRepository, ExternalIssueLinkUpsert,
    ExternalIssueLocalObject, ExternalIssueSyncRecord, ExternalIssueSyncRecordUpsert,
    ExternalIssueSyncStatus,
};
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryExternalIssueLinkRepository {
    links: Arc<RwLock<Vec<ExternalIssueLink>>>,
    sync_records: Arc<RwLock<Vec<ExternalIssueSyncRecord>>>,
}

impl MemoryExternalIssueLinkRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

fn matches_link_identity(link: &ExternalIssueLink, input: &ExternalIssueLinkUpsert) -> bool {
    link.idempotency_key == input.idempotency_key
        || (link.provider == input.provider
            && link.external_kind == input.external_kind
            && link.external_id == input.external_id
            && link.local_object == input.local_object)
}

fn apply_link_update(link: &mut ExternalIssueLink, input: ExternalIssueLinkUpsert) {
    link.provider = input.provider;
    link.external_kind = input.external_kind;
    link.external_id = input.external_id;
    link.external_key = input.external_key;
    link.external_url = input.external_url;
    link.local_object = input.local_object;
    link.local_project_id = input.local_project_id;
    link.local_sha = input.local_sha;
    link.local_state = input.local_state;
    link.idempotency_key = input.idempotency_key;
    link.metadata_json = input.metadata_json;
    link.updated_at = Utc::now();
}

fn new_link(input: ExternalIssueLinkUpsert) -> ExternalIssueLink {
    let now = Utc::now();
    ExternalIssueLink {
        id: Uuid::new_v4().to_string(),
        provider: input.provider,
        external_kind: input.external_kind,
        external_id: input.external_id,
        external_key: input.external_key,
        external_url: input.external_url,
        local_object: input.local_object,
        local_project_id: input.local_project_id,
        local_sha: input.local_sha,
        local_state: input.local_state,
        idempotency_key: input.idempotency_key,
        metadata_json: input.metadata_json,
        created_at: now,
        updated_at: now,
    }
}

fn apply_sync_update(record: &mut ExternalIssueSyncRecord, input: ExternalIssueSyncRecordUpsert) {
    record.link_id = input.link_id;
    record.sync_kind = input.sync_kind;
    record.idempotency_key = input.idempotency_key;
    record.local_sha = input.local_sha;
    record.local_state = input.local_state;
    record.external_version = input.external_version;
    record.status = input.status;
    record.last_attempt_at = Some(Utc::now());
    record.completed_at = terminal_completed_at(record.status);
    record.error_message = input.error_message;
    record.metadata_json = input.metadata_json;
    record.updated_at = Utc::now();
}

fn new_sync_record(input: ExternalIssueSyncRecordUpsert) -> ExternalIssueSyncRecord {
    let now = Utc::now();
    ExternalIssueSyncRecord {
        id: Uuid::new_v4().to_string(),
        link_id: input.link_id,
        sync_kind: input.sync_kind,
        idempotency_key: input.idempotency_key,
        local_sha: input.local_sha,
        local_state: input.local_state,
        external_version: input.external_version,
        status: input.status,
        last_attempt_at: Some(now),
        completed_at: terminal_completed_at(input.status),
        error_message: input.error_message,
        metadata_json: input.metadata_json,
        created_at: now,
        updated_at: now,
    }
}

fn terminal_completed_at(status: ExternalIssueSyncStatus) -> Option<chrono::DateTime<Utc>> {
    match status {
        ExternalIssueSyncStatus::Succeeded
        | ExternalIssueSyncStatus::Failed
        | ExternalIssueSyncStatus::Skipped => Some(Utc::now()),
        ExternalIssueSyncStatus::Pending => None,
    }
}

#[async_trait]
impl ExternalIssueLinkRepository for MemoryExternalIssueLinkRepository {
    async fn upsert_link(&self, input: ExternalIssueLinkUpsert) -> AppResult<ExternalIssueLink> {
        let mut links = self.links.write().await;
        if let Some(link) = links
            .iter_mut()
            .find(|link| matches_link_identity(link, &input))
        {
            apply_link_update(link, input);
            return Ok(link.clone());
        }

        let link = new_link(input);
        links.push(link.clone());
        Ok(link)
    }

    async fn get_link(&self, id: &str) -> AppResult<Option<ExternalIssueLink>> {
        Ok(self
            .links
            .read()
            .await
            .iter()
            .find(|link| link.id == id)
            .cloned())
    }

    async fn find_link_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<ExternalIssueLink>> {
        Ok(self
            .links
            .read()
            .await
            .iter()
            .find(|link| link.idempotency_key == idempotency_key)
            .cloned())
    }

    async fn find_link_by_external_identity(
        &self,
        provider: &str,
        external_kind: &str,
        external_id: &str,
    ) -> AppResult<Option<ExternalIssueLink>> {
        Ok(self
            .links
            .read()
            .await
            .iter()
            .find(|link| {
                link.provider == provider
                    && link.external_kind == external_kind
                    && link.external_id == external_id
            })
            .cloned())
    }

    async fn list_links_for_local(
        &self,
        local_object: &ExternalIssueLocalObject,
    ) -> AppResult<Vec<ExternalIssueLink>> {
        Ok(self
            .links
            .read()
            .await
            .iter()
            .filter(|link| &link.local_object == local_object)
            .cloned()
            .collect())
    }

    async fn upsert_sync_record(
        &self,
        input: ExternalIssueSyncRecordUpsert,
    ) -> AppResult<ExternalIssueSyncRecord> {
        let mut records = self.sync_records.write().await;
        if let Some(record) = records
            .iter_mut()
            .find(|record| record.idempotency_key == input.idempotency_key)
        {
            apply_sync_update(record, input);
            return Ok(record.clone());
        }

        let record = new_sync_record(input);
        records.push(record.clone());
        Ok(record)
    }

    async fn find_sync_record_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<ExternalIssueSyncRecord>> {
        Ok(self
            .sync_records
            .read()
            .await
            .iter()
            .find(|record| record.idempotency_key == idempotency_key)
            .cloned())
    }

    async fn list_sync_records_for_link(
        &self,
        link_id: &str,
    ) -> AppResult<Vec<ExternalIssueSyncRecord>> {
        Ok(self
            .sync_records
            .read()
            .await
            .iter()
            .filter(|record| record.link_id == link_id)
            .cloned()
            .collect())
    }

    async fn update_sync_status(
        &self,
        sync_record_id: &str,
        status: ExternalIssueSyncStatus,
        external_version: Option<&str>,
        error_message: Option<&str>,
    ) -> AppResult<Option<ExternalIssueSyncRecord>> {
        let mut records = self.sync_records.write().await;
        let Some(record) = records
            .iter_mut()
            .find(|record| record.id == sync_record_id)
        else {
            return Ok(None);
        };
        record.status = status;
        record.external_version = external_version.map(str::to_string);
        record.error_message = error_message.map(str::to_string);
        record.completed_at = terminal_completed_at(status);
        record.updated_at = Utc::now();
        Ok(Some(record.clone()))
    }
}
