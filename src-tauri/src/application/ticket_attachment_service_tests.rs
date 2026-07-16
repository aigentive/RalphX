use std::sync::Arc;

use async_trait::async_trait;

use super::*;
use crate::application::clickup_integration_service::ClickUpAttachment;
use crate::application::linear_integration_service::LinearAttachment;
use crate::application::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianCredential,
    AtlassianIntegrationService, AtlassianJiraAttachment, AtlassianOAuthResource,
    AtlassianOAuthTokenResponse, AtlassianResourceContent, AtlassianResourceKind,
    AtlassianResourceSummary, ClickUpApiClient, ClickUpAuthContext, ClickUpComment,
    ClickUpIntegrationService, ClickUpTaskContent, ClickUpWorkspace, LinearApiClient,
    LinearAuthContext, LinearIntegrationService, LinearIssueContent, LinearIssueSummary,
};
use crate::domain::integrations::AtlassianAuthMethod;
use crate::domain::services::ComposerIntegrationReference;
use crate::infrastructure::memory::{
    MemoryAtlassianIntegrationSettingsRepository, MemoryClickUpIntegrationSettingsRepository,
    MemoryLinearIntegrationSettingsRepository, MemorySecretStore,
};

struct AttachmentAtlassianClient {
    content: AtlassianResourceContent,
}

#[async_trait]
impl AtlassianApiClient for AttachmentAtlassianClient {
    async fn validate(&self, auth: &AtlassianAuthContext) -> Result<AtlassianConnectivity, String> {
        assert_eq!(auth.site_url, "https://jira.test");
        assert!(matches!(
            auth.credential,
            AtlassianCredential::ApiToken { .. }
        ));
        Ok(AtlassianConnectivity {
            jira_available: true,
            confluence_available: false,
        })
    }

