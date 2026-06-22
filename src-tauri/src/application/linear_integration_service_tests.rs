use std::sync::Arc;

use async_trait::async_trait;
use std::collections::HashMap;

use tokio::sync::{Mutex, RwLock};

use super::{
    resolve_linear_label_ids, LinearApiClient, LinearAuthContext, LinearComment,
    LinearIntegrationService, LinearIntegrationSettings, LinearIntegrationSettingsRepository,
    LinearIssueContent, LinearIssueSummary, LinearLabel, LinearProject, LinearUser,
    LinearWorkflowState,
};
use crate::domain::integrations::IntegrationValidationStatus;
use crate::domain::services::{ComposerIntegrationReference, SecretStore, SecretStoreError};
use crate::infrastructure::memory::MemorySecretStore;

#[derive(Default)]
struct TestSettingsRepo {
    settings: RwLock<LinearIntegrationSettings>,
}

#[async_trait]
impl LinearIntegrationSettingsRepository for TestSettingsRepo {
    async fn get(&self) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &LinearIntegrationSettings,
    ) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        *self.settings.write().await = settings.clone();
        Ok(settings.clone())
    }
}

#[derive(Default)]
struct TestLinearClient {
    searches: Mutex<Vec<(String, usize)>>,
    validate_error: Mutex<Option<String>>,
    fetch_error: Mutex<Option<String>>,
    list_projects_first: Mutex<Vec<usize>>,
    list_workflow_states_team: Mutex<Vec<Option<String>>>,
    current_user_calls: Mutex<usize>,
    update_issue_state_calls: Mutex<Vec<(String, String)>>,
    assign_calls: Mutex<Vec<String>>,
    clear_assignee_calls: Mutex<Vec<String>>,
    create_comment_calls: Mutex<Vec<(String, String)>>,
    list_issue_team_labels_calls: Mutex<Vec<String>>,
    update_issue_labels_calls: Mutex<Vec<(String, Vec<String>)>>,
    team_labels: Mutex<Vec<LinearLabel>>,
}

#[derive(Default)]
struct RecordingSecretStore {
    secrets: RwLock<HashMap<String, String>>,
    deleted: Mutex<Vec<String>>,
}

#[async_trait]
impl SecretStore for RecordingSecretStore {
    async fn put_secret(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        self.secrets
            .write()
            .await
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn get_secret(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        Ok(self.secrets.read().await.get(key).cloned())
    }

    async fn delete_secret(&self, key: &str) -> Result<(), SecretStoreError> {
        self.deleted.lock().await.push(key.to_string());
        self.secrets.write().await.remove(key);
        Ok(())
    }
}

#[async_trait]
impl LinearApiClient for TestLinearClient {
    async fn validate(&self, auth: &LinearAuthContext) -> Result<(), String> {
        assert_eq!(auth.api_token, "lin-api-token");
        if let Some(error) = self.validate_error.lock().await.clone() {
            Err(error)
        } else {
            Ok(())
        }
    }

    async fn search_issues(
        &self,
        auth: &LinearAuthContext,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LinearIssueSummary>, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.searches.lock().await.push((query.to_string(), limit));
        Ok(vec![LinearIssueSummary {
            id: "issue-id".to_string(),
            key: Some("LIN-123".to_string()),
            title: "Example".to_string(),
            url: Some("https://linear.app/acme/issue/LIN-123/example".to_string()),
            excerpt: Some("Example body".to_string()),
            state_id: Some("state-started".to_string()),
            state_name: Some("In Progress".to_string()),
            state_category: Some("in_progress".to_string()),
            state_color: Some("#f2c94c".to_string()),
            assignee: Some("A. User".to_string()),
            updated_at: Some("2026-06-21T08:00:00Z".to_string()),
            labels: vec!["backend".to_string()],
            project: Some("Platform".to_string()),
        }])
    }

    async fn fetch_issue(
        &self,
        auth: &LinearAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<LinearIssueContent, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        if let Some(error) = self.fetch_error.lock().await.clone() {
            return Err(error);
        }
        Ok(LinearIssueContent {
            id: reference.id.clone(),
            key: reference.key.clone(),
            title: reference
                .title
                .clone()
                .unwrap_or_else(|| "Example".to_string()),
            url: reference.url.clone(),
            body: "Issue body".to_string(),
            state_name: Some("In Progress".to_string()),
            assignee: Some("A. User".to_string()),
            creator: Some("C. User".to_string()),
            updated_at: Some("2026-06-18T08:00:00Z".to_string()),
            comments: Vec::new(),
            labels: Vec::new(),
            project: None,
        })
    }

