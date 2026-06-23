use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use crate::application::clickup_integration_service::{
    ClickUpApiClient, ClickUpAuthContext, ClickUpComment, ClickUpIntegrationService, ClickUpSpace,
    ClickUpStatus, ClickUpTaskContent, ClickUpTaskSummary, ClickUpUser, ClickUpWorkspace,
    EmptyClickUpApiClient, UnavailableClickUpApiClient,
};
use crate::domain::integrations::{
    ClickUpIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::domain::services::{SecretStore, SecretStoreError};
use crate::infrastructure::memory::MemoryClickUpIntegrationSettingsRepository;

const TOKEN_REF_PREFIX: &str = "integrations/clickup/default/api-token";

/// In-memory `SecretStore` that records the keys it stores and deletes so tests
/// can assert the write → read-back → delete-prior token lifecycle.
#[derive(Default)]
struct RecordingSecretStore {
    secrets: RwLock<HashMap<String, String>>,
    deleted: Mutex<Vec<String>>,
}

impl RecordingSecretStore {
    async fn deleted_keys(&self) -> Vec<String> {
        self.deleted.lock().await.clone()
    }

    async fn stored(&self, key: &str) -> Option<String> {
        self.secrets.read().await.get(key).cloned()
    }

    async fn stored_count(&self) -> usize {
        self.secrets.read().await.len()
    }
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

/// `SecretStore` whose read-back always returns a different value than written,
/// to exercise the read-back-verification failure branch of `save_settings`.
#[derive(Default)]
struct MismatchingSecretStore {
    deleted: Mutex<Vec<String>>,
}

impl MismatchingSecretStore {
    async fn deleted_keys(&self) -> Vec<String> {
        self.deleted.lock().await.clone()
    }
}

#[async_trait]
impl SecretStore for MismatchingSecretStore {
    async fn put_secret(&self, _key: &str, _value: &str) -> Result<(), SecretStoreError> {
        Ok(())
    }

    async fn get_secret(&self, _key: &str) -> Result<Option<String>, SecretStoreError> {
        Ok(Some("a-different-value".to_string()))
    }

    async fn delete_secret(&self, key: &str) -> Result<(), SecretStoreError> {
        self.deleted.lock().await.push(key.to_string());
        Ok(())
    }
}

/// Configurable fake `ClickUpApiClient` recording calls and the auth token it
/// observed, so tests can prove the keychain round-trip feeds the client.
#[derive(Default)]
struct TestClickUpClient {
    /// Set once at construction; never mutated, so no lock is needed.
    validate_error: Option<String>,
    seen_tokens: Mutex<Vec<String>>,
    list_tasks_calls: Mutex<Vec<(String, Vec<String>)>>,
    list_spaces_calls: Mutex<Vec<String>>,
}

impl TestClickUpClient {
    fn with_validate_error(error: &str) -> Self {
        Self {
            validate_error: Some(error.to_string()),
            ..Default::default()
        }
    }

    async fn seen_tokens(&self) -> Vec<String> {
        self.seen_tokens.lock().await.clone()
    }

    async fn list_tasks_calls(&self) -> Vec<(String, Vec<String>)> {
        self.list_tasks_calls.lock().await.clone()
    }

    async fn list_spaces_calls(&self) -> Vec<String> {
        self.list_spaces_calls.lock().await.clone()
    }
}

#[async_trait]
impl ClickUpApiClient for TestClickUpClient {
    async fn validate(&self, auth: &ClickUpAuthContext) -> Result<(), String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        if let Some(error) = self.validate_error.clone() {
            Err(error)
        } else {
            Ok(())
        }
    }

    async fn list_workspaces(
        &self,
        auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        Ok(vec![ClickUpWorkspace {
            id: "9000".to_string(),
            name: "Acme".to_string(),
            color: Some("#ff6b35".to_string()),
        }])
    }

    async fn list_spaces(
        &self,
        auth: &ClickUpAuthContext,
        team_id: &str,
    ) -> Result<Vec<ClickUpSpace>, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.list_spaces_calls.lock().await.push(team_id.to_string());
        Ok(vec![ClickUpSpace {
            id: "space-1".to_string(),
            name: "Engineering".to_string(),
            private: false,
        }])
    }

    async fn list_tasks(
        &self,
        auth: &ClickUpAuthContext,
        team_id: &str,
        space_ids: &[String],
    ) -> Result<Vec<ClickUpTaskSummary>, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.list_tasks_calls
            .lock()
            .await
            .push((team_id.to_string(), space_ids.to_vec()));
        Ok(vec![ClickUpTaskSummary {
            id: "abc123".to_string(),
            custom_id: None,
            name: "Fix login".to_string(),
            url: Some("https://app.clickup.com/t/abc123".to_string()),
            status_name: Some("in progress".to_string()),
            status_type: Some("custom".to_string()),
            status_category: Some("in_progress".to_string()),
            status_color: Some("#abc".to_string()),
            assignees: vec!["dev".to_string()],
            tags: vec!["bug".to_string()],
            space_id: Some("space-1".to_string()),
            list_name: Some("Sprint".to_string()),
            updated_at: Some("1700000000000".to_string()),
        }])
    }

    async fn fetch_task(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        Ok(ClickUpTaskContent {
            id: task_id.to_string(),
            custom_id: None,
            name: "Fix login".to_string(),
            url: None,
            description: "body".to_string(),
            status_name: Some("in progress".to_string()),
            status_type: Some("custom".to_string()),
            status_category: Some("in_progress".to_string()),
            creator: Some("dev".to_string()),
            assignees: Vec::new(),
            tags: Vec::new(),
            comments: Vec::new(),
            updated_at: None,
            space_id: Some("space-1".to_string()),
            list_name: Some("Sprint".to_string()),
        })
    }

    async fn list_statuses(
        &self,
        auth: &ClickUpAuthContext,
        _space_id: &str,
    ) -> Result<Vec<ClickUpStatus>, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        Ok(vec![ClickUpStatus {
            id: None,
            status: "in progress".to_string(),
            status_type: "custom".to_string(),
            category: "in_progress".to_string(),
            color: None,
            orderindex: Some(1),
        }])
    }

    async fn current_user(&self, auth: &ClickUpAuthContext) -> Result<ClickUpUser, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        Ok(ClickUpUser {
            id: 42,
            username: Some("dev".to_string()),
            email: None,
        })
    }

    async fn create_comment(
        &self,
        auth: &ClickUpAuthContext,
        _task_id: &str,
        body_markdown: &str,
    ) -> Result<ClickUpComment, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        Ok(ClickUpComment {
            id: "comment-1".to_string(),
            body: body_markdown.to_string(),
            author_id: Some(42),
            author_name: Some("dev".to_string()),
            created_at: None,
        })
    }
}

