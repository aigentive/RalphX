use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::{Mutex, RwLock};

use super::*;
use crate::domain::integrations::IntegrationValidationStatus;
use crate::domain::services::SecretStore;
use crate::infrastructure::memory::MemorySecretStore;

/// The secret-store key the service uses for the default API token. Mirrors the
/// private `ATLASSIAN_TOKEN_SECRET_REF` constant in the service module so the
/// seeded settings point at a real secret.
const TEST_TOKEN_REF: &str = "integrations/atlassian/default/api-token";

#[test]
fn normalizes_site_url_to_https() {
    assert_eq!(
        normalize_site_url("example.atlassian.net/").unwrap(),
        "https://example.atlassian.net"
    );
    assert!(normalize_site_url("http://example.atlassian.net").is_err());
}

#[test]
fn normalizes_loopback_oauth_redirect_uri() {
    assert_eq!(
        normalize_oauth_redirect_uri("http://LOCALHOST:8765/atlassian/oauth/callback/").unwrap(),
        "http://localhost:8765/atlassian/oauth/callback"
    );
    assert_eq!(
        normalize_oauth_redirect_uri("http://127.12.0.1:8765/callback").unwrap(),
        "http://127.12.0.1:8765/callback"
    );
}

#[test]
fn rejects_non_loopback_oauth_redirect_uri() {
    assert!(normalize_oauth_redirect_uri("https://127.0.0.1:8765/callback").is_err());
    assert!(normalize_oauth_redirect_uri("http://example.com:8765/callback").is_err());
    assert!(normalize_oauth_redirect_uri("http://127.0.0.1/callback").is_err());
}

#[test]
fn oauth_callback_result_requires_matching_state() {
    let mut params = HashMap::new();
    params.insert("state".to_string(), "expected".to_string());
    params.insert("code".to_string(), "auth-code".to_string());

    assert_eq!(
        oauth_callback_result(&params, "expected").unwrap(),
        "auth-code"
    );
    assert!(oauth_callback_result(&params, "other").is_err());
}

// ---- Enabled-context service routing harness --------------------------------

/// In-memory [`AtlassianIntegrationSettingsRepository`] mirroring the Linear
/// sibling's `TestSettingsRepo`.
struct TestSettingsRepo {
    settings: RwLock<AtlassianIntegrationSettings>,
}

impl TestSettingsRepo {
    fn enabled() -> Self {
        Self {
            settings: RwLock::new(enabled_api_token_settings()),
        }
    }

    fn disabled() -> Self {
        Self {
            settings: RwLock::new(AtlassianIntegrationSettings::default()),
        }
    }
}

#[async_trait]
impl AtlassianIntegrationSettingsRepository for TestSettingsRepo {
    async fn get(&self) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>> {
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &AtlassianIntegrationSettings,
    ) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>> {
        *self.settings.write().await = settings.clone();
        Ok(settings.clone())
    }
}

