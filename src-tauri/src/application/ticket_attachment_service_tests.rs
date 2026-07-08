use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use crate::application::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianIntegrationService,
    AtlassianJiraAttachment, AtlassianOAuthResource, AtlassianOAuthTokenResponse,
    AtlassianResourceContent, AtlassianResourceKind, AtlassianResourceSummary, ClickUpApiClient,
    ClickUpAttachment, ClickUpAuthContext, ClickUpComment, ClickUpIntegrationService,
    ClickUpTaskContent, ClickUpWorkspace, LinearApiClient, LinearAttachment, LinearAuthContext,
    LinearIntegrationService, LinearIssueContent, LinearIssueSummary, TicketAttachmentFetchRequest,
    TicketAttachmentFetchStatus, TicketAttachmentListRequest, TicketAttachmentProviderBytes,
    TicketAttachmentRetrievalKind, TicketAttachmentService, TicketAttachmentSourceKind,
    TicketingTicketIdentity,
};
use crate::domain::integrations::AtlassianAuthMethod;
use crate::domain::services::ComposerIntegrationReference;
use crate::infrastructure::memory::{
    MemoryAtlassianIntegrationSettingsRepository, MemoryClickUpIntegrationSettingsRepository,
    MemoryLinearIntegrationSettingsRepository, MemorySecretStore,
};

#[derive(Clone)]
struct AttachmentFixture {
    jira_content: AtlassianResourceContent,
    linear_content: LinearIssueContent,
    clickup_content: ClickUpTaskContent,
    downloads: HashMap<String, TicketAttachmentProviderBytes>,
}

struct ServiceFixture {
    service: TicketAttachmentService,
    _cache_dir: tempfile::TempDir,
}

#[async_trait]
impl AtlassianApiClient for AttachmentFixture {
    async fn validate(
        &self,
        _auth: &AtlassianAuthContext,
    ) -> Result<AtlassianConnectivity, String> {
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
        _reference: &ComposerIntegrationReference,
    ) -> Result<AtlassianResourceContent, String> {
        Ok(self.jira_content.clone())
    }

    async fn fetch_jira_attachment_bytes(
        &self,
        _auth: &AtlassianAuthContext,
        content_url: &str,
        _max_bytes: usize,
    ) -> Result<TicketAttachmentProviderBytes, String> {
        self.downloads
            .get(content_url)
            .cloned()
            .ok_or_else(|| "Jira attachment bytes missing".to_string())
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
        Ok(AtlassianOAuthTokenResponse {
            access_token: "access".to_string(),
            refresh_token: None,
            expires_in: None,
            scope: None,
        })
    }

    async fn refresh_oauth_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _refresh_token: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        self.exchange_oauth_code("", "", "", "").await
    }

    async fn oauth_accessible_resources(
        &self,
        _access_token: &str,
    ) -> Result<Vec<AtlassianOAuthResource>, String> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl LinearApiClient for AttachmentFixture {
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
        _reference: &ComposerIntegrationReference,
    ) -> Result<LinearIssueContent, String> {
        Ok(self.linear_content.clone())
    }
}