    async fn list_workflow_states(
        &self,
        auth: &LinearAuthContext,
        team_id: Option<&str>,
    ) -> Result<Vec<LinearWorkflowState>, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.list_workflow_states_team
            .lock()
            .await
            .push(team_id.map(str::to_string));
        Ok(vec![LinearWorkflowState {
            id: "state-1".to_string(),
            name: "In Progress".to_string(),
            category: "in_progress".to_string(),
            color: Some("#f2c94c".to_string()),
        }])
    }

    async fn current_user(&self, auth: &LinearAuthContext) -> Result<LinearUser, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        *self.current_user_calls.lock().await += 1;
        Ok(LinearUser {
            id: "viewer-id".to_string(),
            name: Some("A. User".to_string()),
        })
    }

    async fn update_issue_state(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
        state_id: &str,
    ) -> Result<(), String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.update_issue_state_calls
            .lock()
            .await
            .push((issue_id.to_string(), state_id.to_string()));
        Ok(())
    }

    async fn assign_issue_to_current_user(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
    ) -> Result<LinearUser, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.assign_calls.lock().await.push(issue_id.to_string());
        Ok(LinearUser {
            id: "viewer-id".to_string(),
            name: Some("A. User".to_string()),
        })
    }

    async fn clear_issue_assignee(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
    ) -> Result<(), String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.clear_assignee_calls
            .lock()
            .await
            .push(issue_id.to_string());
        Ok(())
    }

    async fn create_comment(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
        body_markdown: &str,
    ) -> Result<LinearComment, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.create_comment_calls
            .lock()
            .await
            .push((issue_id.to_string(), body_markdown.to_string()));
        Ok(LinearComment {
            id: "comment-1".to_string(),
            body: body_markdown.to_string(),
            author_id: Some("viewer-id".to_string()),
            author_name: Some("A. User".to_string()),
            created_at: Some("2026-06-21T08:00:00Z".to_string()),
            updated_at: Some("2026-06-21T08:00:00Z".to_string()),
        })
    }

    async fn list_projects(
        &self,
        auth: &LinearAuthContext,
        first: usize,
    ) -> Result<Vec<LinearProject>, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.list_projects_first.lock().await.push(first);
        Ok(vec![LinearProject {
            id: "project-1".to_string(),
            name: "Platform".to_string(),
        }])
    }

    async fn list_issue_team_labels(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
    ) -> Result<Vec<LinearLabel>, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.list_issue_team_labels_calls
            .lock()
            .await
            .push(issue_id.to_string());
        Ok(self.team_labels.lock().await.clone())
    }

    async fn update_issue_labels(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
        label_ids: Vec<String>,
    ) -> Result<(), String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.update_issue_labels_calls
            .lock()
            .await
            .push((issue_id.to_string(), label_ids));
        Ok(())
    }
}

#[tokio::test]
async fn search_requires_valid_enabled_settings() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets, client);

    let error = service.search_issues("bug", 10).await.unwrap_err();

    assert_eq!(error, "Linear integration is not enabled");
}

#[tokio::test]
async fn save_validate_and_search_issues_with_api_token() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets, client.clone());

    let saved = service
        .save_settings(Some(" lin-api-token ".to_string()))
        .await
        .unwrap();
    assert!(!saved.enabled);
    assert_eq!(
        saved.validation_status,
        IntegrationValidationStatus::Pending
    );

    let validated = service.validate_and_enable().await.unwrap();
    assert!(validated.enabled);
    assert_eq!(
        validated.validation_status,
        IntegrationValidationStatus::Valid
    );
    assert!(validated.issue_search_available);

    let results = service.search_issues("bug", 50).await.unwrap();
    assert_eq!(results[0].key.as_deref(), Some("LIN-123"));
    assert_eq!(
        client.searches.lock().await.as_slice(),
        &[("bug".to_string(), 25)]
    );
}