/// Settings that satisfy [`enabled_auth_context`]: enabled, validated, API-token
/// auth with a site URL, email, and a token secret reference.
fn enabled_api_token_settings() -> AtlassianIntegrationSettings {
    AtlassianIntegrationSettings {
        enabled: true,
        auth_method: AtlassianAuthMethod::ApiToken,
        site_url: Some("https://example.atlassian.net".to_string()),
        email: Some("user@example.com".to_string()),
        token_secret_ref: Some(TEST_TOKEN_REF.to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        jira_available: true,
        confluence_available: true,
        last_validated_at: Some(Utc::now()),
        updated_at: Utc::now(),
        ..AtlassianIntegrationSettings::default()
    }
}

#[derive(Debug, Default, Clone)]
struct RecordedJiraCalls {
    cleared_assignees: Vec<String>,
    listed_transitions: Vec<String>,
    transitioned: Vec<(String, String)>,
    comments: Vec<(String, String)>,
    label_writes: Vec<(String, Vec<String>)>,
}

/// Fake [`AtlassianApiClient`] that records the Jira write/read calls routed
/// through the service and returns canned, configurable results. It also asserts
/// the auth context produced by the enabled settings is what reaches the client.
#[derive(Default)]
struct TestAtlassianClient {
    calls: Mutex<RecordedJiraCalls>,
    transitions: Mutex<Vec<AtlassianJiraTransition>>,
    error: Mutex<Option<String>>,
    /// Recorded (kind, query, limit) tuples for `search`.
    searches: Mutex<Vec<(AtlassianResourceKind, String, usize)>>,
    /// Recorded `assign_jira_issue_to_current_user` issue keys.
    assigned: Mutex<Vec<String>>,
    /// Recorded `list_jira_projects` limits.
    list_projects_limits: Mutex<Vec<usize>>,
    /// Recorded `list_jira_project_statuses` project keys.
    list_statuses_keys: Mutex<Vec<String>>,
    /// Recorded `(project_key, limit)` tuples for `list_jira_project_issues`.
    list_issues_calls: Mutex<Vec<(String, usize)>>,
    /// Recorded composer references fetched through `fetch_resource_content` /
    /// URL resolution.
    fetches: Mutex<Vec<ComposerIntegrationReference>>,
    /// Optional fetched-resource body override (drives render/truncation paths).
    fetch_body: Mutex<Option<String>>,
    /// Optional canned OAuth token response for `exchange_oauth_code` /
    /// `refresh_oauth_token`.
    oauth_token: Mutex<Option<AtlassianOAuthTokenResponse>>,
    /// Optional canned accessible resources for `oauth_accessible_resources`.
    oauth_resources: Mutex<Vec<AtlassianOAuthResource>>,
}

impl TestAtlassianClient {
    fn with_transitions(transitions: Vec<AtlassianJiraTransition>) -> Self {
        Self {
            transitions: Mutex::new(transitions),
            ..Self::default()
        }
    }

    async fn assert_api_token_auth(&self, auth: &AtlassianAuthContext) {
        assert_eq!(auth.site_url, "https://example.atlassian.net");
        match &auth.credential {
            AtlassianCredential::ApiToken { email, token } => {
                assert_eq!(email, "user@example.com");
                assert_eq!(token, "secret-token");
            }
            other => panic!("expected API-token credential, got {other:?}"),
        }
    }
}

#[async_trait]
impl AtlassianApiClient for TestAtlassianClient {
    async fn validate(
        &self,
        _auth: &AtlassianAuthContext,
    ) -> Result<AtlassianConnectivity, String> {
        Ok(AtlassianConnectivity {
            jira_available: true,
            confluence_available: true,
        })
    }

    async fn search(
        &self,
        auth: &AtlassianAuthContext,
        kind: AtlassianResourceKind,
        query: &str,
        limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.searches
            .lock()
            .await
            .push((kind, query.to_string(), limit));
        Ok(vec![AtlassianResourceSummary {
            kind,
            id: "10001".to_string(),
            key: Some("PROJ-1".to_string()),
            title: "Example issue".to_string(),
            url: Some("https://example.atlassian.net/browse/PROJ-1".to_string()),
            excerpt: Some("Example excerpt".to_string()),
        }])
    }

    async fn fetch(
        &self,
        auth: &AtlassianAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<AtlassianResourceContent, String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.fetches.lock().await.push(reference.clone());
        let body = self
            .fetch_body
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| "Issue body".to_string());
        let kind = reference
            .kind
            .parse::<AtlassianResourceKind>()
            .unwrap_or(AtlassianResourceKind::Jira);
        Ok(AtlassianResourceContent {
            kind,
            id: reference.id.clone(),
            key: reference.key.clone(),
            title: reference.title.clone().unwrap_or_else(|| match kind {
                AtlassianResourceKind::Jira => "Example Jira issue".to_string(),
                AtlassianResourceKind::Confluence => "Example Confluence page".to_string(),
            }),
            url: reference.url.clone(),
            body,
            status: None,
            assignee: None,
            reporter: None,
            updated_at_remote: None,
            description_markdown: None,
            description_text: None,
            acceptance_criteria_markdown: None,
            acceptance_criteria_text: None,
            comments: Vec::new(),
            attachments: Vec::new(),
        })
    }

    async fn assign_jira_issue_to_current_user(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<(), String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.assigned.lock().await.push(issue_key.to_string());
        Ok(())
    }

    async fn list_jira_projects(
        &self,
        auth: &AtlassianAuthContext,
        limit: usize,
    ) -> Result<Vec<JiraProjectSummary>, String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.list_projects_limits.lock().await.push(limit);
        Ok(vec![JiraProjectSummary {
            id: "10000".to_string(),
            key: "PROJ".to_string(),
            name: "Project".to_string(),
        }])
    }

    async fn list_jira_project_statuses(
        &self,
        auth: &AtlassianAuthContext,
        project_key: &str,
    ) -> Result<Vec<JiraStatusSummary>, String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.list_statuses_keys
            .lock()
            .await
            .push(project_key.to_string());
        Ok(vec![JiraStatusSummary {
            id: "3".to_string(),
            name: "In Progress".to_string(),
            category: "in_progress".to_string(),
        }])
    }

    async fn list_jira_project_issues(
        &self,
        auth: &AtlassianAuthContext,
        project_key: &str,
        limit: usize,
    ) -> Result<Vec<JiraIssueDetail>, String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.list_issues_calls
            .lock()
            .await
            .push((project_key.to_string(), limit));
        Ok(vec![JiraIssueDetail {
            key: "PROJ-1".to_string(),
            title: "Issue".to_string(),
            status_id: Some("3".to_string()),
            status_name: Some("In Progress".to_string()),
            status_category: Some("in_progress".to_string()),
            assignee_name: None,
            assignee_avatar: None,
            labels: Vec::new(),
            updated: None,
            priority: None,
            url: None,
        }])
    }

    async fn clear_jira_issue_assignee(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<(), String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.calls
            .lock()
            .await
            .cleared_assignees
            .push(issue_key.to_string());
        Ok(())
    }

    async fn list_jira_issue_transitions(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<Vec<AtlassianJiraTransition>, String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.calls
            .lock()
            .await
            .listed_transitions
            .push(issue_key.to_string());
        Ok(self.transitions.lock().await.clone())
    }

    async fn transition_jira_issue(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        transition_id: &str,
    ) -> Result<(), String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.calls
            .lock()
            .await
            .transitioned
            .push((issue_key.to_string(), transition_id.to_string()));
        Ok(())
    }

    async fn add_jira_comment(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        body_markdown: &str,
    ) -> Result<AtlassianJiraComment, String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.calls
            .lock()
            .await
            .comments
            .push((issue_key.to_string(), body_markdown.to_string()));
        Ok(AtlassianJiraComment {
            id: Some("10001".to_string()),
            author: Some("RalphX".to_string()),
            body_markdown: body_markdown.to_string(),
            body_text: body_markdown.to_string(),
            created_at: Some("2026-06-21T08:00:00Z".to_string()),
            updated_at: None,
        })
    }

    async fn set_jira_issue_labels(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        labels: Vec<String>,
    ) -> Result<(), String> {
        self.assert_api_token_auth(auth).await;
        if let Some(error) = self.error.lock().await.clone() {
            return Err(error);
        }
        self.calls
            .lock()
            .await
            .label_writes
            .push((issue_key.to_string(), labels));
        Ok(())
    }

    async fn exchange_oauth_code(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _code: &str,
        _redirect_uri: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        self.oauth_token
            .lock()
            .await
            .clone()
            .ok_or_else(|| "no canned oauth token".to_string())
    }

    async fn refresh_oauth_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _refresh_token: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        self.oauth_token
            .lock()
            .await
            .clone()
            .ok_or_else(|| "no canned oauth token".to_string())
    }

    async fn oauth_accessible_resources(
        &self,
        _access_token: &str,
    ) -> Result<Vec<AtlassianOAuthResource>, String> {
        Ok(self.oauth_resources.lock().await.clone())
    }
}

