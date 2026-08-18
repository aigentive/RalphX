use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

mod ticketing;
pub use ticketing::{
    ObservedTicketingStatus, ProviderTicketOperation, ProviderTicketOperationKind,
    ProviderTicketOperationStatus, ProviderTicketOperationUpsert, TicketingStatusCatalogEntry,
    TicketingStatusCatalogUpsert, TicketingStatusPresentationPatch,
};

mod clickup_settings;
pub use clickup_settings::{ClickUpIntegrationSettings, ClickUpIntegrationSettingsRepository};

mod granola_settings;
pub use granola_settings::{GranolaIntegrationSettings, GranolaIntegrationSettingsRepository};

pub mod linear_settings;
pub mod linear_webhook;

pub mod jira_agile_types;
pub use jira_agile_types::{
    JiraBoardColumn, JiraBoardConfiguration, JiraBoardSummary, JiraSprintSummary,
};

pub mod atlassian_resources;
pub use atlassian_resources::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianCredential,
    AtlassianJiraAttachment, AtlassianJiraChildIssue, AtlassianJiraComment,
    AtlassianJiraTransition, AtlassianOAuthAuthorization, AtlassianOAuthResource,
    AtlassianOAuthTokenResponse, AtlassianResourceContent, AtlassianResourceKind,
    AtlassianResourceSummary, AtlassianResourceUrlResolution, ConfluenceSpaceSummary,
    JiraCommentsPage, JiraIssueDetail, JiraProjectSummary, JiraStatusSummary, JiraUserSummary,
};

pub mod atlassian_api_error;
pub use atlassian_api_error::AtlassianApiError;
#[cfg(test)]
mod atlassian_api_error_tests;

pub mod atlassian_mcp_ops;
pub use atlassian_mcp_ops::{
    validate_atlassian_raw_path, AtlassianRawMethod, AtlassianRawResponse, ConfluencePageContent,
    ConfluencePageCreateRequest, ConfluencePageUpdateRequest, JiraIssueCreateRequest,
    JiraIssueCreated, JiraIssueUpdateRequest, ATLASSIAN_RAW_PATH_PREFIXES,
    ATLASSIAN_RAW_RESPONSE_MAX_BYTES,
};

pub mod clickup_tasks;
pub use clickup_tasks::{
    ClickUpApiClient, ClickUpAttachment, ClickUpAuthContext, ClickUpComment, ClickUpFolder,
    ClickUpList, ClickUpSpace, ClickUpStatus, ClickUpTag, ClickUpTaskContent, ClickUpTaskListOptions,
    ClickUpTaskSummary, ClickUpUser, ClickUpWorkspace,
};

pub mod granola_notes;
pub use granola_notes::{
    is_valid_granola_note_id, GranolaApiClient, GranolaApiError, GranolaAuthContext,
    GranolaNoteDetail, GranolaNoteListPage, GranolaNoteSummary, GranolaTranscriptEntry,
};
pub use linear_settings::{
    LinearApiClient, LinearAttachment, LinearAuthContext, LinearComment, LinearIntegrationSettings,
    LinearIntegrationSettingsRepository, LinearIssueContent, LinearIssueSummary, LinearLabel,
    LinearProject, LinearUser, LinearWorkflowState,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationValidationStatus {
    NotConfigured,
    Pending,
    Valid,
    Invalid,
}

impl IntegrationValidationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::Pending => "pending",
            Self::Valid => "valid",
            Self::Invalid => "invalid",
        }
    }
}

impl Default for IntegrationValidationStatus {
    fn default() -> Self {
        Self::NotConfigured
    }
}

impl std::str::FromStr for IntegrationValidationStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "not_configured" => Ok(Self::NotConfigured),
            "pending" => Ok(Self::Pending),
            "valid" => Ok(Self::Valid),
            "invalid" => Ok(Self::Invalid),
            other => Err(format!("Unknown integration validation status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtlassianAuthMethod {
    ApiToken,
    OAuth,
}

impl AtlassianAuthMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiToken => "api_token",
            Self::OAuth => "oauth",
        }
    }
}

impl Default for AtlassianAuthMethod {
    fn default() -> Self {
        Self::ApiToken
    }
}