#[tokio::test]
async fn blank_search_uses_enabled_provider_for_default_ticket_list() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets, client.clone());

    service
        .save_settings(Some(" lin-api-token ".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let results = service.search_issues("  ", 50).await.unwrap();

    assert_eq!(results[0].key.as_deref(), Some("LIN-123"));
    assert_eq!(
        client.searches.lock().await.as_slice(),
        &[("  ".to_string(), 25)]
    );
}

#[tokio::test]
async fn clearing_api_token_deletes_existing_secret_and_marks_not_configured() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets.clone(), client);

    let saved = service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();
    let secret_ref = saved.token_secret_ref.clone().unwrap();
    let cleared = service
        .save_settings(Some("   ".to_string()))
        .await
        .unwrap();

    assert!(cleared.token_secret_ref.is_none());
    assert_eq!(
        cleared.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert_eq!(secrets.deleted.lock().await.as_slice(), &[secret_ref]);
}

#[tokio::test]
async fn disconnect_deletes_secret_and_resets_valid_connection() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets.clone(), client);

    let saved = service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();
    let secret_ref = saved.token_secret_ref.clone().unwrap();
    let validated = service.validate_and_enable().await.unwrap();
    assert!(validated.enabled);

    let cleared = service.disconnect().await.unwrap();

    assert!(!cleared.enabled);
    assert!(cleared.token_secret_ref.is_none());
    assert_eq!(
        cleared.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(!cleared.issue_search_available);
    assert!(cleared.last_error.is_none());
    assert!(cleared.last_validated_at.is_none());
    assert_eq!(secrets.deleted.lock().await.as_slice(), &[secret_ref]);
}

#[tokio::test]
async fn disconnect_is_noop_when_not_configured() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets.clone(), client);

    let cleared = service.disconnect().await.unwrap();

    assert!(!cleared.enabled);
    assert!(cleared.token_secret_ref.is_none());
    assert_eq!(
        cleared.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(secrets.deleted.lock().await.is_empty());
}

#[tokio::test]
async fn validation_failure_disables_search() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    *client.validate_error.lock().await = Some("Linear rejected credentials".to_string());
    let service = LinearIntegrationService::new(repo, secrets, client);
    service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();

    let settings = service.validate_and_enable().await.unwrap();

    assert!(!settings.enabled);
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        settings.last_error.as_deref(),
        Some("Linear rejected credentials")
    );
}

#[tokio::test]
async fn replacing_api_token_uses_fresh_readable_secret_ref() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets.clone(), client);

    let first = service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();
    let first_ref = first.token_secret_ref.clone().unwrap();
    let second = service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();
    let second_ref = second.token_secret_ref.clone().unwrap();

    assert_ne!(first_ref, second_ref);
    assert_eq!(
        secrets.get_secret(&second_ref).await.unwrap().as_deref(),
        Some("lin-api-token")
    );
    assert_eq!(secrets.deleted.lock().await.as_slice(), &[first_ref]);
}

struct UnreadableSecretStore;

#[async_trait]
impl SecretStore for UnreadableSecretStore {
    async fn put_secret(&self, _key: &str, _value: &str) -> Result<(), SecretStoreError> {
        Ok(())
    }

    async fn get_secret(&self, _key: &str) -> Result<Option<String>, SecretStoreError> {
        Err(SecretStoreError::Unavailable(
            "The user name or passphrase you entered is not correct.".to_string(),
        ))
    }

    async fn delete_secret(&self, _key: &str) -> Result<(), SecretStoreError> {
        Ok(())
    }
}

#[tokio::test]
async fn save_reports_unreadable_secure_storage_before_marking_token_stored() {
    let repo = Arc::new(TestSettingsRepo::default());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, Arc::new(UnreadableSecretStore), client);

    let error = service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap_err();

    assert!(error.contains("could not be read back from secure storage"));
    assert!(error.contains("passphrase"));
}

#[tokio::test]
async fn expands_linear_issue_references_for_prompt() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets, client);
    service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let expanded = service
        .expand_references_for_prompt(
            "Fix this",
            &[ComposerIntegrationReference {
                provider: "linear".to_string(),
                kind: "linear".to_string(),
                id: "issue-id".to_string(),
                key: Some("LIN-123".to_string()),
                title: Some("Example".to_string()),
                url: Some("https://linear.app/acme/issue/LIN-123/example".to_string()),
            }],
        )
        .await;

    assert!(expanded.contains("<linear_issue"));
    assert!(expanded.contains("LIN-123"));
    assert!(expanded.contains("Issue body"));
}