    async fn search(
        &self,
        _auth: &AtlassianAuthContext,
        _kind: AtlassianResourceKind,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, String> {
        Ok(Vec::new())
    }

    async fn fetch(
        &self,
        _auth: &AtlassianAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<AtlassianResourceContent, String> {
        assert_eq!(reference.provider, "atlassian");
        assert_eq!(reference.kind, "jira");
        Ok(self.content.clone())
    }

    async fn assign_jira_issue_to_current_user(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn exchange_oauth_code(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _code: &str,
        _redirect_uri: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        Err("not used".to_string())
    }

    async fn refresh_oauth_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _refresh_token: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        Err("not used".to_string())
    }

    async fn oauth_accessible_resources(
        &self,
        _access_token: &str,
    ) -> Result<Vec<AtlassianOAuthResource>, String> {
        Ok(Vec::new())
    }
}

struct AttachmentLinearClient {
    content: LinearIssueContent,
}

#[async_trait]
impl LinearApiClient for AttachmentLinearClient {
    async fn validate(&self, _auth: &LinearAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn search_issues(
        &self,
        _auth: &LinearAuthContext,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<LinearIssueSummary>, String> {
        Ok(Vec::new())
    }

    async fn fetch_issue(
        &self,
        _auth: &LinearAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<LinearIssueContent, String> {
        assert_eq!(reference.provider, "linear");
        assert_eq!(reference.kind, "linear");
        Ok(self.content.clone())
    }
}

struct AttachmentClickUpClient {
    task: ClickUpTaskContent,
}

#[async_trait]
impl ClickUpApiClient for AttachmentClickUpClient {
    async fn validate(&self, _auth: &ClickUpAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_workspaces(
        &self,
        _auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        Ok(vec![ClickUpWorkspace {
            id: "team-1".to_string(),
            name: "Team One".to_string(),
            color: None,
        }])
    }

    async fn fetch_task(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        assert_eq!(task_id, self.task.id.as_str());
        Ok(self.task.clone())
    }
}

#[tokio::test]
async fn jira_attachment_metadata_redacts_urls_and_marks_retrievable() {
    let atlassian = enabled_atlassian_service(Arc::new(AttachmentAtlassianClient {
        content: jira_content(vec![AtlassianJiraAttachment {
            id: Some("att-1".to_string()),
            filename: "diagram https://files.example/design.png?token=secret-token".to_string(),
            mime_type: Some("image/png".to_string()),
            size: Some(1234),
            author: Some("A. User".to_string()),
            content_url: Some("https://files.example/download?token=secret-token".to_string()),
            thumbnail_url: Some("https://files.example/thumb".to_string()),
            created_at: Some("2026-07-01T10:00:00Z".to_string()),
        }]),
    }))
    .await;
    let service = TicketAttachmentService::with_optional_services(Some(atlassian), None, None);

    let result = service
        .list_ticket_attachments(jira_ticket())
        .await
        .unwrap();

    assert!(!result.truncated);
    assert_eq!(result.ticket.id, "JRA-1");
    assert_eq!(result.attachments.len(), 1);
    let attachment = &result.attachments[0];
    assert_eq!(attachment.provider, "jira");
    assert_eq!(attachment.display_name, "diagram [redacted_url]");
    assert_eq!(attachment.mime_type.as_deref(), Some("image/png"));
    assert_eq!(attachment.size_bytes, Some(1234));
    assert_eq!(attachment.author_name.as_deref(), Some("A. User"));
    assert!(attachment.retrievable);

    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("https://"));
    assert!(!serialized.contains("secret-token"));
    assert!(!serialized.contains("contentUrl"));
    assert!(!serialized.contains("thumbnailUrl"));
}

#[tokio::test]
async fn linear_attachment_metadata_fetches_issue_and_redacts_url_fields() {
    let linear = enabled_linear_service(Arc::new(AttachmentLinearClient {
        content: linear_content(vec![LinearAttachment {
            id: "lin-att-1".to_string(),
            title: "Spec https://linear.example/attachment?access_token=abc".to_string(),
            subtitle: Some(
                "uploaded Bearer linear-secret from https://linear.example/source".to_string(),
            ),
            url: "https://linear.example/download?access_token=abc".to_string(),
        }]),
    }))
    .await;
    let service = TicketAttachmentService::with_optional_services(None, Some(linear), None);

    let result = service
        .list_ticket_attachments(linear_ticket())
        .await
        .unwrap();

    assert_eq!(result.attachments.len(), 1);
    let attachment = &result.attachments[0];
    assert_eq!(attachment.provider, "linear");
    assert_eq!(attachment.ticket_id, "issue-1");
    assert_eq!(attachment.ticket_key.as_deref(), Some("LIN-1"));
    assert_eq!(attachment.display_name, "Spec [redacted_url]");
    assert_eq!(
        attachment.author_name.as_deref(),
        Some("uploaded [redacted_secret] [redacted_secret] from [redacted_url]")
    );
    assert!(attachment.retrievable);

    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("https://"));
    assert!(!serialized.contains("access_token"));
    assert!(!serialized.contains("linear-secret"));
}

#[tokio::test]
async fn clickup_attachment_metadata_includes_task_and_comment_sources() {
    let clickup = enabled_clickup_service(Arc::new(AttachmentClickUpClient {
        task: ClickUpTaskContent {
            id: "task-abc".to_string(),
            custom_id: None,
            name: "Task ABC".to_string(),
            url: Some("https://clickup.example/task/task-abc".to_string()),
            description: String::new(),
            status_name: None,
            status_type: None,
            status_category: None,
            creator: None,
            assignees: Vec::new(),
            watchers: Vec::new(),
            tags: Vec::new(),
            comments: vec![ClickUpComment {
                id: "comment-1".to_string(),
                body: "comment".to_string(),
                author_id: Some(1),
                author_name: Some("Commenter".to_string()),
                created_at: Some("2026-07-01T10:00:00Z".to_string()),
                attachments: vec![ClickUpAttachment {
                    id: Some("comment-att".to_string()),
                    filename: "comment-log.txt".to_string(),
                    mime_type: Some("text/plain".to_string()),
                    size: Some(42),
                    url: Some("https://clickup.example/comment-att".to_string()),
                }],
                replies: Vec::new(),
            }],
            attachments: vec![ClickUpAttachment {
                id: Some("task-att".to_string()),
                filename: "task-log.txt".to_string(),
                mime_type: Some("text/plain".to_string()),
                size: Some(100),
                url: Some("https://clickup.example/task-att".to_string()),
            }],
            updated_at: None,
            space_id: Some("space-1".to_string()),
            list_name: None,
        },
    }))
    .await;
    let service = TicketAttachmentService::with_optional_services(None, None, Some(clickup));

    let result = service
        .list_ticket_attachments(clickup_ticket())
        .await
        .unwrap();

    assert_eq!(result.attachments.len(), 2);
    assert_eq!(
        result.attachments[0].source.kind,
        TicketAttachmentSourceKind::Ticket
    );
    assert_eq!(result.attachments[0].source.id, None);
    assert_eq!(
        result.attachments[1].source.kind,
        TicketAttachmentSourceKind::Comment
    );
    assert_eq!(
        result.attachments[1].source.id.as_deref(),
        Some("comment-1")
    );
    assert!(result
        .attachments
        .iter()
        .all(|attachment| attachment.retrievable));

    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("https://"));
}

#[tokio::test]
async fn unsupported_provider_and_missing_integration_fail_closed() {
    let service = TicketAttachmentService::with_optional_services(None, None, None);

    let unsupported = service
        .list_ticket_attachments(TicketingTicketIdentity {
            provider: "asana".to_string(),
            id: "task-1".to_string(),
            key: None,
            local_project_id: None,
        })
        .await
        .unwrap_err();
    assert!(unsupported.contains("Unknown ticketing provider"));

    let missing = service
        .list_ticket_attachments(jira_ticket())
        .await
        .unwrap_err();
    assert_eq!(missing, "Jira integration service is unavailable");
}

#[tokio::test]
async fn bounded_output_caps_count_and_field_lengths() {
    let long_name = "x".repeat(MAX_TICKET_ATTACHMENT_TEXT_CHARS + 50);
    let attachments = (0..(MAX_TICKET_ATTACHMENTS + 5))
        .map(|index| AtlassianJiraAttachment {
            id: Some(format!("att-{index}")),
            filename: long_name.clone(),
            mime_type: Some("application/octet-stream".to_string()),
            size: Some(index as i64),
            author: None,
            content_url: Some(format!("https://files.example/{index}")),
            thumbnail_url: None,
            created_at: None,
        })
        .collect();
    let atlassian = enabled_atlassian_service(Arc::new(AttachmentAtlassianClient {
        content: jira_content(attachments),
    }))
    .await;
    let service = TicketAttachmentService::with_optional_services(Some(atlassian), None, None);

    let result = service
        .list_ticket_attachments(jira_ticket())
        .await
        .unwrap();

    assert!(result.truncated);
    assert_eq!(result.attachments.len(), MAX_TICKET_ATTACHMENTS);
    let display_len = result.attachments[0].display_name.chars().count();
    assert_eq!(display_len, MAX_TICKET_ATTACHMENT_TEXT_CHARS);
    assert!(result.attachments[0].display_name.ends_with("..."));
}

#[tokio::test]
async fn disabled_provider_error_is_not_empty_success() {
    let linear = Arc::new(LinearIntegrationService::new(
        Arc::new(MemoryLinearIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        Arc::new(AttachmentLinearClient {
            content: linear_content(Vec::new()),
        }),
    ));
    let service = TicketAttachmentService::with_optional_services(None, Some(linear), None);

    let error = service
        .list_ticket_attachments(linear_ticket())
        .await
        .unwrap_err();

    assert_eq!(error, "Linear integration is not enabled");
}

async fn enabled_atlassian_service(
    client: Arc<dyn AtlassianApiClient>,
) -> Arc<AtlassianIntegrationService> {
    let service = Arc::new(AtlassianIntegrationService::new(
        Arc::new(MemoryAtlassianIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ));
    service
        .save_settings(
            Some(AtlassianAuthMethod::ApiToken),
            Some("jira.test".to_string()),
            Some("agent@example.com".to_string()),
            Some("jira-token".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("atlassian settings should save");
    service
        .validate_and_enable()
        .await
        .expect("atlassian should validate");
    service
}

async fn enabled_linear_service(client: Arc<dyn LinearApiClient>) -> Arc<LinearIntegrationService> {
    let service = Arc::new(LinearIntegrationService::new(
        Arc::new(MemoryLinearIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ));
    service
        .save_settings(Some("lin-token".to_string()))
        .await
        .expect("linear settings should save");
    service
        .validate_and_enable()
        .await
        .expect("linear should validate");
    service
}

async fn enabled_clickup_service(
    client: Arc<dyn ClickUpApiClient>,
) -> Arc<ClickUpIntegrationService> {
    let service = Arc::new(ClickUpIntegrationService::new(
        Arc::new(MemoryClickUpIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ));
    service
        .save_settings(Some("clk-token".to_string()), Some("team-1".to_string()))
        .await
        .expect("clickup settings should save");
    service
        .validate_and_enable()
        .await
        .expect("clickup should validate");
    service
}

fn jira_ticket() -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: "jira".to_string(),
        id: "10001".to_string(),
        key: Some("JRA-1".to_string()),
        local_project_id: Some("project-1".to_string()),
    }
}

fn linear_ticket() -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: "linear".to_string(),
        id: "issue-1".to_string(),
        key: Some("LIN-1".to_string()),
        local_project_id: None,
    }
}

fn clickup_ticket() -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: "clickup".to_string(),
        id: "task-abc".to_string(),
        key: None,
        local_project_id: None,
    }
}

fn jira_content(attachments: Vec<AtlassianJiraAttachment>) -> AtlassianResourceContent {
    AtlassianResourceContent {
        kind: AtlassianResourceKind::Jira,
        id: "10001".to_string(),
        key: Some("JRA-1".to_string()),
        title: "Jira ticket".to_string(),
        url: Some("https://jira.test/browse/JRA-1".to_string()),
        body: String::new(),
        status: None,
        assignee: None,
        reporter: None,
        updated_at_remote: None,
        description_markdown: None,
        description_text: None,
        acceptance_criteria_markdown: None,
        acceptance_criteria_text: None,
        comments: Vec::new(),
        attachments,
    }
}

fn linear_content(attachments: Vec<LinearAttachment>) -> LinearIssueContent {
    LinearIssueContent {
        id: "issue-1".to_string(),
        key: Some("LIN-1".to_string()),
        title: "Linear issue".to_string(),
        url: Some("https://linear.example/issue/LIN-1".to_string()),
        body: String::new(),
        state_name: None,
        assignee: None,
        creator: None,
        updated_at: None,
        comments: Vec::new(),
        attachments,
        labels: Vec::new(),
        project: None,
    }
}