/// Builds a service whose settings are enabled/valid and whose secret store
/// holds the API token, so `enabled_auth_context` resolves successfully.
async fn enabled_service(client: Arc<TestAtlassianClient>) -> AtlassianIntegrationService {
    let repo = Arc::new(TestSettingsRepo::enabled());
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret(TEST_TOKEN_REF, "secret-token")
        .await
        .expect("secret store should accept token");
    AtlassianIntegrationService::new(repo, secrets, client)
}

/// Builds a service whose settings are disabled so `enabled_auth_context` fails
/// before any client call is made.
fn disabled_service(client: Arc<TestAtlassianClient>) -> AtlassianIntegrationService {
    let repo = Arc::new(TestSettingsRepo::disabled());
    let secrets = Arc::new(MemorySecretStore::new());
    AtlassianIntegrationService::new(repo, secrets, client)
}

#[tokio::test]
async fn clear_jira_issue_assignee_routes_to_client_when_enabled() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    service
        .clear_jira_issue_assignee("PROJ-1")
        .await
        .expect("clear assignee should succeed");

    assert_eq!(
        client.calls.lock().await.cleared_assignees,
        vec!["PROJ-1".to_string()]
    );
}

#[tokio::test]
async fn list_jira_issue_transitions_routes_and_returns_client_result() {
    let transitions = vec![AtlassianJiraTransition {
        provider_transition_id: "31".to_string(),
        to_state_id: "3".to_string(),
        name: "Start Progress".to_string(),
        category: "in_progress".to_string(),
    }];
    let client = Arc::new(TestAtlassianClient::with_transitions(transitions.clone()));
    let service = enabled_service(client.clone()).await;

    let returned = service
        .list_jira_issue_transitions("PROJ-1")
        .await
        .expect("transitions should be returned");

    assert_eq!(returned, transitions);
    assert_eq!(
        client.calls.lock().await.listed_transitions,
        vec!["PROJ-1".to_string()]
    );
}

#[tokio::test]
async fn transition_jira_issue_routes_to_client_when_enabled() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    service
        .transition_jira_issue("PROJ-1", "31")
        .await
        .expect("transition should succeed");

    assert_eq!(
        client.calls.lock().await.transitioned,
        vec![("PROJ-1".to_string(), "31".to_string())]
    );
}

#[tokio::test]
async fn add_jira_comment_routes_and_propagates_created_comment() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let comment = service
        .add_jira_comment("PROJ-1", "Looks good")
        .await
        .expect("comment should be created");

    assert_eq!(comment.body_markdown, "Looks good");
    assert_eq!(comment.id.as_deref(), Some("10001"));
    assert_eq!(
        client.calls.lock().await.comments,
        vec![("PROJ-1".to_string(), "Looks good".to_string())]
    );
}

#[tokio::test]
async fn set_jira_issue_labels_routes_to_client_when_enabled() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    service
        .set_jira_issue_labels("PROJ-1", vec!["backend".to_string(), "urgent".to_string()])
        .await
        .expect("label update should succeed");

    assert_eq!(
        client.calls.lock().await.label_writes,
        vec![(
            "PROJ-1".to_string(),
            vec!["backend".to_string(), "urgent".to_string()]
        )]
    );
}

#[tokio::test]
async fn jira_write_propagates_client_error() {
    let client = Arc::new(TestAtlassianClient::default());
    *client.error.lock().await = Some("Jira rejected the request".to_string());
    let service = enabled_service(client.clone()).await;

    let error = service
        .transition_jira_issue("PROJ-1", "31")
        .await
        .unwrap_err();

    assert_eq!(error, "Jira rejected the request");
}

#[tokio::test]
async fn jira_writes_are_blocked_when_integration_disabled() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = disabled_service(client.clone());

    // Every write path must fail at the enabled-context gate before any client
    // call is recorded.
    assert_eq!(
        service
            .clear_jira_issue_assignee("PROJ-1")
            .await
            .unwrap_err(),
        "Atlassian integration is not enabled"
    );
    assert_eq!(
        service
            .list_jira_issue_transitions("PROJ-1")
            .await
            .unwrap_err(),
        "Atlassian integration is not enabled"
    );
    assert_eq!(
        service
            .transition_jira_issue("PROJ-1", "31")
            .await
            .unwrap_err(),
        "Atlassian integration is not enabled"
    );
    assert_eq!(
        service.add_jira_comment("PROJ-1", "hi").await.unwrap_err(),
        "Atlassian integration is not enabled"
    );
    assert_eq!(
        service
            .set_jira_issue_labels("PROJ-1", vec!["x".to_string()])
            .await
            .unwrap_err(),
        "Atlassian integration is not enabled"
    );

    let calls = client.calls.lock().await;
    assert!(calls.cleared_assignees.is_empty());
    assert!(calls.listed_transitions.is_empty());
    assert!(calls.transitioned.is_empty());
    assert!(calls.comments.is_empty());
    assert!(calls.label_writes.is_empty());
}

// ── search / fetch / project listing routing when enabled ────────────────────

#[tokio::test]
async fn search_resources_clamps_limit_and_routes_to_client() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let results = service
        .search_resources(AtlassianResourceKind::Jira, "bug", 500)
        .await
        .expect("search should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key.as_deref(), Some("PROJ-1"));
    assert_eq!(
        client.searches.lock().await.as_slice(),
        &[(AtlassianResourceKind::Jira, "bug".to_string(), 25)]
    );
}

#[tokio::test]
async fn search_resources_requires_enabled_settings() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = disabled_service(client.clone());

    let error = service
        .search_resources(AtlassianResourceKind::Confluence, "q", 10)
        .await
        .unwrap_err();
    assert_eq!(error, "Atlassian integration is not enabled");
    assert!(client.searches.lock().await.is_empty());
}