fn build_service(
    client: Arc<dyn ClickUpApiClient>,
) -> (
    ClickUpIntegrationService,
    Arc<MemoryClickUpIntegrationSettingsRepository>,
    Arc<RecordingSecretStore>,
) {
    let repo = Arc::new(MemoryClickUpIntegrationSettingsRepository::new());
    let secret = Arc::new(RecordingSecretStore::default());
    let service = ClickUpIntegrationService::new(repo.clone(), secret.clone(), client);
    (service, repo, secret)
}

#[tokio::test]
async fn save_settings_writes_token_to_keychain_and_stores_only_ref() {
    let (service, repo, secret) = build_service(Arc::new(EmptyClickUpApiClient));

    let settings = service
        .save_settings(Some("pk_secret_token".to_string()), Some("9000".to_string()))
        .await
        .expect("save should succeed");

    let secret_ref = settings
        .token_secret_ref
        .clone()
        .expect("token ref should be set");
    assert!(secret_ref.starts_with(TOKEN_REF_PREFIX), "ref={secret_ref}");
    assert_eq!(settings.workspace_id.as_deref(), Some("9000"));
    assert_eq!(settings.validation_status, IntegrationValidationStatus::Pending);
    assert!(!settings.enabled);
    assert!(!settings.task_search_available);

    // The raw token lives only in the keychain under the ref, never in the DB row.
    assert_eq!(
        secret.stored(&secret_ref).await.as_deref(),
        Some("pk_secret_token")
    );
    let persisted = repo.get().await.unwrap();
    assert_eq!(persisted.token_secret_ref.as_deref(), Some(secret_ref.as_str()));
}

#[tokio::test]
async fn save_settings_replacing_token_deletes_previous_ref() {
    let (service, _repo, secret) = build_service(Arc::new(EmptyClickUpApiClient));

    let first = service
        .save_settings(Some("first-token".to_string()), None)
        .await
        .unwrap();
    let first_ref = first.token_secret_ref.unwrap();

    let second = service
        .save_settings(Some("second-token".to_string()), None)
        .await
        .unwrap();
    let second_ref = second.token_secret_ref.unwrap();

    assert_ne!(first_ref, second_ref);
    assert!(secret.deleted_keys().await.contains(&first_ref));
    assert_eq!(secret.stored(&first_ref).await, None);
    assert_eq!(secret.stored(&second_ref).await.as_deref(), Some("second-token"));
    assert_eq!(secret.stored_count().await, 1);
}

#[tokio::test]
async fn save_settings_read_back_mismatch_errors_and_deletes_written_ref() {
    let repo = Arc::new(MemoryClickUpIntegrationSettingsRepository::new());
    let secret = Arc::new(MismatchingSecretStore::default());
    let service =
        ClickUpIntegrationService::new(repo.clone(), secret.clone(), Arc::new(EmptyClickUpApiClient));

    let result = service
        .save_settings(Some("pk_token".to_string()), None)
        .await;

    assert!(result.is_err(), "mismatched read-back must fail");
    // The just-written ref is cleaned up; no ref persisted.
    assert_eq!(secret.deleted_keys().await.len(), 1);
    assert_eq!(repo.get().await.unwrap().token_secret_ref, None);
}