#[tokio::test]
async fn expand_references_skips_non_linear_and_reports_fetch_errors() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    *client.fetch_error.lock().await = Some("Linear issue not found".to_string());
    let service = LinearIntegrationService::new(repo, secrets, client);
    service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let expanded = service
        .expand_references_for_prompt(
            "Fix this",
            &[
                ComposerIntegrationReference {
                    provider: "atlassian".to_string(),
                    kind: "jira".to_string(),
                    id: "RX-1".to_string(),
                    key: Some("RX-1".to_string()),
                    title: Some("Ignored Jira issue".to_string()),
                    url: None,
                },
                ComposerIntegrationReference {
                    provider: "linear".to_string(),
                    kind: "linear".to_string(),
                    id: "issue-id".to_string(),
                    key: Some("LIN-123".to_string()),
                    title: Some("Example".to_string()),
                    url: None,
                },
            ],
        )
        .await;

    assert!(expanded.contains("ralphx_integration_references"));
    assert!(expanded.contains("integration_reference_skipped"));
    assert!(expanded.contains("Linear issue not found"));
    assert!(!expanded.contains("RX-1"));
}

fn team_labels() -> Vec<LinearLabel> {
    vec![
        LinearLabel {
            id: "label-bug".to_string(),
            name: "Bug".to_string(),
        },
        LinearLabel {
            id: "label-feature".to_string(),
            name: "Feature".to_string(),
        },
    ]
}

#[test]
fn resolve_linear_label_ids_matches_exact_names() {
    let ids = resolve_linear_label_ids(
        &["Bug".to_string(), "Feature".to_string()],
        &team_labels(),
    )
    .expect("exact names should resolve");
    assert_eq!(ids, vec!["label-bug".to_string(), "label-feature".to_string()]);
}

#[test]
fn resolve_linear_label_ids_is_case_insensitive_and_trims() {
    let ids = resolve_linear_label_ids(&[" bug ".to_string(), "FEATURE".to_string()], &team_labels())
        .expect("case-insensitive trimmed names should resolve");
    assert_eq!(ids, vec!["label-bug".to_string(), "label-feature".to_string()]);
}

#[test]
fn resolve_linear_label_ids_dedupes_repeated_names() {
    let ids = resolve_linear_label_ids(
        &["Bug".to_string(), "bug".to_string(), " BUG ".to_string()],
        &team_labels(),
    )
    .expect("duplicate names should resolve once");
    assert_eq!(ids, vec!["label-bug".to_string()]);
}

#[test]
fn resolve_linear_label_ids_skips_empty_names() {
    let ids = resolve_linear_label_ids(
        &["".to_string(), "   ".to_string(), "Bug".to_string()],
        &team_labels(),
    )
    .expect("empty names are ignored");
    assert_eq!(ids, vec!["label-bug".to_string()]);
}

#[test]
fn resolve_linear_label_ids_rejects_unknown_names() {
    let error = resolve_linear_label_ids(
        &["Bug".to_string(), "Nonexistent".to_string()],
        &team_labels(),
    )
    .expect_err("unknown names should error");
    assert!(error.contains("Nonexistent"), "error should name the missing label: {error}");
}

#[test]
fn resolve_linear_label_ids_preserves_first_seen_order() {
    let ids = resolve_linear_label_ids(
        &["Feature".to_string(), "Bug".to_string(), "feature".to_string()],
        &team_labels(),
    )
    .expect("names should resolve");
    // First-seen order is preserved and the repeated "feature" id is not added twice.
    assert_eq!(
        ids,
        vec!["label-feature".to_string(), "label-bug".to_string()]
    );
}

/// Saves a token and marks the integration valid+enabled so enabled-context flow
/// methods can route through the test Linear client.
async fn enabled_service(
    client: Arc<TestLinearClient>,
) -> LinearIntegrationService {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let service = LinearIntegrationService::new(repo, secrets, client);
    service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();
    service
}

#[tokio::test]
async fn list_projects_clamps_first_into_one_to_hundred_range() {
    let client = Arc::new(TestLinearClient::default());
    let service = enabled_service(client.clone()).await;

    service.list_projects(0).await.unwrap();
    service.list_projects(500).await.unwrap();
    service.list_projects(50).await.unwrap();

    assert_eq!(
        client.list_projects_first.lock().await.as_slice(),
        &[1, 100, 50]
    );
}

#[tokio::test]
async fn list_projects_requires_enabled_settings() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets, client.clone());

    let error = service.list_projects(10).await.unwrap_err();

    assert_eq!(error, "Linear integration is not enabled");
    assert!(client.list_projects_first.lock().await.is_empty());
}

