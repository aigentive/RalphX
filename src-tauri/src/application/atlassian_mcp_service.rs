//! Atlassian MCP tool operations on [`AtlassianIntegrationService`].
//!
//! These live beside the service rather than inside it so the shipped service
//! file does not keep growing. Every method resolves credentials through the
//! service's own `enabled_auth_context`, so the enablement gate, API-token vs
//! OAuth selection, and OAuth refresh are reused unchanged.
//!
//! Unlike the older operations, these return [`AtlassianApiError`] so callers
//! classify failures on the numeric HTTP status rather than on message text.

use serde_json::Value;

use super::atlassian_integration_service::{AtlassianIntegrationService, AtlassianResourceKind};
use crate::domain::integrations::atlassian_api_error::AtlassianApiError;
use crate::domain::integrations::atlassian_mcp_ops::{
    AtlassianRawMethod, ConfluencePageContent, ConfluencePageCreateRequest,
    ConfluencePageUpdateRequest, JiraIssueCreateRequest, JiraIssueCreated, JiraIssueUpdateRequest,
};

impl AtlassianIntegrationService {
    async fn mcp_auth(&self) -> Result<super::AtlassianAuthContext, AtlassianApiError> {
        self.enabled_auth_context()
            .await
            .map_err(AtlassianApiError::transport)
    }

    /// Create a Jira issue.
    ///
    /// # Errors
    ///
    /// Returns an error when the integration is unusable or Atlassian rejects
    /// the request.
    pub async fn create_jira_issue(
        &self,
        request: &JiraIssueCreateRequest,
    ) -> Result<JiraIssueCreated, AtlassianApiError> {
        let auth = self.mcp_auth().await?;
        self.client().create_jira_issue(&auth, request).await
    }

    /// Update a bounded subset of Jira issue fields.
    ///
    /// # Errors
    ///
    /// Returns an error when the integration is unusable or Atlassian rejects
    /// the request.
    pub async fn update_jira_issue(
        &self,
        issue_key: &str,
        request: &JiraIssueUpdateRequest,
    ) -> Result<(), AtlassianApiError> {
        let auth = self.mcp_auth().await?;
        self.client()
            .update_jira_issue(&auth, issue_key, request)
            .await
    }

    /// Read a Confluence page including its storage-format body.
    ///
    /// # Errors
    ///
    /// Returns an error when the integration is unusable or Atlassian rejects
    /// the request.
    pub async fn confluence_get_page(
        &self,
        page_id: &str,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        let auth = self.mcp_auth().await?;
        self.client().confluence_get_page(&auth, page_id).await
    }

    /// Create a Confluence page.
    ///
    /// # Errors
    ///
    /// Returns an error when the integration is unusable or Atlassian rejects
    /// the request.
    pub async fn confluence_create_page(
        &self,
        request: &ConfluencePageCreateRequest,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        let auth = self.mcp_auth().await?;
        self.client().confluence_create_page(&auth, request).await
    }

    /// Update a Confluence page, incrementing its version.
    ///
    /// # Errors
    ///
    /// Returns an error when the integration is unusable or Atlassian rejects
    /// the request.
    pub async fn confluence_update_page(
        &self,
        page_id: &str,
        request: &ConfluencePageUpdateRequest,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        let auth = self.mcp_auth().await?;
        self.client()
            .confluence_update_page(&auth, page_id, request)
            .await
    }

    /// Issue a bounded generic Atlassian API request.
    ///
    /// Path containment is enforced again inside the client at the request sink.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is rejected, the integration is unusable,
    /// or Atlassian rejects the request.
    pub async fn atlassian_raw_api_request(
        &self,
        method: AtlassianRawMethod,
        kind: AtlassianResourceKind,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, AtlassianApiError> {
        // Reject unsafe paths before touching credentials so a containment
        // failure can never be masked by a credential failure.
        crate::domain::integrations::atlassian_mcp_ops::validate_atlassian_raw_path(path)
            .map_err(AtlassianApiError::transport)?;
        let auth = self.mcp_auth().await?;
        self.client()
            .raw_api_request(&auth, method, kind, path, body)
            .await
    }
}