#[tokio::test]
async fn fetch_resource_content_routes_to_client() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let content = service
        .fetch_resource_content(&ComposerIntegrationReference {
            provider: "atlassian".to_string(),
            kind: "jira".to_string(),
            id: "10001".to_string(),
            key: Some("PROJ-1".to_string()),
            title: Some("Issue".to_string()),
            url: None,
            summary_excerpt: None,
            include_transcript: None,
        })
        .await
        .expect("fetch should succeed");

    assert_eq!(content.id, "10001");
    assert_eq!(content.body, "Issue body");
}

#[tokio::test]
async fn resolve_resource_urls_converts_supported_authorized_urls() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let results = service
        .resolve_resource_urls(&[
            "https://example.atlassian.net/browse/rx-42".to_string(),
            "https://example.atlassian.net/wiki/spaces/OPS/pages/123456/Deploy-notes".to_string(),
        ])
        .await
        .expect("url resolution");

    assert_eq!(results.len(), 2);
    let jira = results[0].resource.as_ref().expect("jira resource");
    assert_eq!(jira.kind, AtlassianResourceKind::Jira);
    assert_eq!(jira.id, "RX-42");
    assert_eq!(jira.key.as_deref(), Some("RX-42"));
    assert_eq!(jira.title, "Example Jira issue");
    assert_eq!(
        jira.url.as_deref(),
        Some("https://example.atlassian.net/browse/RX-42")
    );

    let confluence = results[1].resource.as_ref().expect("confluence resource");
    assert_eq!(confluence.kind, AtlassianResourceKind::Confluence);
    assert_eq!(confluence.id, "123456");
    assert!(confluence.key.is_none());
    assert_eq!(confluence.title, "Example Confluence page");
    assert_eq!(
        confluence.url.as_deref(),
        Some("https://example.atlassian.net/wiki/spaces/OPS/pages/123456/Deploy-notes")
    );

    let fetches = client.fetches.lock().await;
    assert_eq!(fetches.len(), 2);
    assert_eq!(fetches[0].kind, "jira");
    assert_eq!(fetches[0].id, "RX-42");
    assert_eq!(fetches[0].key.as_deref(), Some("RX-42"));
    assert_eq!(fetches[1].kind, "confluence");
    assert_eq!(fetches[1].id, "123456");
}

#[tokio::test]
async fn resolve_resource_urls_leaves_wrong_site_and_unsupported_urls_unresolved() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let results = service
        .resolve_resource_urls(&[
            "https://other.atlassian.net/browse/RX-42".to_string(),
            "https://example.atlassian.net/wiki/spaces/OPS/overview".to_string(),
        ])
        .await
        .expect("url resolution");

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|result| result.resource.is_none()));
    assert!(client.fetches.lock().await.is_empty());
}

#[tokio::test]
async fn resolve_resource_urls_skips_blank_and_handles_url_edge_cases() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let results = service
        .resolve_resource_urls(&[
            "   ".to_string(),
            "http://example.atlassian.net/browse/RX-42".to_string(),
            "https://example.atlassian.net/browse/123-abc".to_string(),
            "https://example.atlassian.net/projects/RX".to_string(),
            "https://example.atlassian.net/wiki/spaces/OPS/pages/not-number/Deploy".to_string(),
            "https://example.atlassian.net/wiki/spaces/OPS?focusedCommentId=1&pageId=7890"
                .to_string(),
        ])
        .await
        .expect("url resolution");

    assert_eq!(results.len(), 5);
    assert!(results[..4].iter().all(|result| result.resource.is_none()));
    let resource = results[4].resource.as_ref().expect("query pageId resource");
    assert_eq!(resource.kind, AtlassianResourceKind::Confluence);
    assert_eq!(resource.id, "7890");

    let fetches = client.fetches.lock().await;
    assert_eq!(fetches.len(), 1);
    assert_eq!(fetches[0].kind, "confluence");
    assert_eq!(fetches[0].id, "7890");
}

#[tokio::test]
async fn resolve_resource_urls_requires_enabled_settings() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = disabled_service(client.clone());

    let error = service
        .resolve_resource_urls(&["https://example.atlassian.net/browse/RX-42".to_string()])
        .await
        .unwrap_err();

    assert_eq!(error, "Atlassian integration is not enabled");
    assert!(client.fetches.lock().await.is_empty());
}

#[tokio::test]
async fn resolve_resource_urls_keeps_inaccessible_resources_unresolved() {
    let client = Arc::new(TestAtlassianClient::default());
    *client.error.lock().await = Some("Atlassian returned HTTP 404".to_string());
    let service = enabled_service(client.clone()).await;

    let results = service
        .resolve_resource_urls(&["https://example.atlassian.net/browse/RX-404".to_string()])
        .await
        .expect("url resolution");

    assert_eq!(results.len(), 1);
    assert!(results[0].resource.is_none());
    assert_eq!(client.fetches.lock().await.len(), 0);
}

#[tokio::test]
async fn assign_jira_issue_routes_to_client_when_enabled() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    service
        .assign_jira_issue_to_current_user("PROJ-1")
        .await
        .expect("assignment should succeed");

    assert_eq!(
        client.assigned.lock().await.as_slice(),
        &["PROJ-1".to_string()]
    );
}

#[tokio::test]
async fn list_jira_projects_routes_limit_to_client() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let projects = service.list_jira_projects(50).await.expect("projects");

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].key, "PROJ");
    assert_eq!(client.list_projects_limits.lock().await.as_slice(), &[50]);
}

#[tokio::test]
async fn list_jira_project_statuses_routes_key_to_client() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let statuses = service
        .list_jira_project_statuses("PROJ")
        .await
        .expect("statuses");

    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].category, "in_progress");
    assert_eq!(
        client.list_statuses_keys.lock().await.as_slice(),
        &["PROJ".to_string()]
    );
}

