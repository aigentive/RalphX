use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Runtime};

use crate::application::{
    AtlassianIntegrationService, AtlassianJiraComment, AtlassianJiraTransition, ClickUpComment,
    ClickUpIntegrationService, ClickUpStatus, ClickUpUser, LinearComment, LinearIntegrationService,
    LinearUser, LinearWorkflowState,
};
use crate::domain::integrations::{
    ExternalIssueLink, ExternalIssueSyncRecordUpsert, ExternalIssueSyncStatus,
    ProviderTicketOperation, ProviderTicketOperationKind, ProviderTicketOperationStatus,
    ProviderTicketOperationUpsert,
};

use super::ExternalIssueLinkService;

pub const TICKETING_OPERATION_EVENT: &str = "ticketing:operation_updated";

const PROVIDER_JIRA: &str = "jira";
const PROVIDER_LINEAR: &str = "linear";
const PROVIDER_CLICKUP: &str = "clickup";
const LINK_PROVIDER_JIRA: &str = "atlassian";
const LINK_PROVIDER_CLICKUP: &str = "clickup";
const KIND_JIRA: &str = "jira";
const KIND_LINEAR: &str = "issue";
const KIND_CLICKUP: &str = "task";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingTicketIdentity {
    pub provider: String,
    pub id: String,
    pub key: Option<String>,
    pub local_project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingTransitionOption {
    pub to_state_id: String,
    pub provider_transition_id: Option<String>,
    pub name: String,
    pub category: String,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketTransitionRequest {
    pub ticket: TicketingTicketIdentity,
    pub to_state_id: String,
    pub provider_transition_id: Option<String>,
    pub client_operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAssignRequest {
    pub ticket: TicketingTicketIdentity,
    pub client_operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketCommentRequest {
    pub ticket: TicketingTicketIdentity,
    pub body_markdown: String,
    pub client_operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketSetLabelsRequest {
    pub ticket: TicketingTicketIdentity,
    pub labels: Vec<String>,
    pub client_operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingLabelResult {
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingCommentResult {
    pub id: Option<String>,
    pub body_markdown: String,
    pub body_text: String,
    pub author_name: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingPersonResult {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingMutationResult {
    pub ticket: TicketingTicketIdentity,
    pub operation: ProviderTicketOperation,
    pub idempotent: bool,
    pub transition: Option<TicketingTransitionOption>,
    pub assignee: Option<TicketingPersonResult>,
    pub comment: Option<TicketingCommentResult>,
    pub labels: Option<TicketingLabelResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingOperationEvent {
    pub provider: String,
    pub external_kind: String,
    pub external_id: String,
    pub external_key: Option<String>,
    pub local_project_id: Option<String>,
    pub operation_id: String,
    pub operation: ProviderTicketOperationKind,
    pub client_operation_id: String,
    pub status: ProviderTicketOperationStatus,
    pub provider_operation_id: Option<String>,
    pub error_message: Option<String>,
}

pub trait TicketingEventSink: Send + Sync {
    fn emit_ticketing_operation_event(&self, event: TicketingOperationEvent);
}

pub struct TauriTicketingEventSink<R: Runtime = tauri::Wry> {
    app_handle: AppHandle<R>,
}

impl<R: Runtime> TauriTicketingEventSink<R> {
    pub fn new(app_handle: AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

impl<R: Runtime> TicketingEventSink for TauriTicketingEventSink<R> {
    fn emit_ticketing_operation_event(&self, event: TicketingOperationEvent) {
        let _ = self.app_handle.emit(TICKETING_OPERATION_EVENT, event);
    }
}

#[derive(Debug)]
struct TicketIdentity {
    provider: String,
    external_kind: String,
    external_id: String,
    external_key: Option<String>,
    local_project_id: Option<String>,
}

struct BegunOperation {
    operation: ProviderTicketOperation,
    link: Option<ExternalIssueLink>,
    idempotent: bool,
}

pub struct TicketingService {
    atlassian: Arc<AtlassianIntegrationService>,
    linear: Arc<LinearIntegrationService>,
    clickup: Arc<ClickUpIntegrationService>,
    external_issues: Arc<ExternalIssueLinkService>,
    event_sink: Option<Arc<dyn TicketingEventSink>>,
}

impl TicketingService {
    pub fn new(
        atlassian: Arc<AtlassianIntegrationService>,
        linear: Arc<LinearIntegrationService>,
        clickup: Arc<ClickUpIntegrationService>,
        external_issues: Arc<ExternalIssueLinkService>,
    ) -> Self {
        Self {
            atlassian,
            linear,
            clickup,
            external_issues,
            event_sink: None,
        }
    }

    pub fn with_event_sink(mut self, event_sink: Arc<dyn TicketingEventSink>) -> Self {
        self.event_sink = Some(event_sink);
        self
    }

    pub async fn list_transitions(
        &self,
        ticket: &TicketingTicketIdentity,
    ) -> Result<Vec<TicketingTransitionOption>, String> {
        let identity = normalize_ticket_identity(ticket)?;
        self.ensure_write_capability(&identity.provider, ProviderTicketOperationKind::Transition)
            .await?;
        match identity.provider.as_str() {
            PROVIDER_JIRA => self
                .atlassian
                .list_jira_issue_transitions(&identity.external_id)
                .await
                .map(|items| items.into_iter().map(jira_transition_option).collect()),
            PROVIDER_LINEAR => self
                .linear
                .list_workflow_states(None)
                .await
                .map(|items| items.into_iter().map(linear_transition_option).collect()),
            PROVIDER_CLICKUP => {
                // ClickUp statuses are scoped to a Space, so resolve the task's
                // space first, then list that space's statuses as transitions.
                let task = self.clickup.fetch_task(&identity.external_id).await?;
                let space_id = task.space_id.ok_or_else(|| {
                    "ClickUp task is missing a space; cannot list statuses".to_string()
                })?;
                self.clickup
                    .list_statuses(&space_id)
                    .await
                    .map(|items| items.into_iter().map(clickup_transition_option).collect())
            }
            _ => unreachable!("ticket provider validated above"),
        }
    }

    pub async fn transition_ticket_status(
        &self,
        request: TicketTransitionRequest,
    ) -> Result<TicketingMutationResult, String> {
        let identity = normalize_ticket_identity(&request.ticket)?;
        let target_state_id =
            required_trimmed(&request.to_state_id, "Target state id is required")?;
        let client_operation_id = client_operation_id_or_derive(
            request.client_operation_id.as_deref(),
            &identity,
            ProviderTicketOperationKind::Transition,
            target_state_id,
        );
        let mut begun = self
            .begin_operation(
                &identity,
                ProviderTicketOperationKind::Transition,
                client_operation_id,
                transition_metadata(target_state_id, request.provider_transition_id.as_deref()),
            )
            .await?;
        if begun.idempotent {
            return Ok(self.result_for(request.ticket, begun, None, None, None, None));
        }

        if let Err(error) = self
            .ensure_write_capability(&identity.provider, ProviderTicketOperationKind::Transition)
            .await
        {
            self.fail_operation(&mut begun, error.clone()).await?;
            return Err(error);
        }

        let transitions = match self.list_transitions(&request.ticket).await {
            Ok(transitions) => transitions,
            Err(error) => {
                self.fail_operation(&mut begun, error.clone()).await?;
                return Err(error);
            }
        };
        let transition = match find_transition(
            &transitions,
            target_state_id,
            request.provider_transition_id.as_deref(),
        ) {
            Ok(transition) => transition,
            Err(error) => {
                self.fail_operation(&mut begun, error.clone()).await?;
                return Err(error);
            }
        };

        let provider_result = match identity.provider.as_str() {
            PROVIDER_JIRA => {
                let transition_id = transition
                    .provider_transition_id
                    .as_deref()
                    .ok_or_else(|| "Jira transition id is required".to_string());
                match transition_id {
                    Ok(transition_id) => {
                        self.atlassian
                            .transition_jira_issue(&identity.external_id, transition_id)
                            .await
                    }
                    Err(error) => Err(error),
                }
            }
            PROVIDER_LINEAR => {
                self.linear
                    .update_issue_state(&identity.external_id, &transition.to_state_id)
                    .await
            }
            PROVIDER_CLICKUP => {
                // ClickUp identifies statuses by name; `to_state_id` carries the
                // status name resolved from `list_transitions`.
                self.clickup
                    .update_task_status(&identity.external_id, &transition.to_state_id)
                    .await
            }
            _ => unreachable!("ticket provider validated above"),
        };
        if let Err(error) = provider_result {
            self.fail_operation(&mut begun, error.clone()).await?;
            return Err(error);
        }

        self.succeed_operation(
            &mut begun,
            transition.provider_transition_id.as_deref(),
            success_transition_metadata(&transition),
        )
        .await?;
        Ok(self.result_for(request.ticket, begun, Some(transition), None, None, None))
    }

    pub async fn assign_ticket(
        &self,
        request: TicketAssignRequest,
    ) -> Result<TicketingMutationResult, String> {
        let identity = normalize_ticket_identity(&request.ticket)?;
        let client_operation_id = client_operation_id_or_derive(
            request.client_operation_id.as_deref(),
            &identity,
            ProviderTicketOperationKind::Assign,
            "current_user",
        );
        let mut begun = self
            .begin_operation(
                &identity,
                ProviderTicketOperationKind::Assign,
                client_operation_id,
                json!({ "assignee": "current_user" }).to_string(),
            )
            .await?;
        if begun.idempotent {
            return Ok(self.result_for(request.ticket, begun, None, None, None, None));
        }

        if let Err(error) = self
            .ensure_write_capability(&identity.provider, ProviderTicketOperationKind::Assign)
            .await
        {
            self.fail_operation(&mut begun, error.clone()).await?;
            return Err(error);
        }

        let assignee = match identity.provider.as_str() {
            PROVIDER_JIRA => self
                .atlassian
                .assign_jira_issue_to_current_user(&identity.external_id)
                .await
                .map(|_| None),
            PROVIDER_LINEAR => self
                .linear
                .assign_issue_to_current_user(&identity.external_id)
                .await
                .map(linear_user_to_ticketing_person)
                .map(Some),
            PROVIDER_CLICKUP => self
                .clickup
                .assign_task_to_current_user(&identity.external_id)
                .await
                .map(clickup_user_to_ticketing_person)
                .map(Some),
            _ => unreachable!("ticket provider validated above"),
        };
        let assignee = match assignee {
            Ok(assignee) => assignee,
            Err(error) => {
                self.fail_operation(&mut begun, error.clone()).await?;
                return Err(error);
            }
        };

        self.succeed_operation(
            &mut begun,
            None,
            json!({ "assignee": "current_user" }).to_string(),
        )
        .await?;
        Ok(self.result_for(request.ticket, begun, None, assignee, None, None))
    }

    pub async fn clear_ticket_assignee(
        &self,
        request: TicketAssignRequest,
    ) -> Result<TicketingMutationResult, String> {
        let identity = normalize_ticket_identity(&request.ticket)?;
        let client_operation_id = client_operation_id_or_derive(
            request.client_operation_id.as_deref(),
            &identity,
            ProviderTicketOperationKind::Assign,
            "clear",
        );
        let mut begun = self
            .begin_operation(
                &identity,
                ProviderTicketOperationKind::Assign,
                client_operation_id,
                json!({ "assignee": null }).to_string(),
            )
            .await?;
        if begun.idempotent {
            return Ok(self.result_for(request.ticket, begun, None, None, None, None));
        }

        if let Err(error) = self
            .ensure_write_capability(&identity.provider, ProviderTicketOperationKind::Assign)
            .await
        {
            self.fail_operation(&mut begun, error.clone()).await?;
            return Err(error);
        }

        let provider_result = match identity.provider.as_str() {
            PROVIDER_JIRA => {
                self.atlassian
                    .clear_jira_issue_assignee(&identity.external_id)
                    .await
            }
            PROVIDER_LINEAR => {
                self.linear
                    .clear_issue_assignee(&identity.external_id)
                    .await
            }
            PROVIDER_CLICKUP => {
                self.clickup
                    .clear_task_assignee(&identity.external_id)
                    .await
            }
            _ => unreachable!("ticket provider validated above"),
        };
        if let Err(error) = provider_result {
            self.fail_operation(&mut begun, error.clone()).await?;
            return Err(error);
        }

        self.succeed_operation(&mut begun, None, json!({ "assignee": null }).to_string())
            .await?;
        Ok(self.result_for(request.ticket, begun, None, None, None, None))
    }

    pub async fn add_ticket_comment(
        &self,
        request: TicketCommentRequest,
    ) -> Result<TicketingMutationResult, String> {
        let identity = normalize_ticket_identity(&request.ticket)?;
        let body_markdown =
            required_trimmed(&request.body_markdown, "Ticket comment body is required")?;
        let body_hash = stable_hash(body_markdown);
        let client_operation_id = client_operation_id_or_derive(
            request.client_operation_id.as_deref(),
            &identity,
            ProviderTicketOperationKind::Comment,
            &body_hash,
        );
        let mut begun = self
            .begin_operation(
                &identity,
                ProviderTicketOperationKind::Comment,
                client_operation_id,
                json!({ "bodyHash": body_hash }).to_string(),
            )
            .await?;
        if begun.idempotent {
            return Ok(self.result_for(request.ticket, begun, None, None, None, None));
        }

        if let Err(error) = self
            .ensure_write_capability(&identity.provider, ProviderTicketOperationKind::Comment)
            .await
        {
            self.fail_operation(&mut begun, error.clone()).await?;
            return Err(error);
        }

        let comment = match identity.provider.as_str() {
            PROVIDER_JIRA => self
                .atlassian
                .add_jira_comment(&identity.external_id, body_markdown)
                .await
                .map(jira_comment_result),
            PROVIDER_LINEAR => self
                .linear
                .create_comment(&identity.external_id, body_markdown)
                .await
                .map(linear_comment_result),
            PROVIDER_CLICKUP => self
                .clickup
                .create_comment(&identity.external_id, body_markdown)
                .await
                .map(clickup_comment_result),
            _ => unreachable!("ticket provider validated above"),
        };
        let comment = match comment {
            Ok(comment) => comment,
            Err(error) => {
                self.fail_operation(&mut begun, error.clone()).await?;
                return Err(error);
            }
        };

        self.succeed_operation(
            &mut begun,
            comment.id.as_deref(),
            json!({ "commentId": comment.id }).to_string(),
        )
        .await?;
        Ok(self.result_for(request.ticket, begun, None, None, Some(comment), None))
    }

    pub async fn set_ticket_labels(
        &self,
        request: TicketSetLabelsRequest,
    ) -> Result<TicketingMutationResult, String> {
        let identity = normalize_ticket_identity(&request.ticket)?;
        let normalized = normalize_labels(&request.labels);
        let labels_hash = stable_hash(&normalized.join("\u{1f}"));
        let client_operation_id = client_operation_id_or_derive(
            request.client_operation_id.as_deref(),
            &identity,
            ProviderTicketOperationKind::SetLabels,
            &labels_hash,
        );
        let mut begun = self
            .begin_operation(
                &identity,
                ProviderTicketOperationKind::SetLabels,
                client_operation_id,
                json!({ "labelsHash": labels_hash, "count": normalized.len() }).to_string(),
            )
            .await?;
        if begun.idempotent {
            return Ok(self.result_for(request.ticket, begun, None, None, None, None));
        }

        if let Err(error) = self
            .ensure_write_capability(&identity.provider, ProviderTicketOperationKind::SetLabels)
            .await
        {
            self.fail_operation(&mut begun, error.clone()).await?;
            return Err(error);
        }

        let write_result = match identity.provider.as_str() {
            PROVIDER_JIRA => {
                self.atlassian
                    .set_jira_issue_labels(&identity.external_id, normalized.clone())
                    .await
            }
            PROVIDER_LINEAR => {
                self.linear
                    .set_issue_labels(&identity.external_id, normalized.clone())
                    .await
            }
            PROVIDER_CLICKUP => {
                // ClickUp labels are free-text tags; forward the normalized set.
                self.clickup
                    .set_task_tags(&identity.external_id, normalized.clone())
                    .await
            }
            _ => unreachable!("ticket provider validated above"),
        };
        if let Err(error) = write_result {
            self.fail_operation(&mut begun, error.clone()).await?;
            return Err(error);
        }

        self.succeed_operation(
            &mut begun,
            None,
            json!({ "labels": normalized }).to_string(),
        )
        .await?;
        Ok(self.result_for(
            request.ticket,
            begun,
            None,
            None,
            None,
            Some(TicketingLabelResult { labels: normalized }),
        ))
    }

    async fn ensure_write_capability(
        &self,
        provider: &str,
        operation: ProviderTicketOperationKind,
    ) -> Result<(), String> {
        match provider {
            PROVIDER_JIRA => {
                let settings = self.atlassian.get_settings().await?;
                if !settings.enabled
                    || settings.validation_status
                        != crate::domain::integrations::IntegrationValidationStatus::Valid
                {
                    return Err("Jira integration is not enabled".to_string());
                }
                if !settings.jira_available {
                    return Err(
                        "Jira ticket write-back is not available for this connection".to_string(),
                    );
                }
            }
            PROVIDER_LINEAR => {
                let settings = self.linear.get_settings().await?;
                if !settings.enabled
                    || settings.validation_status
                        != crate::domain::integrations::IntegrationValidationStatus::Valid
                {
                    return Err("Linear integration is not enabled".to_string());
                }
                if !settings.issue_search_available {
                    return Err(
                        "Linear ticket write-back is not available for this connection".to_string(),
                    );
                }
            }
            PROVIDER_CLICKUP => {
                let settings = self.clickup.get_settings().await?;
                if !settings.enabled
                    || settings.validation_status
                        != crate::domain::integrations::IntegrationValidationStatus::Valid
                {
                    return Err("ClickUp integration is not enabled".to_string());
                }
                if !settings.task_search_available {
                    return Err(
                        "ClickUp ticket write-back is not available for this connection"
                            .to_string(),
                    );
                }
            }
            _ => return Err(format!("Unknown ticketing provider: {provider}")),
        }
        let _ = operation;
        Ok(())
    }

    async fn begin_operation(
        &self,
        identity: &TicketIdentity,
        operation: ProviderTicketOperationKind,
        client_operation_id: String,
        metadata_json: String,
    ) -> Result<BegunOperation, String> {
        if let Some(existing) = self
            .external_issues
            .find_provider_ticket_operation_by_client_operation_id(&client_operation_id)
            .await
            .map_err(|error| error.to_string())?
        {
            if !ticket_operation_matches_request(&existing, identity, operation) {
                return Err(format!(
                    "Client operation id {} was already used for a different ticketing operation",
                    client_operation_id
                ));
            }
            if existing.status == ProviderTicketOperationStatus::Succeeded {
                return Ok(BegunOperation {
                    operation: existing,
                    link: None,
                    idempotent: true,
                });
            }
        }

        let link = self.find_link(identity).await?;
        let operation = self
            .external_issues
            .upsert_provider_ticket_operation(ProviderTicketOperationUpsert {
                provider: identity.provider.clone(),
                external_kind: identity.external_kind.clone(),
                external_id: identity.external_id.clone(),
                external_key: identity.external_key.clone(),
                link_id: link.as_ref().map(|link| link.id.clone()),
                local_project_id: identity.local_project_id.clone(),
                operation,
                client_operation_id,
                status: ProviderTicketOperationStatus::Pending,
                provider_operation_id: None,
                error_message: None,
                metadata_json: Some(metadata_json),
            })
            .await
            .map_err(|error| error.to_string())?;
        self.emit_operation_event(&operation);
        Ok(BegunOperation {
            operation,
            link,
            idempotent: false,
        })
    }

    async fn find_link(
        &self,
        identity: &TicketIdentity,
    ) -> Result<Option<ExternalIssueLink>, String> {
        let link_provider = match identity.provider.as_str() {
            PROVIDER_JIRA => LINK_PROVIDER_JIRA,
            PROVIDER_CLICKUP => LINK_PROVIDER_CLICKUP,
            _ => PROVIDER_LINEAR,
        };
        self.external_issues
            .find_link_by_external_identity(
                link_provider,
                &identity.external_kind,
                &identity.external_id,
            )
            .await
            .map_err(|error| error.to_string())
    }

    async fn fail_operation(
        &self,
        begun: &mut BegunOperation,
        error: String,
    ) -> Result<(), String> {
        let metadata = json!({ "error": error }).to_string();
        let updated = self
            .finish_operation(
                begun,
                ProviderTicketOperationStatus::Failed,
                None,
                Some(&error),
                Some(metadata),
            )
            .await?;
        begun.operation = updated;
        Ok(())
    }

    async fn succeed_operation(
        &self,
        begun: &mut BegunOperation,
        provider_operation_id: Option<&str>,
        metadata_json: String,
    ) -> Result<(), String> {
        let updated = self
            .finish_operation(
                begun,
                ProviderTicketOperationStatus::Succeeded,
                provider_operation_id,
                None,
                Some(metadata_json),
            )
            .await?;
        begun.operation = updated;
        Ok(())
    }

    async fn finish_operation(
        &self,
        begun: &BegunOperation,
        status: ProviderTicketOperationStatus,
        provider_operation_id: Option<&str>,
        error_message: Option<&str>,
        metadata_json: Option<String>,
    ) -> Result<ProviderTicketOperation, String> {
        let operation = self
            .external_issues
            .update_provider_ticket_operation_status(
                &begun.operation.id,
                status,
                provider_operation_id,
                error_message,
            )
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                format!(
                    "Ticketing operation {} no longer exists; refusing to emit stale operation result",
                    begun.operation.id
                )
            })?;
        if let Some(link) = begun.link.as_ref() {
            if let Err(error) = self
                .external_issues
                .upsert_sync_record(ExternalIssueSyncRecordUpsert {
                    link_id: link.id.clone(),
                    sync_kind: sync_kind_for(operation.operation).to_string(),
                    idempotency_key: format!(
                        "ticketing:{}:{}",
                        operation.operation.as_str(),
                        operation.client_operation_id
                    ),
                    local_sha: link.local_sha.clone(),
                    local_state: link.local_state.clone(),
                    external_version: provider_operation_id.map(str::to_string),
                    status: sync_status_for(status),
                    error_message: error_message.map(str::to_string),
                    metadata_json,
                })
                .await
            {
                tracing::warn!(
                    error = %error,
                    operation_id = %operation.id,
                    "failed to update external issue sync record for ticket operation"
                );
            }
        }
        self.emit_operation_event(&operation);
        Ok(operation)
    }

    fn result_for(
        &self,
        ticket: TicketingTicketIdentity,
        begun: BegunOperation,
        transition: Option<TicketingTransitionOption>,
        assignee: Option<TicketingPersonResult>,
        comment: Option<TicketingCommentResult>,
        labels: Option<TicketingLabelResult>,
    ) -> TicketingMutationResult {
        TicketingMutationResult {
            ticket,
            operation: begun.operation,
            idempotent: begun.idempotent,
            transition,
            assignee,
            comment,
            labels,
        }
    }

    fn emit_operation_event(&self, operation: &ProviderTicketOperation) {
        let Some(event_sink) = self.event_sink.as_ref() else {
            return;
        };
        event_sink.emit_ticketing_operation_event(TicketingOperationEvent {
            provider: operation.provider.clone(),
            external_kind: operation.external_kind.clone(),
            external_id: operation.external_id.clone(),
            external_key: operation.external_key.clone(),
            local_project_id: operation.local_project_id.clone(),
            operation_id: operation.id.clone(),
            operation: operation.operation,
            client_operation_id: operation.client_operation_id.clone(),
            status: operation.status,
            provider_operation_id: operation.provider_operation_id.clone(),
            error_message: operation.error_message.clone(),
        });
    }
}

fn normalize_ticket_identity(ticket: &TicketingTicketIdentity) -> Result<TicketIdentity, String> {
    let provider = ticket.provider.trim();
    let provider = match provider {
        PROVIDER_JIRA | PROVIDER_LINEAR | PROVIDER_CLICKUP => provider.to_string(),
        other => return Err(format!("Unknown ticketing provider: {other}")),
    };
    let raw_id = required_trimmed(&ticket.id, "Ticket id is required")?;
    let key = ticket
        .key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    // Only Jira keys its external object by the human-readable issue key; Linear
    // and ClickUp address objects by their opaque id.
    let external_id = if provider == PROVIDER_JIRA {
        key.clone().unwrap_or_else(|| raw_id.to_string())
    } else {
        raw_id.to_string()
    };
    let external_kind = match provider.as_str() {
        PROVIDER_JIRA => KIND_JIRA,
        PROVIDER_CLICKUP => KIND_CLICKUP,
        _ => KIND_LINEAR,
    };
    Ok(TicketIdentity {
        external_kind: external_kind.to_string(),
        provider,
        external_id,
        external_key: key,
        local_project_id: ticket
            .local_project_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn ticket_operation_matches_request(
    operation: &ProviderTicketOperation,
    identity: &TicketIdentity,
    requested_operation: ProviderTicketOperationKind,
) -> bool {
    operation.provider == identity.provider
        && operation.external_kind == identity.external_kind
        && operation.external_id == identity.external_id
        && operation.external_key == identity.external_key
        && operation.local_project_id == identity.local_project_id
        && operation.operation == requested_operation
}

fn required_trimmed<'a>(value: &'a str, message: &str) -> Result<&'a str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(message.to_string())
    } else {
        Ok(trimmed)
    }
}

fn client_operation_id_or_derive(
    provided: Option<&str>,
    identity: &TicketIdentity,
    operation: ProviderTicketOperationKind,
    suffix: &str,
) -> String {
    if let Some(value) = provided.map(str::trim).filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    format!(
        "ticketing:{}:{}:{}:{}:{}",
        identity.provider,
        identity.external_kind,
        identity.external_id,
        operation.as_str(),
        stable_hash(suffix)
    )
}

fn stable_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

/// Normalizes a desired label set: trims each label, drops empties, removes
/// case-insensitive duplicates (keeping the first surface form seen), and sorts
/// case-insensitively so that the same logical set always hashes the same way
/// regardless of input order or whitespace. Original casing is preserved in the
/// returned values so the provider/UI sees the user's intended label text.
fn normalize_labels(labels: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for label in labels {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !normalized
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            normalized.push(trimmed.to_string());
        }
    }
    normalized.sort_by_key(|label| label.to_lowercase());
    normalized
}

fn find_transition(
    transitions: &[TicketingTransitionOption],
    to_state_id: &str,
    provider_transition_id: Option<&str>,
) -> Result<TicketingTransitionOption, String> {
    let transition = if let Some(provider_transition_id) = provider_transition_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        transitions.iter().find(|transition| {
            transition.provider_transition_id.as_deref() == Some(provider_transition_id)
        })
    } else {
        transitions
            .iter()
            .find(|transition| transition.to_state_id == to_state_id)
    };
    let Some(transition) = transition.cloned() else {
        return Err(format!("Transition target is not available: {to_state_id}"));
    };
    if let Some(disabled_reason) = transition.disabled_reason.as_ref() {
        return Err(disabled_reason.clone());
    }
    Ok(transition)
}

fn jira_transition_option(value: AtlassianJiraTransition) -> TicketingTransitionOption {
    TicketingTransitionOption {
        to_state_id: value.to_state_id,
        provider_transition_id: Some(value.provider_transition_id),
        name: value.name,
        category: value.category,
        disabled_reason: None,
    }
}

fn linear_transition_option(value: LinearWorkflowState) -> TicketingTransitionOption {
    TicketingTransitionOption {
        to_state_id: value.id,
        provider_transition_id: None,
        name: value.name,
        category: value.category,
        disabled_reason: None,
    }
}

fn jira_comment_result(value: AtlassianJiraComment) -> TicketingCommentResult {
    TicketingCommentResult {
        id: value.id,
        body_markdown: value.body_markdown,
        body_text: value.body_text,
        author_name: value.author,
        created_at: value.created_at,
        updated_at: value.updated_at,
    }
}

fn linear_comment_result(value: LinearComment) -> TicketingCommentResult {
    TicketingCommentResult {
        id: Some(value.id),
        body_markdown: value.body.clone(),
        body_text: value.body,
        author_name: value.author_name,
        created_at: value.created_at,
        updated_at: value.updated_at,
    }
}

fn linear_user_to_ticketing_person(value: LinearUser) -> TicketingPersonResult {
    TicketingPersonResult {
        id: Some(value.id),
        name: value.name.unwrap_or_else(|| "Me".to_string()),
    }
}

fn clickup_transition_option(value: ClickUpStatus) -> TicketingTransitionOption {
    TicketingTransitionOption {
        // ClickUp's status update API is keyed by status name, so the name is
        // also the transition target id. ClickUp has no separate transition id.
        to_state_id: value.status.clone(),
        provider_transition_id: None,
        name: value.status,
        category: value.category,
        disabled_reason: None,
    }
}

fn clickup_comment_result(value: ClickUpComment) -> TicketingCommentResult {
    TicketingCommentResult {
        id: Some(value.id),
        body_markdown: value.body.clone(),
        body_text: value.body,
        author_name: value.author_name,
        created_at: value.created_at,
        // ClickUp comment creation does not return an updated timestamp.
        updated_at: None,
    }
}

fn clickup_user_to_ticketing_person(value: ClickUpUser) -> TicketingPersonResult {
    TicketingPersonResult {
        id: Some(value.id.to_string()),
        name: value.username.unwrap_or_else(|| "Me".to_string()),
    }
}

fn transition_metadata(to_state_id: &str, provider_transition_id: Option<&str>) -> String {
    json!({
        "toStateId": to_state_id,
        "providerTransitionId": provider_transition_id,
    })
    .to_string()
}

fn success_transition_metadata(transition: &TicketingTransitionOption) -> String {
    json!({
        "toStateId": transition.to_state_id,
        "providerTransitionId": transition.provider_transition_id,
        "name": transition.name,
        "category": transition.category,
    })
    .to_string()
}

fn sync_kind_for(operation: ProviderTicketOperationKind) -> &'static str {
    match operation {
        ProviderTicketOperationKind::Transition => "ticket_transition",
        ProviderTicketOperationKind::Assign => "ticket_assignment",
        ProviderTicketOperationKind::Comment => "ticket_comment",
        ProviderTicketOperationKind::SetLabels => "ticket_labels",
    }
}

fn sync_status_for(status: ProviderTicketOperationStatus) -> ExternalIssueSyncStatus {
    match status {
        ProviderTicketOperationStatus::Pending => ExternalIssueSyncStatus::Pending,
        ProviderTicketOperationStatus::Succeeded => ExternalIssueSyncStatus::Succeeded,
        ProviderTicketOperationStatus::Failed
        | ProviderTicketOperationStatus::TimedOut
        | ProviderTicketOperationStatus::Canceled => ExternalIssueSyncStatus::Failed,
    }
}

#[cfg(test)]
#[path = "ticketing_service_tests.rs"]
mod tests;