impl std::str::FromStr for AtlassianAuthMethod {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "api_token" => Ok(Self::ApiToken),
            "oauth" => Ok(Self::OAuth),
            other => Err(format!("Unknown Atlassian auth method: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianIntegrationSettings {
    pub enabled: bool,
    pub auth_method: AtlassianAuthMethod,
    pub site_url: Option<String>,
    pub email: Option<String>,
    pub token_secret_ref: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub oauth_client_secret_ref: Option<String>,
    pub oauth_access_token_ref: Option<String>,
    pub oauth_refresh_token_ref: Option<String>,
    pub oauth_cloud_id: Option<String>,
    pub oauth_scopes: Option<String>,
    pub oauth_access_token_expires_at: Option<DateTime<Utc>>,
    pub validation_status: IntegrationValidationStatus,
    pub jira_available: bool,
    pub confluence_available: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for AtlassianIntegrationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auth_method: AtlassianAuthMethod::ApiToken,
            site_url: None,
            email: None,
            token_secret_ref: None,
            oauth_client_id: None,
            oauth_redirect_uri: None,
            oauth_client_secret_ref: None,
            oauth_access_token_ref: None,
            oauth_refresh_token_ref: None,
            oauth_cloud_id: None,
            oauth_scopes: None,
            oauth_access_token_expires_at: None,
            validation_status: IntegrationValidationStatus::NotConfigured,
            jira_available: false,
            confluence_available: false,
            last_validated_at: None,
            last_error: None,
            updated_at: Utc::now(),
        }
    }
}

#[async_trait]
pub trait AtlassianIntegrationSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>>;

    async fn upsert(
        &self,
        settings: &AtlassianIntegrationSettings,
    ) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalIssueLocalObjectKind {
    Task,
    Proposal,
    Session,
    PullRequest,
    Check,
    Qa,
}

impl ExternalIssueLocalObjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Proposal => "proposal",
            Self::Session => "session",
            Self::PullRequest => "pull_request",
            Self::Check => "check",
            Self::Qa => "qa",
        }
    }
}