#[tokio::test]
async fn list_jira_project_issues_routes_key_and_limit() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let issues = service
        .list_jira_project_issues("PROJ", 75)
        .await
        .expect("issues");

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].key, "PROJ-1");
    assert_eq!(
        client.list_issues_calls.lock().await.as_slice(),
        &[("PROJ".to_string(), 75)]
    );
}

#[tokio::test]
async fn project_listing_methods_require_enabled_settings() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = disabled_service(client.clone());

    assert_eq!(
        service.list_jira_projects(10).await.unwrap_err(),
        "Atlassian integration is not enabled"
    );
    assert_eq!(
        service
            .list_jira_project_statuses("PROJ")
            .await
            .unwrap_err(),
        "Atlassian integration is not enabled"
    );
    assert_eq!(
        service
            .list_jira_project_issues("PROJ", 10)
            .await
            .unwrap_err(),
        "Atlassian integration is not enabled"
    );
    assert_eq!(
        service
            .fetch_resource_content(&ComposerIntegrationReference {
                provider: "atlassian".to_string(),
                kind: "jira".to_string(),
                id: "x".to_string(),
                key: None,
                title: None,
                url: None,
                summary_excerpt: None,
                include_transcript: None,
            })
            .await
            .unwrap_err(),
        "Atlassian integration is not enabled"
    );
    assert_eq!(
        service
            .assign_jira_issue_to_current_user("PROJ-1")
            .await
            .unwrap_err(),
        "Atlassian integration is not enabled"
    );
}

// ── expand_references_for_prompt ─────────────────────────────────────────────

#[tokio::test]
async fn expand_references_returns_message_with_no_references() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;
    assert_eq!(service.expand_references_for_prompt("hi", &[]).await, "hi");
}

#[tokio::test]
async fn expand_references_returns_message_when_disabled() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = disabled_service(client.clone());
    let expanded = service
        .expand_references_for_prompt(
            "hi",
            &[ComposerIntegrationReference {
                provider: "atlassian".to_string(),
                kind: "jira".to_string(),
                id: "PROJ-1".to_string(),
                key: None,
                title: None,
                url: None,
                summary_excerpt: None,
                include_transcript: None,
            }],
        )
        .await;
    assert_eq!(expanded, "hi");
}

#[tokio::test]
async fn expand_references_skips_non_atlassian_and_reports_fetch_errors() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = enabled_service(client.clone()).await;

    let expanded = service
        .expand_references_for_prompt(
            "Fix",
            &[
                ComposerIntegrationReference {
                    provider: "linear".to_string(),
                    kind: "linear".to_string(),
                    id: "LIN-1".to_string(),
                    key: None,
                    title: None,
                    url: None,
                    summary_excerpt: None,
                    include_transcript: None,
                },
                ComposerIntegrationReference {
                    provider: "atlassian".to_string(),
                    kind: "jira".to_string(),
                    id: "PROJ-1".to_string(),
                    key: Some("PROJ-1".to_string()),
                    title: Some("Issue".to_string()),
                    url: None,
                    summary_excerpt: None,
                    include_transcript: None,
                },
            ],
        )
        .await;

    // The non-atlassian reference is skipped silently; the atlassian one renders.
    assert!(expanded.contains("ralphx_integration_references"));
    assert!(expanded.contains("<jira"));
    assert!(!expanded.contains("LIN-1"));
}

#[tokio::test]
async fn expand_references_reports_fetch_error_as_skipped() {
    let client = Arc::new(TestAtlassianClient::default());
    *client.error.lock().await = Some("Jira issue not found".to_string());
    let service = enabled_service(client.clone()).await;

    let expanded = service
        .expand_references_for_prompt(
            "Fix",
            &[ComposerIntegrationReference {
                provider: "atlassian".to_string(),
                kind: "jira".to_string(),
                id: "PROJ-404".to_string(),
                key: None,
                title: None,
                url: None,
                summary_excerpt: None,
                include_transcript: None,
            }],
        )
        .await;

    assert!(expanded.contains("integration_reference_skipped"));
    assert!(expanded.contains("Jira issue not found"));
}

#[tokio::test]
async fn expand_references_truncates_large_resource_body() {
    let client = Arc::new(TestAtlassianClient::default());
    *client.fetch_body.lock().await = Some("z".repeat(70 * 1024));
    let service = enabled_service(client.clone()).await;

    let expanded = service
        .expand_references_for_prompt(
            "Fix",
            &[ComposerIntegrationReference {
                provider: "atlassian".to_string(),
                kind: "jira".to_string(),
                id: "PROJ-1".to_string(),
                key: Some("PROJ-1".to_string()),
                title: Some("Big".to_string()),
                url: None,
                summary_excerpt: None,
                include_transcript: None,
            }],
        )
        .await;

    assert!(expanded.contains("truncated=\"true\""), "{expanded}");
    assert!(expanded.contains("bytes=\"71680\""));
}

// ── validate_and_enable + save_settings + disconnect flows ───────────────────

/// Builds a fresh API-token service with seeded settings/secret so validate flows
/// can route to the (happy) test client.
async fn api_token_service(client: Arc<TestAtlassianClient>) -> AtlassianIntegrationService {
    let repo = Arc::new(TestSettingsRepo::enabled());
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret(TEST_TOKEN_REF, "secret-token")
        .await
        .unwrap();
    AtlassianIntegrationService::new(repo, secrets, client)
}

#[tokio::test]
async fn validate_and_enable_marks_valid_on_success() {
    let client = Arc::new(TestAtlassianClient::default());
    let service = api_token_service(client).await;

    let settings = service.validate_and_enable().await.unwrap();

    assert!(settings.enabled);
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::Valid
    );
    assert!(settings.jira_available);
    assert!(settings.confluence_available);
    assert!(settings.last_error.is_none());
    assert!(settings.last_validated_at.is_some());
}