#[async_trait]
impl ClickUpApiClient for AttachmentFixture {
    async fn validate(&self, _auth: &ClickUpAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_workspaces(
        &self,
        _auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        Ok(vec![ClickUpWorkspace {
            id: "team-1".to_string(),
            name: "Team".to_string(),
            color: None,
        }])
    }

    async fn fetch_task(
        &self,
        _auth: &ClickUpAuthContext,
        _task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        Ok(self.clickup_content.clone())
    }

    async fn fetch_attachment_bytes(
        &self,
        _auth: &ClickUpAuthContext,
        url: &str,
        _max_bytes: usize,
    ) -> Result<TicketAttachmentProviderBytes, String> {
        self.downloads
            .get(url)
            .cloned()
            .ok_or_else(|| "ClickUp attachment bytes missing".to_string())
    }
}

#[tokio::test]
async fn list_attachments_normalizes_provider_metadata_and_sources() {
    let fixture = fixture_service().await;
    let service = &fixture.service;

    let jira = service
        .list_attachments(TicketAttachmentListRequest {
            ticket: ticket("jira", "10001", Some("JRA-42"), Some("local-project")),
        })
        .await
        .expect("jira list should succeed");
    assert_eq!(jira.attachments.len(), 1);
    let jira_attachment = &jira.attachments[0];
    assert_eq!(jira_attachment.provider, "jira");
    assert_eq!(jira_attachment.ticket.id, "10001");
    assert_eq!(jira_attachment.ticket.key.as_deref(), Some("JRA-42"));
    assert_eq!(jira_attachment.id, "jira-a1");
    assert_eq!(jira_attachment.name, "notes.txt");
    assert_eq!(jira_attachment.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(jira_attachment.size, Some(18));
    assert_eq!(jira_attachment.author_name.as_deref(), Some("Designer"));
    assert_eq!(jira_attachment.created_at.as_deref(), Some("2026-04-01T10:00:00Z"));
    assert_eq!(jira_attachment.source.kind, TicketAttachmentSourceKind::TopLevel);
    assert_eq!(jira_attachment.retrieval_kind, TicketAttachmentRetrievalKind::Download);
    assert!(jira_attachment.retrievable);
    assert_eq!(jira_attachment.unsupported_reason, None);

    let linear = service
        .list_attachments(TicketAttachmentListRequest {
            ticket: ticket("linear", "lin-issue-1", Some("LIN-1"), None),
        })
        .await
        .expect("linear list should succeed");
    assert_eq!(linear.attachments.len(), 1);
    let linear_attachment = &linear.attachments[0];
    assert_eq!(linear_attachment.provider, "linear");
    assert_eq!(linear_attachment.id, "linear-a1");
    assert_eq!(linear_attachment.name, "Design spec");
    assert_eq!(
        linear_attachment.retrieval_kind,
        TicketAttachmentRetrievalKind::ExternalLink
    );
    assert!(linear_attachment.retrievable);

    let clickup = service
        .list_attachments(TicketAttachmentListRequest {
            ticket: ticket("clickup", "task-abc", None, Some("local-project")),
        })
        .await
        .expect("clickup list should succeed");
    assert_eq!(clickup.attachments.len(), 2);
    assert_eq!(clickup.attachments[0].source.kind, TicketAttachmentSourceKind::TopLevel);
    assert_eq!(clickup.attachments[1].source.kind, TicketAttachmentSourceKind::Comment);
    assert_eq!(
        clickup.attachments[1].source.comment_id.as_deref(),
        Some("comment-1")
    );
}

#[tokio::test]
async fn fetch_attachment_returns_inline_text_external_link_and_cached_binary_results() {
    let fixture = fixture_service().await;
    let service = &fixture.service;

    let jira = service
        .fetch_attachment(TicketAttachmentFetchRequest {
            ticket: ticket("jira", "10001", Some("JRA-42"), None),
            attachment_id: "jira-a1".to_string(),
        })
        .await
        .expect("jira fetch should succeed");
    assert_eq!(jira.result.status, TicketAttachmentFetchStatus::InlineText);
    assert_eq!(jira.result.inline_text.as_deref(), Some("Jira text attachment"));
    assert_eq!(jira.result.size, Some(20));
    assert_eq!(jira.result.mime_type.as_deref(), Some("text/plain"));
    assert!(jira.result.sha256.is_some());
    assert_eq!(jira.result.cached_file, None);
    assert_eq!(jira.result.external_link, None);

    let linear = service
        .fetch_attachment(TicketAttachmentFetchRequest {
            ticket: ticket("linear", "lin-issue-1", Some("LIN-1"), None),
            attachment_id: "linear-a1".to_string(),
        })
        .await
        .expect("linear fetch should succeed");
    assert_eq!(linear.result.status, TicketAttachmentFetchStatus::ExternalLink);
    assert_eq!(
        linear
            .result
            .external_link
            .as_ref()
            .map(|link| link.url.as_str()),
        Some("https://linear.app/acme/attachment/linear-a1")
    );
    assert_eq!(linear.result.cached_file, None);
    assert_eq!(linear.result.inline_text, None);

    let clickup = service
        .fetch_attachment(TicketAttachmentFetchRequest {
            ticket: ticket("clickup", "task-abc", None, None),
            attachment_id: "clickup-binary".to_string(),
        })
        .await
        .expect("clickup fetch should succeed");
    assert_eq!(clickup.result.status, TicketAttachmentFetchStatus::CachedFile);
    let cached = clickup
        .result
        .cached_file
        .as_ref()
        .expect("binary should be cached");
    let path = PathBuf::from(&cached.path);
    assert!(path.exists(), "cached file should exist at {}", path.display());
    assert_eq!(std::fs::read(&path).expect("read cached bytes"), vec![0, 1, 2, 3]);
    assert!(!cached.path.contains("../ticket"));
    assert!(!cached.path.contains("task-abc"));
    assert!(!cached.path.contains("diagram.png"));
    assert_eq!(cached.size, 4);
    assert_eq!(cached.mime_type.as_deref(), Some("image/png"));
}

#[tokio::test]
async fn fetch_attachment_withholds_token_bearing_external_links() {
    let fixture = fixture_service_with_linear_url(
        "https://linear.app/acme/attachment/linear-a1?access_token=secret-value",
    )
    .await;
    let service = &fixture.service;

    let response = service
        .fetch_attachment(TicketAttachmentFetchRequest {
            ticket: ticket("linear", "lin-issue-1", Some("LIN-1"), None),
            attachment_id: "linear-a1".to_string(),
        })
        .await
        .expect("linear fetch should stay bounded");

    assert_eq!(
        response.result.status,
        TicketAttachmentFetchStatus::Unsupported
    );
    assert_eq!(response.result.external_link, None);
    assert_eq!(
        response.result.unsupported_reason.as_deref(),
        Some(
            "Attachment external link was withheld because it appears to contain credentials or bearer access material"
        )
    );
}

#[tokio::test]
async fn unsupported_provider_returns_clear_reason_without_fetching() {
    let fixture = fixture_service().await;
    let service = &fixture.service;

    let response = service
        .list_attachments(TicketAttachmentListRequest {
            ticket: ticket("github", "issue-1", None, None),
        })
        .await
        .expect("unsupported provider should be a bounded response");

    assert!(response.attachments.is_empty());
    assert_eq!(
        response.unsupported_reason.as_deref(),
        Some("Unsupported ticket attachment provider: github")
    );
}

async fn fixture_service() -> ServiceFixture {
    fixture_service_with_linear_url("https://linear.app/acme/attachment/linear-a1").await
}

async fn fixture_service_with_linear_url(linear_attachment_url: &str) -> ServiceFixture {
    let fixture = Arc::new(AttachmentFixture {
        jira_content: AtlassianResourceContent {
            kind: AtlassianResourceKind::Jira,
            id: "10001".to_string(),
            key: Some("JRA-42".to_string()),
            title: "Jira issue".to_string(),
            url: Some("https://jira.test/browse/JRA-42".to_string()),
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
            attachments: vec![AtlassianJiraAttachment {
                id: Some("jira-a1".to_string()),
                filename: "notes.txt".to_string(),
                mime_type: Some("text/plain".to_string()),
                size: Some(18),
                author: Some("Designer".to_string()),
                content_url: Some("https://jira.test/secure/attachment/a1/notes.txt".to_string()),
                thumbnail_url: None,
                created_at: Some("2026-04-01T10:00:00Z".to_string()),
            }],
        },
        linear_content: LinearIssueContent {
            id: "lin-issue-1".to_string(),
            key: Some("LIN-1".to_string()),
            title: "Linear issue".to_string(),
            url: Some("https://linear.app/acme/issue/LIN-1".to_string()),
            body: String::new(),
            state_name: None,
            assignee: None,
            creator: None,
            updated_at: None,
            comments: Vec::new(),
            attachments: vec![LinearAttachment {
                id: "linear-a1".to_string(),
                title: "Design spec".to_string(),
                subtitle: Some("External link".to_string()),
                url: linear_attachment_url.to_string(),
            }],
            labels: Vec::new(),
            project: Some("Platform".to_string()),
        },
        clickup_content: ClickUpTaskContent {
            id: "task-abc".to_string(),
            custom_id: None,
            name: "ClickUp task".to_string(),
            url: Some("https://app.clickup.com/t/task-abc".to_string()),
            description: String::new(),
            status_name: None,
            status_type: None,
            status_category: None,
            creator: Some("Reporter".to_string()),
            assignees: Vec::new(),
            watchers: Vec::new(),
            tags: Vec::new(),
            comments: vec![ClickUpComment {
                id: "comment-1".to_string(),
                body: "See attachment".to_string(),
                author_id: Some(9),
                author_name: Some("Reviewer".to_string()),
                created_at: Some("2026-04-02T11:00:00Z".to_string()),
                attachments: vec![ClickUpAttachment {
                    id: Some("clickup-comment".to_string()),
                    filename: "comment-log.txt".to_string(),
                    mime_type: Some("text/plain".to_string()),
                    size: Some(12),
                    url: Some("https://clickup.test/comment-log.txt".to_string()),
                }],
                replies: Vec::new(),
            }],
            attachments: vec![ClickUpAttachment {
                id: Some("clickup-binary".to_string()),
                filename: "../ticket/diagram.png".to_string(),
                mime_type: Some("image/png".to_string()),
                size: Some(4),
                url: Some("https://clickup.test/diagram.png".to_string()),
            }],
            updated_at: None,
            space_id: Some("space-1".to_string()),
            list_name: Some("List".to_string()),
        },
        downloads: HashMap::from([
            (
                "https://jira.test/secure/attachment/a1/notes.txt".to_string(),
                TicketAttachmentProviderBytes {
                    bytes: b"Jira text attachment".to_vec(),
                    mime_type: Some("text/plain".to_string()),
                },
            ),
            (
                "https://clickup.test/diagram.png".to_string(),
                TicketAttachmentProviderBytes {
                    bytes: vec![0, 1, 2, 3],
                    mime_type: Some("image/png".to_string()),
                },
            ),
        ]),
    });

    let atlassian = Arc::new(AtlassianIntegrationService::new(
        Arc::new(MemoryAtlassianIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        fixture.clone(),
    ));
    atlassian
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
    atlassian
        .validate_and_enable()
        .await
        .expect("atlassian should validate");

    let linear = Arc::new(LinearIntegrationService::new(
        Arc::new(MemoryLinearIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        fixture.clone(),
    ));
    linear
        .save_settings(Some("linear-token".to_string()))
        .await
        .expect("linear settings should save");
    linear
        .validate_and_enable()
        .await
        .expect("linear should validate");

    let clickup = Arc::new(ClickUpIntegrationService::new(
        Arc::new(MemoryClickUpIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        fixture,
    ));
    clickup
        .save_settings(Some("clickup-token".to_string()), Some("team-1".to_string()))
        .await
        .expect("clickup settings should save");
    clickup
        .validate_and_enable()
        .await
        .expect("clickup should validate");

    let cache_dir = tempfile::Builder::new()
        .prefix("ticket-attachment-cache-")
        .tempdir_in(std::env::current_dir().expect("current dir"))
        .expect("tempdir under workspace");

    ServiceFixture {
        service: TicketAttachmentService::new(atlassian, linear, clickup, cache_dir.path()),
        _cache_dir: cache_dir,
    }
}

fn ticket(
    provider: &str,
    id: &str,
    key: Option<&str>,
    local_project_id: Option<&str>,
) -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: provider.to_string(),
        id: id.to_string(),
        key: key.map(str::to_string),
        local_project_id: local_project_id.map(str::to_string),
    }
}