impl std::str::FromStr for ExternalIssueLocalObjectKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "task" => Ok(Self::Task),
            "proposal" => Ok(Self::Proposal),
            "session" => Ok(Self::Session),
            "pull_request" => Ok(Self::PullRequest),
            "check" => Ok(Self::Check),
            "qa" => Ok(Self::Qa),
            other => Err(format!("Unknown external issue local object kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIssueLocalObject {
    pub kind: ExternalIssueLocalObjectKind,
    pub id: String,
}

impl ExternalIssueLocalObject {
    pub fn new(kind: ExternalIssueLocalObjectKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }

    pub fn task(id: impl Into<String>) -> Self {
        Self::new(ExternalIssueLocalObjectKind::Task, id)
    }

    pub fn proposal(id: impl Into<String>) -> Self {
        Self::new(ExternalIssueLocalObjectKind::Proposal, id)
    }

    pub fn session(id: impl Into<String>) -> Self {
        Self::new(ExternalIssueLocalObjectKind::Session, id)
    }

    pub fn pull_request(id: impl Into<String>) -> Self {
        Self::new(ExternalIssueLocalObjectKind::PullRequest, id)
    }

    pub fn check(id: impl Into<String>) -> Self {
        Self::new(ExternalIssueLocalObjectKind::Check, id)
    }

    pub fn qa(id: impl Into<String>) -> Self {
        Self::new(ExternalIssueLocalObjectKind::Qa, id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalIssueSyncStatus {
    Pending,
    Succeeded,
    Failed,
    Skipped,
}

impl ExternalIssueSyncStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

impl std::str::FromStr for ExternalIssueSyncStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            other => Err(format!("Unknown external issue sync status: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIssueLink {
    pub id: String,
    pub provider: String,
    pub external_kind: String,
    pub external_id: String,
    pub external_key: Option<String>,
    pub external_url: Option<String>,
    pub local_object: ExternalIssueLocalObject,
    pub local_project_id: Option<String>,
    pub local_sha: Option<String>,
    pub local_state: Option<String>,
    pub idempotency_key: String,
    pub metadata_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIssueLinkUpsert {
    pub provider: String,
    pub external_kind: String,
    pub external_id: String,
    pub external_key: Option<String>,
    pub external_url: Option<String>,
    pub local_object: ExternalIssueLocalObject,
    pub local_project_id: Option<String>,
    pub local_sha: Option<String>,
    pub local_state: Option<String>,
    pub idempotency_key: String,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIssueSyncRecord {
    pub id: String,
    pub link_id: String,
    pub sync_kind: String,
    pub idempotency_key: String,
    pub local_sha: Option<String>,
    pub local_state: Option<String>,
    pub external_version: Option<String>,
    pub status: ExternalIssueSyncStatus,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub metadata_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIssueSyncRecordUpsert {
    pub link_id: String,
    pub sync_kind: String,
    pub idempotency_key: String,
    pub local_sha: Option<String>,
    pub local_state: Option<String>,
    pub external_version: Option<String>,
    pub status: ExternalIssueSyncStatus,
    pub error_message: Option<String>,
    pub metadata_json: Option<String>,
}

#[async_trait]
pub trait ExternalIssueLinkRepository: Send + Sync {
    async fn upsert_link(&self, input: ExternalIssueLinkUpsert) -> AppResult<ExternalIssueLink>;

    async fn get_link(&self, id: &str) -> AppResult<Option<ExternalIssueLink>>;

    async fn find_link_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<ExternalIssueLink>>;

    async fn find_link_by_external_identity(
        &self,
        provider: &str,
        external_kind: &str,
        external_id: &str,
    ) -> AppResult<Option<ExternalIssueLink>>;

    async fn list_links_for_local(
        &self,
        local_object: &ExternalIssueLocalObject,
    ) -> AppResult<Vec<ExternalIssueLink>>;

    async fn upsert_sync_record(
        &self,
        input: ExternalIssueSyncRecordUpsert,
    ) -> AppResult<ExternalIssueSyncRecord>;

    async fn find_sync_record_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<ExternalIssueSyncRecord>>;

    async fn list_sync_records_for_link(
        &self,
        link_id: &str,
    ) -> AppResult<Vec<ExternalIssueSyncRecord>>;

    async fn update_sync_status(
        &self,
        sync_record_id: &str,
        status: ExternalIssueSyncStatus,
        external_version: Option<&str>,
        error_message: Option<&str>,
    ) -> AppResult<Option<ExternalIssueSyncRecord>>;

    async fn upsert_provider_ticket_operation(
        &self,
        input: ProviderTicketOperationUpsert,
    ) -> AppResult<ProviderTicketOperation>;

    async fn find_provider_ticket_operation_by_client_operation_id(
        &self,
        client_operation_id: &str,
    ) -> AppResult<Option<ProviderTicketOperation>>;

    async fn list_provider_ticket_operations_for_ticket(
        &self,
        provider: &str,
        external_kind: &str,
        external_id: &str,
        external_key: Option<&str>,
        local_project_id: Option<&str>,
    ) -> AppResult<Vec<ProviderTicketOperation>>;

    async fn update_provider_ticket_operation_status(
        &self,
        operation_id: &str,
        status: ProviderTicketOperationStatus,
        provider_operation_id: Option<&str>,
        error_message: Option<&str>,
    ) -> AppResult<Option<ProviderTicketOperation>>;
}

#[async_trait]
pub trait TicketingStatusCatalogRepository: Send + Sync {
    async fn list_status_catalog(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>>;

    async fn upsert_status_catalog_entry(
        &self,
        input: TicketingStatusCatalogUpsert,
    ) -> AppResult<TicketingStatusCatalogEntry>;

    async fn update_status_presentation(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
        patches: Vec<TicketingStatusPresentationPatch>,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>>;

    async fn mark_missing_statuses_stale(
        &self,
        provider: &str,
        scope_kind: &str,
        scope_id: &str,
        observed_provider_status_ids: &[String],
        stale_since: DateTime<Utc>,
    ) -> AppResult<Vec<TicketingStatusCatalogEntry>>;
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn validation_status_round_trips_storage_values() {
        let cases = [
            (IntegrationValidationStatus::NotConfigured, "not_configured"),
            (IntegrationValidationStatus::Pending, "pending"),
            (IntegrationValidationStatus::Valid, "valid"),
            (IntegrationValidationStatus::Invalid, "invalid"),
        ];

        for (status, value) in cases {
            assert_eq!(status.as_str(), value);
            assert_eq!(
                IntegrationValidationStatus::from_str(value).unwrap(),
                status
            );
        }
        assert_eq!(
            IntegrationValidationStatus::default(),
            IntegrationValidationStatus::NotConfigured
        );
        assert!(IntegrationValidationStatus::from_str("expired").is_err());
    }

    #[test]
    fn auth_method_round_trips_storage_values() {
        let cases = [
            (AtlassianAuthMethod::ApiToken, "api_token"),
            (AtlassianAuthMethod::OAuth, "oauth"),
        ];

        for (method, value) in cases {
            assert_eq!(method.as_str(), value);
            assert_eq!(AtlassianAuthMethod::from_str(value).unwrap(), method);
        }
        assert_eq!(
            AtlassianAuthMethod::default(),
            AtlassianAuthMethod::ApiToken
        );
        assert!(AtlassianAuthMethod::from_str("password").is_err());
    }

    #[test]
    fn external_issue_local_object_kinds_round_trip_storage_values() {
        let cases = [
            (ExternalIssueLocalObjectKind::Task, "task"),
            (ExternalIssueLocalObjectKind::Proposal, "proposal"),
            (ExternalIssueLocalObjectKind::Session, "session"),
            (ExternalIssueLocalObjectKind::PullRequest, "pull_request"),
            (ExternalIssueLocalObjectKind::Check, "check"),
            (ExternalIssueLocalObjectKind::Qa, "qa"),
        ];

        for (kind, value) in cases {
            assert_eq!(kind.as_str(), value);
            assert_eq!(ExternalIssueLocalObjectKind::from_str(value).unwrap(), kind);
        }
        assert!(ExternalIssueLocalObjectKind::from_str("milestone").is_err());
    }

    #[test]
    fn external_issue_local_object_constructors_set_expected_kind() {
        let cases = [
            (
                ExternalIssueLocalObject::task("task-1"),
                ExternalIssueLocalObjectKind::Task,
            ),
            (
                ExternalIssueLocalObject::proposal("proposal-1"),
                ExternalIssueLocalObjectKind::Proposal,
            ),
            (
                ExternalIssueLocalObject::session("session-1"),
                ExternalIssueLocalObjectKind::Session,
            ),
            (
                ExternalIssueLocalObject::pull_request("pr-1"),
                ExternalIssueLocalObjectKind::PullRequest,
            ),
            (
                ExternalIssueLocalObject::check("check-1"),
                ExternalIssueLocalObjectKind::Check,
            ),
            (
                ExternalIssueLocalObject::qa("qa-1"),
                ExternalIssueLocalObjectKind::Qa,
            ),
        ];

        for (object, kind) in cases {
            assert_eq!(object.kind, kind);
            assert!(object.id.ends_with("-1"));
        }
    }

    #[test]
    fn external_issue_sync_status_round_trips_storage_values() {
        let cases = [
            (ExternalIssueSyncStatus::Pending, "pending"),
            (ExternalIssueSyncStatus::Succeeded, "succeeded"),
            (ExternalIssueSyncStatus::Failed, "failed"),
            (ExternalIssueSyncStatus::Skipped, "skipped"),
        ];

        for (status, value) in cases {
            assert_eq!(status.as_str(), value);
            assert_eq!(ExternalIssueSyncStatus::from_str(value).unwrap(), status);
        }
        assert!(ExternalIssueSyncStatus::from_str("retrying").is_err());
    }

    #[test]
    fn provider_ticket_operation_kind_round_trips_storage_values() {
        let cases = [
            (ProviderTicketOperationKind::Transition, "transition"),
            (ProviderTicketOperationKind::Assign, "assign"),
            (ProviderTicketOperationKind::Comment, "comment"),
            (ProviderTicketOperationKind::SetLabels, "set_labels"),
        ];

        for (kind, value) in cases {
            assert_eq!(kind.as_str(), value);
            assert_eq!(ProviderTicketOperationKind::from_str(value).unwrap(), kind);
        }
    }

    #[test]
    fn provider_ticket_operation_kind_from_str_rejects_unknown_variant() {
        let error = ProviderTicketOperationKind::from_str("archive")
            .expect_err("unknown operation kind should fail to parse");
        assert_eq!(error, "Unknown provider ticket operation kind: archive");
        assert!(ProviderTicketOperationKind::from_str("").is_err());
        assert!(ProviderTicketOperationKind::from_str("Transition").is_err());
    }

    #[test]
    fn provider_ticket_operation_status_round_trips_storage_values() {
        let cases = [
            (ProviderTicketOperationStatus::Pending, "pending"),
            (ProviderTicketOperationStatus::Succeeded, "succeeded"),
            (ProviderTicketOperationStatus::Failed, "failed"),
            (ProviderTicketOperationStatus::TimedOut, "timed_out"),
            (ProviderTicketOperationStatus::Canceled, "canceled"),
        ];

        for (status, value) in cases {
            assert_eq!(status.as_str(), value);
            assert_eq!(
                ProviderTicketOperationStatus::from_str(value).unwrap(),
                status
            );
        }
    }

    #[test]
    fn provider_ticket_operation_status_from_str_rejects_unknown_variant() {
        let error = ProviderTicketOperationStatus::from_str("retrying")
            .expect_err("unknown operation status should fail to parse");
        assert_eq!(error, "Unknown provider ticket operation status: retrying");
        assert!(ProviderTicketOperationStatus::from_str("").is_err());
        assert!(ProviderTicketOperationStatus::from_str("Pending").is_err());
    }

    #[test]
    fn provider_ticket_operation_status_is_terminal_for_all_but_pending() {
        assert!(!ProviderTicketOperationStatus::Pending.is_terminal());
        assert!(ProviderTicketOperationStatus::Succeeded.is_terminal());
        assert!(ProviderTicketOperationStatus::Failed.is_terminal());
        assert!(ProviderTicketOperationStatus::TimedOut.is_terminal());
        assert!(ProviderTicketOperationStatus::Canceled.is_terminal());
    }
}