#[tokio::test]
async fn validate_and_enable_requires_email_for_api_token() {
    // Enabled API-token settings with a token but NO email: auth_context fails
    // before the client validate is reached.
    let mut seeded = enabled_api_token_settings();
    seeded.email = None;
    let repo = Arc::new(TestSettingsRepo {
        settings: RwLock::new(seeded),
    });
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret(TEST_TOKEN_REF, "secret-token")
        .await
        .unwrap();
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets, client);

    let error = service.validate_and_enable().await.unwrap_err();
    assert_eq!(error, "Atlassian account email is required");
}

#[tokio::test]
async fn validate_and_enable_requires_token_secret_in_storage() {
    // Settings reference a token secret, but the secret store does not hold it.
    let repo = Arc::new(TestSettingsRepo::enabled());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets, client);

    let error = service.validate_and_enable().await.unwrap_err();
    assert_eq!(error, "Atlassian API token is missing from secure storage");
}

#[tokio::test]
async fn save_settings_normalizes_and_resets_state_for_api_token() {
    let repo = Arc::new(TestSettingsRepo::disabled());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets.clone(), client);

    let saved = service
        .save_settings(
            Some(AtlassianAuthMethod::ApiToken),
            Some(" example.atlassian.net/ ".to_string()),
            Some(" user@example.com ".to_string()),
            Some(" the-token ".to_string()),
            None,
            None,
            None,
        )
        .await
        .unwrap();

    assert!(!saved.enabled);
    assert_eq!(
        saved.site_url.as_deref(),
        Some("https://example.atlassian.net")
    );
    assert_eq!(saved.email.as_deref(), Some("user@example.com"));
    assert!(saved.token_secret_ref.is_some());
    // All three required API-token fields are present → Pending.
    assert_eq!(
        saved.validation_status,
        IntegrationValidationStatus::Pending
    );
    assert_eq!(
        secrets.get_secret(TEST_TOKEN_REF).await.unwrap().as_deref(),
        Some("the-token")
    );
}

#[tokio::test]
async fn save_settings_clears_token_when_blank() {
    let repo = Arc::new(TestSettingsRepo::enabled());
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret(TEST_TOKEN_REF, "secret-token")
        .await
        .unwrap();
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets.clone(), client);

    let saved = service
        .save_settings(None, None, None, Some("   ".to_string()), None, None, None)
        .await
        .unwrap();

    assert!(saved.token_secret_ref.is_none());
    // The old secret was deleted from storage.
    assert!(secrets.get_secret(TEST_TOKEN_REF).await.unwrap().is_none());
}

#[tokio::test]
async fn save_settings_rejects_invalid_redirect_uri() {
    let repo = Arc::new(TestSettingsRepo::disabled());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets, client);

    let error = service
        .save_settings(
            Some(AtlassianAuthMethod::OAuth),
            None,
            None,
            None,
            Some("client-id".to_string()),
            Some("client-secret".to_string()),
            Some("https://example.com/callback".to_string()),
        )
        .await
        .unwrap_err();

    assert!(
        error.contains("loopback"),
        "redirect URI must be loopback: {error}"
    );
}

#[tokio::test]
async fn disconnect_clears_all_secrets_and_resets() {
    let repo = Arc::new(TestSettingsRepo::enabled());
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret(TEST_TOKEN_REF, "secret-token")
        .await
        .unwrap();
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets.clone(), client);

    let cleared = service.disconnect().await.unwrap();

    assert!(!cleared.enabled);
    assert_eq!(
        cleared.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(cleared.token_secret_ref.is_none());
    assert!(secrets.get_secret(TEST_TOKEN_REF).await.unwrap().is_none());
}

#[tokio::test]
async fn exchange_oauth_code_applies_token_and_enables_oauth_connection() {
    // OAuth settings: client id, redirect URI, and a stored client secret.
    const SECRET_REF: &str = "integrations/atlassian/default/oauth-client-secret";
    let seeded = AtlassianIntegrationSettings {
        auth_method: AtlassianAuthMethod::OAuth,
        oauth_client_id: Some("client-id".to_string()),
        oauth_redirect_uri: Some("http://127.0.0.1:8765/atlassian/oauth/callback".to_string()),
        oauth_client_secret_ref: Some(SECRET_REF.to_string()),
        ..AtlassianIntegrationSettings::default()
    };
    let repo = Arc::new(TestSettingsRepo {
        settings: RwLock::new(seeded),
    });
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret(SECRET_REF, "client-secret")
        .await
        .unwrap();

    let client = Arc::new(TestAtlassianClient::default());
    *client.oauth_token.lock().await = Some(AtlassianOAuthTokenResponse {
        access_token: "access-123".to_string(),
        refresh_token: Some("refresh-456".to_string()),
        expires_in: Some(3600),
        scope: Some("read:jira-work".to_string()),
    });
    *client.oauth_resources.lock().await = vec![AtlassianOAuthResource {
        id: "cloud-abc".to_string(),
        url: "https://acme.atlassian.net".to_string(),
        scopes: vec!["read:jira-work".to_string()],
    }];
    let service = AtlassianIntegrationService::new(repo, secrets.clone(), client);

    let settings = service
        .exchange_oauth_code("auth-code".to_string())
        .await
        .expect("oauth exchange should enable the connection");

    // The token response was applied: site URL + cloud id resolved from the
    // accessible resource, and the access token persisted to secure storage.
    assert!(settings.enabled);
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::Valid
    );
    assert_eq!(
        settings.site_url.as_deref(),
        Some("https://acme.atlassian.net")
    );
    assert_eq!(settings.oauth_cloud_id.as_deref(), Some("cloud-abc"));
    assert_eq!(settings.oauth_scopes.as_deref(), Some("read:jira-work"));
    assert!(settings.oauth_access_token_ref.is_some());
    assert!(settings.oauth_refresh_token_ref.is_some());
    assert!(settings.oauth_access_token_expires_at.is_some());
    assert_eq!(
        secrets
            .get_secret("integrations/atlassian/default/oauth-access-token")
            .await
            .unwrap()
            .as_deref(),
        Some("access-123")
    );
}

