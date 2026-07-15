use std::sync::Arc;

use serde_json::Value;

use crate::domain::integrations::{
    ExternalIssueLink, ExternalIssueLinkRepository, ExternalIssueLinkUpsert,
    ExternalIssueLocalObject, ExternalIssueSyncRecord, ExternalIssueSyncRecordUpsert,
    ProviderTicketOperation, ProviderTicketOperationStatus, ProviderTicketOperationUpsert,
};
use crate::domain::services::{
    primary_jira_key_from_composer_metadata, primary_jira_key_from_title,
    primary_linear_issue_from_composer_metadata,
};
use crate::error::AppResult;

pub struct ExternalIssueLinkService {
    repo: Arc<dyn ExternalIssueLinkRepository>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketConversationLinkInput {
    pub provider: String,
    pub external_kind: String,
    pub external_id: String,
    pub external_key: Option<String>,
    pub external_url: Option<String>,
    pub conversation_id: String,
    pub project_id: String,
    pub local_sha: Option<String>,
    pub local_state: Option<String>,
    pub metadata_json: Option<String>,
}

impl ExternalIssueLinkService {
    pub fn new(repo: Arc<dyn ExternalIssueLinkRepository>) -> Self {
        Self { repo }
    }

    pub async fn upsert_link(
        &self,
        input: ExternalIssueLinkUpsert,
    ) -> AppResult<ExternalIssueLink> {
        self.repo.upsert_link(input).await
    }

    pub async fn list_links_for_local(
        &self,
        local_object: &ExternalIssueLocalObject,
    ) -> AppResult<Vec<ExternalIssueLink>> {
        self.repo.list_links_for_local(local_object).await
    }

    pub async fn get_link(&self, id: &str) -> AppResult<Option<ExternalIssueLink>> {
        self.repo.get_link(id).await
    }

    pub async fn find_link_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<ExternalIssueLink>> {
        self.repo
            .find_link_by_idempotency_key(idempotency_key)
            .await
    }

    pub async fn find_link_by_external_identity(
        &self,
        provider: &str,
        external_kind: &str,
        external_id: &str,
    ) -> AppResult<Option<ExternalIssueLink>> {
        self.repo
            .find_link_by_external_identity(provider, external_kind, external_id)
            .await
    }

    pub async fn upsert_ticket_conversation_link(
        &self,
        input: TicketConversationLinkInput,
    ) -> AppResult<ExternalIssueLink> {
        let provider = input.provider.trim().to_ascii_lowercase();
        let external_kind = input.external_kind.trim().to_ascii_lowercase();
        let external_id = input.external_id.trim().to_string();
        let conversation_id = input.conversation_id.trim().to_string();
        self.repo
            .upsert_link(ExternalIssueLinkUpsert {
                provider: provider.clone(),
                external_kind: external_kind.clone(),
                external_id: external_id.clone(),
                external_key: input.external_key,
                external_url: input.external_url,
                local_object: ExternalIssueLocalObject::session(conversation_id.clone()),
                local_project_id: Some(input.project_id),
                local_sha: input.local_sha,
                local_state: input.local_state,
                idempotency_key: format!(
                    "{provider}:{external_kind}:{external_id}:session:{conversation_id}"
                ),
                metadata_json: input.metadata_json,
            })
            .await
    }

    pub async fn list_ticket_links_for_conversation(
        &self,
        conversation_id: &str,
    ) -> AppResult<Vec<ExternalIssueLink>> {
        self.repo
            .list_links_for_local(&ExternalIssueLocalObject::session(conversation_id))
            .await
    }

    pub async fn upsert_sync_record(
        &self,
        input: ExternalIssueSyncRecordUpsert,
    ) -> AppResult<ExternalIssueSyncRecord> {
        self.repo.upsert_sync_record(input).await
    }

    pub async fn find_sync_record_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<ExternalIssueSyncRecord>> {
        self.repo
            .find_sync_record_by_idempotency_key(idempotency_key)
            .await
    }

    pub async fn list_sync_records_for_link(
        &self,
        link_id: &str,
    ) -> AppResult<Vec<ExternalIssueSyncRecord>> {
        self.repo.list_sync_records_for_link(link_id).await
    }

    pub async fn update_sync_status(
        &self,
        sync_record_id: &str,
        status: crate::domain::integrations::ExternalIssueSyncStatus,
        external_version: Option<&str>,
        error_message: Option<&str>,
    ) -> AppResult<Option<ExternalIssueSyncRecord>> {
        self.repo
            .update_sync_status(sync_record_id, status, external_version, error_message)
            .await
    }