#[tokio::test]
async fn list_workflow_states_routes_team_filter_to_client() {
    let client = Arc::new(TestLinearClient::default());
    let service = enabled_service(client.clone()).await;

    let states = service.list_workflow_states(Some("team-1")).await.unwrap();

    assert_eq!(states.len(), 1);
    assert_eq!(states[0].category, "in_progress");
    assert_eq!(
        client.list_workflow_states_team.lock().await.as_slice(),
        &[Some("team-1".to_string())]
    );
}

#[tokio::test]
async fn current_user_routes_to_client() {
    let client = Arc::new(TestLinearClient::default());
    let service = enabled_service(client.clone()).await;

    let user = service.current_user().await.unwrap();

    assert_eq!(user.id, "viewer-id");
    assert_eq!(*client.current_user_calls.lock().await, 1);
}

#[tokio::test]
async fn update_issue_state_routes_arguments_to_client() {
    let client = Arc::new(TestLinearClient::default());
    let service = enabled_service(client.clone()).await;

    service
        .update_issue_state("issue-1", "state-done")
        .await
        .unwrap();

    assert_eq!(
        client.update_issue_state_calls.lock().await.as_slice(),
        &[("issue-1".to_string(), "state-done".to_string())]
    );
}

#[tokio::test]
async fn assign_issue_to_current_user_routes_to_client() {
    let client = Arc::new(TestLinearClient::default());
    let service = enabled_service(client.clone()).await;

    let user = service
        .assign_issue_to_current_user("issue-1")
        .await
        .unwrap();

    assert_eq!(user.id, "viewer-id");
    assert_eq!(
        client.assign_calls.lock().await.as_slice(),
        &["issue-1".to_string()]
    );
}

#[tokio::test]
async fn clear_issue_assignee_routes_to_client() {
    let client = Arc::new(TestLinearClient::default());
    let service = enabled_service(client.clone()).await;

    service.clear_issue_assignee("issue-1").await.unwrap();

    assert_eq!(
        client.clear_assignee_calls.lock().await.as_slice(),
        &["issue-1".to_string()]
    );
}

#[tokio::test]
async fn create_comment_routes_body_to_client() {
    let client = Arc::new(TestLinearClient::default());
    let service = enabled_service(client.clone()).await;

    let comment = service
        .create_comment("issue-1", "Ready for review")
        .await
        .unwrap();

    assert_eq!(comment.body, "Ready for review");
    assert_eq!(
        client.create_comment_calls.lock().await.as_slice(),
        &[("issue-1".to_string(), "Ready for review".to_string())]
    );
}

#[tokio::test]
async fn list_issue_team_labels_routes_to_client() {
    let client = Arc::new(TestLinearClient::default());
    *client.team_labels.lock().await = team_labels();
    let service = enabled_service(client.clone()).await;

    let labels = service.list_issue_team_labels("issue-1").await.unwrap();

    assert_eq!(labels, team_labels());
    assert_eq!(
        client.list_issue_team_labels_calls.lock().await.as_slice(),
        &["issue-1".to_string()]
    );
}

#[tokio::test]
async fn set_issue_labels_resolves_names_and_updates_with_ids() {
    let client = Arc::new(TestLinearClient::default());
    *client.team_labels.lock().await = team_labels();
    let service = enabled_service(client.clone()).await;

    service
        .set_issue_labels("issue-1", vec!["Bug".to_string(), "bug".to_string()])
        .await
        .unwrap();

    // The team labels are fetched, the duplicate desired name resolves once, and
    // the resolved id is forwarded to the label-update call.
    assert_eq!(
        client.list_issue_team_labels_calls.lock().await.as_slice(),
        &["issue-1".to_string()]
    );
    assert_eq!(
        client.update_issue_labels_calls.lock().await.as_slice(),
        &[("issue-1".to_string(), vec!["label-bug".to_string()])]
    );
}

#[tokio::test]
async fn set_issue_labels_rejects_unknown_names_without_updating() {
    let client = Arc::new(TestLinearClient::default());
    *client.team_labels.lock().await = team_labels();
    let service = enabled_service(client.clone()).await;

    let error = service
        .set_issue_labels("issue-1", vec!["Nonexistent".to_string()])
        .await
        .unwrap_err();

    assert!(error.contains("Nonexistent"), "error should name the missing label: {error}");
    assert!(client.update_issue_labels_calls.lock().await.is_empty());
}