#[tokio::test]
async fn exchange_oauth_code_requires_redirect_uri() {
    let seeded = AtlassianIntegrationSettings {
        auth_method: AtlassianAuthMethod::OAuth,
        oauth_client_id: Some("client-id".to_string()),
        ..AtlassianIntegrationSettings::default()
    };
    let repo = Arc::new(TestSettingsRepo {
        settings: RwLock::new(seeded),
    });
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets, client);

    let error = service
        .exchange_oauth_code("auth-code".to_string())
        .await
        .unwrap_err();
    assert_eq!(error, "Atlassian OAuth redirect URI is required");
}

#[tokio::test]
async fn build_oauth_authorization_requires_client_id() {
    let repo = Arc::new(TestSettingsRepo::disabled());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets, client);

    let error = service.build_oauth_authorization().await.unwrap_err();
    assert_eq!(error, "Atlassian OAuth client ID is required");
}

#[tokio::test]
async fn build_oauth_authorization_produces_consent_url_with_defaults() {
    let seeded = AtlassianIntegrationSettings {
        oauth_client_id: Some("my-client".to_string()),
        ..AtlassianIntegrationSettings::default()
    };
    let repo = Arc::new(TestSettingsRepo {
        settings: RwLock::new(seeded),
    });
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestAtlassianClient::default());
    let service = AtlassianIntegrationService::new(repo, secrets, client);

    let authorization = service.build_oauth_authorization().await.unwrap();

    assert!(authorization
        .authorization_url
        .contains("auth.atlassian.com/authorize"));
    assert!(authorization
        .authorization_url
        .contains("client_id=my-client"));
    assert!(authorization
        .authorization_url
        .contains("response_type=code"));
    assert!(!authorization.state.is_empty());
    // The default loopback redirect URI is used.
    assert_eq!(
        authorization.redirect_uri,
        "http://127.0.0.1:8765/atlassian/oauth/callback"
    );
}

// ── EmptyAtlassianApiClient / UnavailableAtlassianApiClient ───────────────────

fn api_token_auth() -> AtlassianAuthContext {
    AtlassianAuthContext {
        site_url: "https://example.atlassian.net".to_string(),
        credential: AtlassianCredential::ApiToken {
            email: "user@example.com".to_string(),
            token: "secret-token".to_string(),
        },
    }
}