    pub async fn upsert_provider_ticket_operation(
        &self,
        input: ProviderTicketOperationUpsert,
    ) -> AppResult<ProviderTicketOperation> {
        self.repo.upsert_provider_ticket_operation(input).await
    }

    pub async fn find_provider_ticket_operation_by_client_operation_id(
        &self,
        client_operation_id: &str,
    ) -> AppResult<Option<ProviderTicketOperation>> {
        self.repo
            .find_provider_ticket_operation_by_client_operation_id(client_operation_id)
            .await
    }

    pub async fn list_provider_ticket_operations_for_ticket(
        &self,
        provider: &str,
        external_kind: &str,
        external_id: &str,
        external_key: Option<&str>,
        local_project_id: Option<&str>,
    ) -> AppResult<Vec<ProviderTicketOperation>> {
        self.repo
            .list_provider_ticket_operations_for_ticket(
                provider,
                external_kind,
                external_id,
                external_key,
                local_project_id,
            )
            .await
    }

    pub async fn update_provider_ticket_operation_status(
        &self,
        operation_id: &str,
        status: ProviderTicketOperationStatus,
        provider_operation_id: Option<&str>,
        error_message: Option<&str>,
    ) -> AppResult<Option<ProviderTicketOperation>> {
        self.repo
            .update_provider_ticket_operation_status(
                operation_id,
                status,
                provider_operation_id,
                error_message,
            )
            .await
    }

    pub async fn ensure_jira_task_link_from_metadata_or_title(
        &self,
        task_id: &str,
        project_id: Option<&str>,
        composer_metadata: Option<&str>,
        title: &str,
        local_sha: Option<&str>,
        local_state: Option<&str>,
    ) -> AppResult<Option<ExternalIssueLink>> {
        let metadata_key = primary_jira_key_from_composer_metadata(composer_metadata);
        let key = metadata_key
            .clone()
            .or_else(|| primary_jira_key_from_title(title));
        let Some(key) = key else {
            return Ok(None);
        };

        let metadata_url = composer_metadata.and_then(first_jira_reference_url);
        self.repo
            .upsert_link(ExternalIssueLinkUpsert {
                provider: "atlassian".to_string(),
                external_kind: "jira".to_string(),
                external_id: key.clone(),
                external_key: Some(key.clone()),
                external_url: metadata_url,
                local_object: ExternalIssueLocalObject::task(task_id),
                local_project_id: project_id.map(str::to_string),
                local_sha: local_sha.map(str::to_string),
                local_state: local_state.map(str::to_string),
                idempotency_key: format!("atlassian:jira:{key}:task:{task_id}"),
                metadata_json: composer_metadata.map(str::to_string),
            })
            .await
            .map(Some)
    }

    pub async fn ensure_linear_task_link_from_metadata(
        &self,
        task_id: &str,
        project_id: Option<&str>,
        composer_metadata: Option<&str>,
        local_sha: Option<&str>,
        local_state: Option<&str>,
    ) -> AppResult<Option<ExternalIssueLink>> {
        let Some((external_id, external_key, external_url)) =
            primary_linear_issue_from_composer_metadata(composer_metadata)
        else {
            return Ok(None);
        };

        self.repo
            .upsert_link(ExternalIssueLinkUpsert {
                provider: "linear".to_string(),
                external_kind: "issue".to_string(),
                external_id: external_id.clone(),
                external_key,
                external_url,
                local_object: ExternalIssueLocalObject::task(task_id),
                local_project_id: project_id.map(str::to_string),
                local_sha: local_sha.map(str::to_string),
                local_state: local_state.map(str::to_string),
                idempotency_key: format!("linear:issue:{external_id}:task:{task_id}"),
                metadata_json: composer_metadata.map(str::to_string),
            })
            .await
            .map(Some)
    }
}

fn first_jira_reference_url(metadata: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(metadata).ok()?;
    value
        .get("composer_integration_references")?
        .as_array()?
        .iter()
        .find_map(|reference| {
            let provider = reference.get("provider")?.as_str()?;
            let kind = reference.get("kind")?.as_str()?;
            if provider == "atlassian" && kind == "jira" {
                reference
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            }
        })
}

#[cfg(test)]
#[path = "external_issue_link_service_tests.rs"]
mod tests;