#[tokio::test]
async fn save_settings_with_empty_token_clears_ref() {
    let (service, repo, secret) = build_service(Arc::new(EmptyClickUpApiClient));

    let saved = service
        .save_settings(Some("pk_token".to_string()), None)
        .await
        .unwrap();
    let secret_ref = saved.token_secret_ref.unwrap();

    let cleared = service
        .save_settings(Some(String::new()), None)
        .await
        .unwrap();

    assert_eq!(cleared.token_secret_ref, None);
    assert_eq!(
        cleared.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(secret.deleted_keys().await.contains(&secret_ref));
    assert_eq!(repo.get().await.unwrap().token_secret_ref, None);
}

#[tokio::test]
async fn validate_and_enable_marks_valid_when_client_ok() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_valid".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    let settings = service.validate_and_enable().await.unwrap();

    assert!(settings.enabled);
    assert_eq!(settings.validation_status, IntegrationValidationStatus::Valid);
    assert!(settings.task_search_available);
    assert!(settings.last_error.is_none());
    assert!(settings.last_validated_at.is_some());
    // The token from the keychain was handed to the client verbatim.
    assert_eq!(client.seen_tokens().await, vec!["pk_valid".to_string()]);
}

#[tokio::test]
async fn validate_and_enable_marks_invalid_when_client_errors() {
    let client = Arc::new(TestClickUpClient::with_validate_error("invalid token"));
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_bad".to_string()), None)
        .await
        .unwrap();
    let settings = service.validate_and_enable().await.unwrap();

    assert!(!settings.enabled);
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert!(!settings.task_search_available);
    assert_eq!(settings.last_error.as_deref(), Some("invalid token"));
}

#[tokio::test]
async fn validate_and_enable_requires_a_saved_token() {
    let (service, _repo, _secret) = build_service(Arc::new(EmptyClickUpApiClient));

    let result = service.validate_and_enable().await;

    assert!(result.is_err(), "validating without a token must fail");
}

#[tokio::test]
async fn disconnect_clears_secret_and_resets_settings() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, repo, secret) = build_service(client);

    let saved = service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    let secret_ref = saved.token_secret_ref.unwrap();
    service.validate_and_enable().await.unwrap();

    let cleared = service.disconnect().await.unwrap();

    // Reset to defaults (compared field-by-field; `updated_at` is set fresh).
    assert!(!cleared.enabled);
    assert_eq!(cleared.token_secret_ref, None);
    assert_eq!(cleared.workspace_id, None);
    assert!(!cleared.task_search_available);
    assert_eq!(cleared.last_validated_at, None);
    assert_eq!(cleared.last_error, None);
    assert_eq!(
        cleared.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(secret.deleted_keys().await.contains(&secret_ref));
    assert_eq!(repo.get().await.unwrap().token_secret_ref, None);
}

#[tokio::test]
async fn list_tasks_errors_when_not_enabled() {
    let (service, _repo, _secret) = build_service(Arc::new(EmptyClickUpApiClient));

    // Token saved but not validated/enabled yet.
    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();

    let result = service.list_tasks(vec!["space-1".to_string()]).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not enabled"));
}

#[tokio::test]
async fn list_tasks_errors_when_workspace_missing() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, _repo, _secret) = build_service(client);

    // Enabled, but no workspace selected.
    service
        .save_settings(Some("pk_token".to_string()), None)
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let result = service.list_tasks(vec!["space-1".to_string()]).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("workspace is not selected"));
}

#[tokio::test]
async fn list_spaces_and_tasks_succeed_when_enabled_with_workspace() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let spaces = service.list_spaces().await.unwrap();
    assert_eq!(spaces.len(), 1);
    assert_eq!(spaces[0].id, "space-1");
    assert_eq!(client.list_spaces_calls().await, vec!["9000".to_string()]);

    let tasks = service.list_tasks(vec!["space-1".to_string()]).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "abc123");
    assert_eq!(tasks[0].status_category.as_deref(), Some("in_progress"));
    assert_eq!(
        client.list_tasks_calls().await,
        vec![("9000".to_string(), vec!["space-1".to_string()])]
    );
}

#[tokio::test]
async fn create_comment_passes_through_when_enabled() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, _repo, _secret) = build_service(client);

    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let comment = service.create_comment("abc123", "looks good").await.unwrap();
    assert_eq!(comment.body, "looks good");
}

#[tokio::test]
async fn unavailable_client_reports_reason() {
    let client = Arc::new(UnavailableClickUpApiClient::new("ClickUp HTTP client unavailable"));
    let (service, _repo, _secret) = build_service(client);

    service
        .save_settings(Some("pk_token".to_string()), None)
        .await
        .unwrap();
    let settings = service.validate_and_enable().await.unwrap();

    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        settings.last_error.as_deref(),
        Some("ClickUp HTTP client unavailable")
    );
}