#[tokio::test]
async fn empty_client_returns_happy_path_stubs() {
    let client = EmptyAtlassianApiClient;
    let auth = api_token_auth();

    // The empty client returns happy-path stubs for the methods it overrides.
    let _connectivity = client.validate(&auth).await.unwrap();
    assert!(client
        .search(&auth, AtlassianResourceKind::Jira, "q", 5)
        .await
        .unwrap()
        .is_empty());
    let fetched = client
        .fetch(
            &auth,
            &ComposerIntegrationReference {
                provider: "atlassian".to_string(),
                kind: "jira".to_string(),
                id: "PROJ-1".to_string(),
                key: Some("PROJ-1".to_string()),
                title: None,
                url: None,
                summary_excerpt: None,
                include_transcript: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(fetched.id, "PROJ-1");
    client
        .assign_jira_issue_to_current_user(&auth, "PROJ-1")
        .await
        .unwrap();
    client
        .clear_jira_issue_assignee(&auth, "PROJ-1")
        .await
        .unwrap();
    assert!(client
        .list_jira_issue_transitions(&auth, "PROJ-1")
        .await
        .unwrap()
        .is_empty());
    client
        .transition_jira_issue(&auth, "PROJ-1", "31")
        .await
        .unwrap();
    let comment = client
        .add_jira_comment(&auth, "PROJ-1", "body")
        .await
        .unwrap();
    assert_eq!(comment.body_markdown, "body");

    // Methods the empty client does NOT override fall back to the trait default
    // error impls.
    assert!(client
        .set_jira_issue_labels(&auth, "PROJ-1", vec![])
        .await
        .is_err());
    assert!(client.list_jira_projects(&auth, 10).await.is_err());
    assert!(client
        .list_jira_project_statuses(&auth, "PROJ")
        .await
        .is_err());
    assert!(client
        .list_jira_project_issues(&auth, "PROJ", 10)
        .await
        .is_err());
}

#[tokio::test]
async fn unavailable_client_propagates_reason() {
    let client = UnavailableAtlassianApiClient::new("Atlassian is down");
    let auth = api_token_auth();

    assert_eq!(
        client.validate(&auth).await.unwrap_err(),
        "Atlassian is down"
    );
    assert_eq!(
        client
            .search(&auth, AtlassianResourceKind::Confluence, "q", 5)
            .await
            .unwrap_err(),
        "Atlassian is down"
    );
    assert_eq!(
        client
            .fetch(
                &auth,
                &ComposerIntegrationReference {
                    provider: "atlassian".to_string(),
                    kind: "jira".to_string(),
                    id: "PROJ-1".to_string(),
                    key: None,
                    title: None,
                    url: None,
                    summary_excerpt: None,
                    include_transcript: None,
                }
            )
            .await
            .unwrap_err(),
        "Atlassian is down"
    );
    assert_eq!(
        client
            .assign_jira_issue_to_current_user(&auth, "PROJ-1")
            .await
            .unwrap_err(),
        "Atlassian is down"
    );
}

// ── Pure helpers ─────────────────────────────────────────────────────────────

#[test]
fn resource_kind_from_str_and_as_str_roundtrip() {
    use std::str::FromStr;
    assert_eq!(
        AtlassianResourceKind::from_str("jira").unwrap(),
        AtlassianResourceKind::Jira
    );
    assert_eq!(
        AtlassianResourceKind::from_str("confluence").unwrap(),
        AtlassianResourceKind::Confluence
    );
    assert!(AtlassianResourceKind::from_str("bogus").is_err());
    assert_eq!(AtlassianResourceKind::Jira.as_str(), "jira");
    assert_eq!(AtlassianResourceKind::Confluence.as_str(), "confluence");
}

#[test]
fn normalize_site_url_handles_empty_scheme_and_host_cases() {
    assert_eq!(normalize_site_url("   ").unwrap(), "");
    // No scheme → https prepended.
    assert_eq!(
        normalize_site_url("acme.atlassian.net").unwrap(),
        "https://acme.atlassian.net"
    );
    // Non-https scheme rejected.
    assert!(normalize_site_url("http://acme.atlassian.net").is_err());
}

#[test]
fn parse_loopback_redirect_uri_rejects_bad_shapes() {
    assert!(normalize_oauth_redirect_uri("").is_err());
    // Non-http scheme.
    assert!(normalize_oauth_redirect_uri("https://127.0.0.1:8765/cb").is_err());
    // Query string not allowed.
    assert!(normalize_oauth_redirect_uri("http://127.0.0.1:8765/cb?x=1").is_err());
    // Non-loopback IPv4.
    assert!(normalize_oauth_redirect_uri("http://10.0.0.1:8765/cb").is_err());
    // Missing port.
    assert!(normalize_oauth_redirect_uri("http://127.0.0.1/cb").is_err());
    // localhost is accepted and lowercased.
    assert_eq!(
        normalize_oauth_redirect_uri("http://LOCALHOST:8765/cb").unwrap(),
        "http://localhost:8765/cb"
    );
}

#[test]
fn oauth_callback_result_reports_provider_error() {
    let mut params = HashMap::new();
    params.insert("error".to_string(), "access_denied".to_string());
    params.insert("error_description".to_string(), "User said no".to_string());
    let error = oauth_callback_result(&params, "expected").unwrap_err();
    assert!(error.contains("User said no"), "{error}");

    // Missing code is rejected even when state matches.
    let mut params = HashMap::new();
    params.insert("state".to_string(), "expected".to_string());
    let error = oauth_callback_result(&params, "expected").unwrap_err();
    assert!(error.contains("authorization code"), "{error}");

    // Missing state is rejected.
    let params = HashMap::new();
    let error = oauth_callback_result(&params, "expected").unwrap_err();
    assert!(error.contains("did not include state"), "{error}");
}

#[test]
fn oauth_callback_html_renders_success_and_failure() {
    let ok = oauth_callback_html(&Ok("code".to_string()));
    assert!(ok.contains("authorization received"));
    let err = oauth_callback_html(&Err("bad <thing>".to_string()));
    assert!(err.contains("authorization failed"));
    // The error is HTML-escaped.
    assert!(err.contains("&lt;thing&gt;"));
}

#[test]
fn pending_status_for_settings_covers_both_auth_methods() {
    // API token: not configured until all three fields present.
    let mut settings = AtlassianIntegrationSettings {
        auth_method: AtlassianAuthMethod::ApiToken,
        ..AtlassianIntegrationSettings::default()
    };
    assert_eq!(
        pending_status_for_settings(&settings),
        IntegrationValidationStatus::NotConfigured
    );
    settings.site_url = Some("https://x.atlassian.net".to_string());
    settings.email = Some("u@example.com".to_string());
    settings.token_secret_ref = Some("ref".to_string());
    assert_eq!(
        pending_status_for_settings(&settings),
        IntegrationValidationStatus::Pending
    );

    // OAuth: needs site_url + client_id + redirect_uri + client_secret_ref.
    let mut oauth = AtlassianIntegrationSettings {
        auth_method: AtlassianAuthMethod::OAuth,
        ..AtlassianIntegrationSettings::default()
    };
    assert_eq!(
        pending_status_for_settings(&oauth),
        IntegrationValidationStatus::NotConfigured
    );
    oauth.site_url = Some("https://x.atlassian.net".to_string());
    oauth.oauth_client_id = Some("client".to_string());
    oauth.oauth_redirect_uri = Some("http://127.0.0.1:8765/cb".to_string());
    oauth.oauth_client_secret_ref = Some("secret-ref".to_string());
    assert_eq!(
        pending_status_for_settings(&oauth),
        IntegrationValidationStatus::Pending
    );
}

#[test]
fn select_oauth_resource_matches_and_rejects() {
    let resources = vec![
        AtlassianOAuthResource {
            id: "cloud-1".to_string(),
            url: "https://acme.atlassian.net".to_string(),
            scopes: vec![],
        },
        AtlassianOAuthResource {
            id: "cloud-2".to_string(),
            url: "https://other.atlassian.net".to_string(),
            scopes: vec![],
        },
    ];

    // No site URL → first resource.
    let chosen = select_oauth_resource(None, &resources).unwrap();
    assert_eq!(chosen.id, "cloud-1");

    // Matching site URL → matched resource.
    let chosen = select_oauth_resource(Some("https://other.atlassian.net"), &resources).unwrap();
    assert_eq!(chosen.id, "cloud-2");

    // Non-matching site URL → error.
    assert!(select_oauth_resource(Some("https://nope.atlassian.net"), &resources).is_err());

    // Empty resource list → error.
    assert!(select_oauth_resource(None, &[]).is_err());
}

#[test]
fn percent_encode_passes_unreserved_and_escapes_others() {
    assert_eq!(percent_encode("aZ09-_.~"), "aZ09-_.~");
    assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
}

#[test]
fn escape_attr_escapes_markup_characters() {
    assert_eq!(escape_attr("a&b\"<c>"), "a&amp;b&quot;&lt;c&gt;");
}
