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
        Err("not used in these tests".to_string())
    }

    async fn assign_jira_issue_to_current_user(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
    ) -> Result<(), String> {
        Ok(())
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
        Err("not used in these tests".to_string())
    }

    async fn refresh_oauth_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _refresh_token: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        Err("not used in these tests".to_string())
    }

    async fn oauth_accessible_resources(
        &self,
        _access_token: &str,
    ) -> Result<Vec<AtlassianOAuthResource>, String> {
        Err("not used in these tests".to_string())
    }
}

/// Builds a service whose settings are enabled/valid and whose secret store
/// holds the API token, so `enabled_auth_context` resolves successfully.
async fn enabled_service(
    client: Arc<TestAtlassianClient>,
) -> AtlassianIntegrationService {
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
        service.clear_jira_issue_assignee("PROJ-1").await.unwrap_err(),
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
        service.transition_jira_issue("PROJ-1", "31").await.unwrap_err(),
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
