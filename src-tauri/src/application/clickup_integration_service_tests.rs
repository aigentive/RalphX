use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use crate::application::clickup_integration_service::{
    ClickUpApiClient, ClickUpAuthContext, ClickUpComment, ClickUpFolder, ClickUpIntegrationService,
    ClickUpList, ClickUpSpace, ClickUpStatus, ClickUpTaskContent, ClickUpTaskListOptions,
    ClickUpTaskSummary, ClickUpUser, ClickUpWorkspace, EmptyClickUpApiClient,
    UnavailableClickUpApiClient,
};
use crate::application::integration_reference_expansion::{
    SkippedIntegrationReferenceReason, MAX_INTEGRATION_REFERENCES,
};
use crate::domain::integrations::{
    ClickUpIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::domain::services::{ComposerIntegrationReference, SecretStore, SecretStoreError};
use crate::infrastructure::memory::MemoryClickUpIntegrationSettingsRepository;

const TOKEN_REF_PREFIX: &str = "integrations/clickup/default/api-token";

fn clickup_reference(id: impl Into<String>) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "clickup".to_string(),
        kind: "task".to_string(),
        id: id.into(),
        key: None,
        title: None,
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    }
}

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
    list_folders_calls: Mutex<Vec<String>>,
    list_folder_lists_calls: Mutex<Vec<String>>,
    list_folderless_lists_calls: Mutex<Vec<String>>,
    list_tasks_for_list_calls: Mutex<Vec<(String, Vec<i64>)>>,
    fetch_task_calls: Mutex<Vec<String>>,
    fetch_task_failures: Mutex<Vec<String>>,
    custom_task_calls: Mutex<Vec<(String, String)>>,
    custom_task_failures: Mutex<Vec<String>>,
    status_updates: Mutex<Vec<(String, String)>>,
    assignments: Mutex<Vec<String>>,
    cleared_assignees: Mutex<Vec<String>>,
    tag_updates: Mutex<Vec<(String, Vec<String>)>>,
}

impl TestClickUpClient {
    fn with_validate_error(error: &str) -> Self {
        Self {
            validate_error: Some(error.to_string()),
            ..Default::default()
        }
    }

    fn with_fetch_task_failure(task_id: &str) -> Self {
        Self {
            fetch_task_failures: Mutex::new(vec![task_id.to_string()]),
            ..Default::default()
        }
    }

    fn with_fetch_and_custom_task_failure(task_id: &str) -> Self {
        Self {
            fetch_task_failures: Mutex::new(vec![task_id.to_string()]),
            custom_task_failures: Mutex::new(vec![task_id.to_string()]),
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

    async fn list_folders_calls(&self) -> Vec<String> {
        self.list_folders_calls.lock().await.clone()
    }

    async fn list_folder_lists_calls(&self) -> Vec<String> {
        self.list_folder_lists_calls.lock().await.clone()
    }

    async fn list_folderless_lists_calls(&self) -> Vec<String> {
        self.list_folderless_lists_calls.lock().await.clone()
    }

    async fn list_tasks_for_list_calls(&self) -> Vec<(String, Vec<i64>)> {
        self.list_tasks_for_list_calls.lock().await.clone()
    }

    async fn fetch_task_calls(&self) -> Vec<String> {
        self.fetch_task_calls.lock().await.clone()
    }

    async fn custom_task_calls(&self) -> Vec<(String, String)> {
        self.custom_task_calls.lock().await.clone()
    }

    async fn status_updates(&self) -> Vec<(String, String)> {
        self.status_updates.lock().await.clone()
    }

    async fn assignments(&self) -> Vec<String> {
        self.assignments.lock().await.clone()
    }

    async fn cleared_assignees(&self) -> Vec<String> {
        self.cleared_assignees.lock().await.clone()
    }

    async fn tag_updates(&self) -> Vec<(String, Vec<String>)> {
        self.tag_updates.lock().await.clone()
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
        self.list_spaces_calls
            .lock()
            .await
            .push(team_id.to_string());
        Ok(vec![ClickUpSpace {
            id: "space-1".to_string(),
            name: "Engineering".to_string(),
            private: false,
        }])
    }

    async fn list_folders(
        &self,
        auth: &ClickUpAuthContext,
        space_id: &str,
    ) -> Result<Vec<ClickUpFolder>, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.list_folders_calls
            .lock()
            .await
            .push(space_id.to_string());
        Ok(vec![ClickUpFolder {
            id: "folder-1".to_string(),
            name: "Folder".to_string(),
            space_id: Some(space_id.to_string()),
        }])
    }

    async fn list_folder_lists(
        &self,
        auth: &ClickUpAuthContext,
        folder_id: &str,
    ) -> Result<Vec<ClickUpList>, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.list_folder_lists_calls
            .lock()
            .await
            .push(folder_id.to_string());
        Ok(vec![ClickUpList {
            id: "list-folder".to_string(),
            name: "Folder List".to_string(),
            folder_id: Some(folder_id.to_string()),
            space_id: Some("space-1".to_string()),
        }])
    }

    async fn list_folderless_lists(
        &self,
        auth: &ClickUpAuthContext,
        space_id: &str,
    ) -> Result<Vec<ClickUpList>, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.list_folderless_lists_calls
            .lock()
            .await
            .push(space_id.to_string());
        Ok(vec![ClickUpList {
            id: "list-space".to_string(),
            name: "Space List".to_string(),
            folder_id: None,
            space_id: Some(space_id.to_string()),
        }])
    }

    async fn list_tasks(
        &self,
        auth: &ClickUpAuthContext,
        team_id: &str,
        space_ids: &[String],
        _options: ClickUpTaskListOptions,
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
            assignee_ids: vec![42],
            watchers: Vec::new(),
            tags: vec!["bug".to_string()],
            sprint_names: Vec::new(),
            location_ids: Vec::new(),
            location_folder_ids: Vec::new(),
            location_space_ids: Vec::new(),
            space_id: Some("space-1".to_string()),
            folder_id: None,
            list_id: None,
            list_name: Some("Sprint".to_string()),
            updated_at: Some("1700000000000".to_string()),
        }])
    }

    async fn list_tasks_for_list(
        &self,
        auth: &ClickUpAuthContext,
        list_id: &str,
        options: ClickUpTaskListOptions,
    ) -> Result<Vec<ClickUpTaskSummary>, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.list_tasks_for_list_calls
            .lock()
            .await
            .push((list_id.to_string(), options.assignee_ids));
        Ok(vec![ClickUpTaskSummary {
            id: "list-task".to_string(),
            custom_id: None,
            name: "List scoped task".to_string(),
            url: None,
            status_name: Some("todo".to_string()),
            status_type: Some("open".to_string()),
            status_category: Some("todo".to_string()),
            status_color: None,
            assignees: Vec::new(),
            assignee_ids: Vec::new(),
            watchers: Vec::new(),
            tags: Vec::new(),
            sprint_names: Vec::new(),
            location_ids: vec![list_id.to_string()],
            location_folder_ids: Vec::new(),
            location_space_ids: Vec::new(),
            space_id: Some("space-1".to_string()),
            folder_id: None,
            list_id: Some(list_id.to_string()),
            list_name: Some("Space List".to_string()),
            updated_at: None,
        }])
    }

    async fn fetch_task(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.fetch_task_calls.lock().await.push(task_id.to_string());
        if self
            .fetch_task_failures
            .lock()
            .await
            .iter()
            .any(|value| value == task_id)
        {
            return Err("ClickUp returned HTTP 404".to_string());
        }
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
            watchers: Vec::new(),
            tags: Vec::new(),
            comments: Vec::new(),
            attachments: Vec::new(),
            updated_at: None,
            space_id: Some("space-1".to_string()),
            list_name: Some("Sprint".to_string()),
        })
    }

    async fn fetch_task_by_custom_id(
        &self,
        auth: &ClickUpAuthContext,
        team_id: &str,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.custom_task_calls
            .lock()
            .await
            .push((team_id.to_string(), task_id.to_string()));
        if self
            .custom_task_failures
            .lock()
            .await
            .iter()
            .any(|value| value == task_id)
        {
            return Err("ClickUp custom lookup returned HTTP 404".to_string());
        }
        Ok(ClickUpTaskContent {
            id: "opaque-from-custom".to_string(),
            custom_id: Some(task_id.to_string()),
            name: "Fix login".to_string(),
            url: Some(format!("https://app.clickup.com/t/{team_id}/{task_id}")),
            description: "body".to_string(),
            status_name: Some("in progress".to_string()),
            status_type: Some("custom".to_string()),
            status_category: Some("in_progress".to_string()),
            creator: Some("dev".to_string()),
            assignees: Vec::new(),
            watchers: Vec::new(),
            tags: Vec::new(),
            comments: Vec::new(),
            attachments: Vec::new(),
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
            attachments: Vec::new(),
            replies: Vec::new(),
        })
    }

    async fn update_task_status(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
        status_name: &str,
    ) -> Result<(), String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.status_updates
            .lock()
            .await
            .push((task_id.to_string(), status_name.to_string()));
        Ok(())
    }

    async fn assign_task_to_current_user(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpUser, String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.assignments.lock().await.push(task_id.to_string());
        Ok(ClickUpUser {
            id: 42,
            username: Some("dev".to_string()),
            email: Some("dev@example.com".to_string()),
        })
    }

    async fn clear_task_assignee(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<(), String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.cleared_assignees
            .lock()
            .await
            .push(task_id.to_string());
        Ok(())
    }

    async fn set_task_tags(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
        tags: Vec<String>,
    ) -> Result<(), String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.tag_updates
            .lock()
            .await
            .push((task_id.to_string(), tags));
        Ok(())
    }
}

struct MinimalClickUpClient;

#[async_trait]
impl ClickUpApiClient for MinimalClickUpClient {
    async fn validate(&self, _auth: &ClickUpAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_workspaces(
        &self,
        _auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        Ok(Vec::new())
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
async fn prompt_expansion_renders_clickup_task_and_reports_zero_budget() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, _repo, _secret) = build_service(client);
    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();
    let reference = ComposerIntegrationReference {
        provider: "clickup".to_string(),
        kind: "task".to_string(),
        id: "task-1".to_string(),
        key: None,
        title: None,
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    };

    let rendered = service
        .expand_references_for_prompt_with_budget("Base", std::slice::from_ref(&reference), 4096)
        .await;
    assert!(rendered.rewritten_prompt.contains("<clickup_task"));
    assert!(rendered.rewritten_prompt.contains("Fix login"));
    assert!(rendered.skipped_references.is_empty());

    let exhausted = service
        .expand_references_for_prompt_with_budget("Base", &[reference], 0)
        .await;
    assert_eq!(exhausted.rewritten_prompt, "Base");
    assert_eq!(exhausted.skipped_references.len(), 1);
    assert_eq!(
        exhausted.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::BudgetExceeded
    );
}

#[tokio::test]
async fn budgeted_expansion_reports_typed_budget_auth_and_fetch_skips() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, repo, secret) = build_service(client.clone());
    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();
    let reference = clickup_reference("task-1");

    let zero_budget = service
        .expand_references_for_prompt_with_budget("Base", std::slice::from_ref(&reference), 0)
        .await;
    assert_eq!(zero_budget.rewritten_prompt, "Base");
    assert_eq!(
        zero_budget.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::BudgetExceeded
    );

    let capped_references = (0..=MAX_INTEGRATION_REFERENCES)
        .map(|index| clickup_reference(format!("task-{index}")))
        .collect::<Vec<_>>();
    let capped = service
        .expand_references_for_prompt_with_budget("Base", &capped_references, 16 * 1024)
        .await;
    assert!(capped.rewritten_prompt.contains("<clickup_task"));
    assert!(capped.skipped_references.iter().any(|skipped| {
        skipped.id == format!("task-{MAX_INTEGRATION_REFERENCES}")
            && skipped.reason == SkippedIntegrationReferenceReason::BudgetExceeded
    }));

    let one = service
        .expand_references_for_prompt_with_budget(
            "Base",
            std::slice::from_ref(&reference),
            16 * 1024,
        )
        .await;
    let one_reference_budget = one.rewritten_prompt.len() - "Base".len();
    let starved = service
        .expand_references_for_prompt_with_budget(
            "Base",
            &[reference.clone(), clickup_reference("task-2")],
            one_reference_budget,
        )
        .await;
    assert!(starved.rewritten_prompt.contains("task-1"));
    assert!(starved.skipped_references.iter().any(|skipped| {
        skipped.id == "task-2"
            && skipped.reason == SkippedIntegrationReferenceReason::BudgetExceeded
    }));

    let (disabled_service, _disabled_repo, _disabled_secret) =
        build_service(Arc::new(TestClickUpClient::default()));
    let disabled = disabled_service
        .expand_references_for_prompt_with_budget("Base", std::slice::from_ref(&reference), 4096)
        .await;
    assert_eq!(
        disabled.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::IntegrationDisabled
    );

    let settings = repo.get().await.unwrap();
    secret
        .delete_secret(settings.token_secret_ref.as_deref().unwrap())
        .await
        .unwrap();
    let missing_credentials = service
        .expand_references_for_prompt_with_budget("Base", std::slice::from_ref(&reference), 4096)
        .await;
    assert_eq!(
        missing_credentials.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::MissingCredentials
    );

    let failing_client = Arc::new(TestClickUpClient::with_fetch_task_failure("123456789"));
    let (failing_service, _failing_repo, _failing_secret) = build_service(failing_client);
    failing_service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    failing_service.validate_and_enable().await.unwrap();
    let fetch_failure = failing_service
        .expand_references_for_prompt_with_budget("Base", &[clickup_reference("123456789")], 4096)
        .await;
    assert_eq!(
        fetch_failure.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::ApiError
    );
}

#[tokio::test]
async fn save_settings_writes_token_to_keychain_and_stores_only_ref() {
    let (service, repo, secret) = build_service(Arc::new(EmptyClickUpApiClient));

    let settings = service
        .save_settings(
            Some("pk_secret_token".to_string()),
            Some("9000".to_string()),
        )
        .await
        .expect("save should succeed");

    let secret_ref = settings
        .token_secret_ref
        .clone()
        .expect("token ref should be set");
    assert!(secret_ref.starts_with(TOKEN_REF_PREFIX));
    assert_eq!(settings.workspace_id.as_deref(), Some("9000"));
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::Pending
    );
    assert!(!settings.enabled);
    assert!(!settings.task_search_available);

    // The raw token lives only in the keychain under the ref, never in the DB row.
    assert_eq!(
        secret.stored(&secret_ref).await.as_deref(),
        Some("pk_secret_token")
    );
    let persisted = repo.get().await.unwrap();
    assert_eq!(
        persisted.token_secret_ref.as_deref(),
        Some(secret_ref.as_str())
    );
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
    assert_eq!(
        secret.stored(&second_ref).await.as_deref(),
        Some("second-token")
    );
    assert_eq!(secret.stored_count().await, 1);
}

#[tokio::test]
async fn save_settings_workspace_only_preserves_valid_connection() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, _repo, _secret) = build_service(client);

    service
        .save_settings(Some("pk_valid".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    let validated = service.validate_and_enable().await.unwrap();
    assert!(validated.enabled);
    assert_eq!(
        validated.validation_status,
        IntegrationValidationStatus::Valid
    );
    assert!(validated.task_search_available);

    let updated = service
        .save_settings(None, Some("9001".to_string()))
        .await
        .unwrap();

    assert_eq!(updated.workspace_id.as_deref(), Some("9001"));
    assert!(updated.enabled);
    assert_eq!(
        updated.validation_status,
        IntegrationValidationStatus::Valid
    );
    assert!(updated.task_search_available);
    assert!(updated.last_validated_at.is_some());
}

#[tokio::test]
async fn save_settings_read_back_mismatch_errors_and_deletes_written_ref() {
    let repo = Arc::new(MemoryClickUpIntegrationSettingsRepository::new());
    let secret = Arc::new(MismatchingSecretStore::default());
    let service = ClickUpIntegrationService::new(
        repo.clone(),
        secret.clone(),
        Arc::new(EmptyClickUpApiClient),
    );

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
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::Valid
    );
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

    let result = service
        .list_tasks(
            vec!["space-1".to_string()],
            ClickUpTaskListOptions::default(),
        )
        .await;
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

    let result = service
        .list_tasks(
            vec!["space-1".to_string()],
            ClickUpTaskListOptions::default(),
        )
        .await;
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

    let tasks = service
        .list_tasks(
            vec!["space-1".to_string()],
            ClickUpTaskListOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "abc123");
    assert_eq!(tasks[0].status_category.as_deref(), Some("in_progress"));
    assert_eq!(
        client.list_tasks_calls().await,
        vec![("9000".to_string(), vec!["space-1".to_string()])]
    );
}

#[tokio::test]
async fn enabled_service_passes_through_clickup_hierarchy_and_list_tasks() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let folders = service.list_folders("space-1").await.unwrap();
    let folder_lists = service.list_folder_lists("folder-1").await.unwrap();
    let folderless_lists = service.list_folderless_lists("space-1").await.unwrap();
    let list_tasks = service
        .list_tasks_for_list(
            "list-space",
            ClickUpTaskListOptions {
                query: Some("current".to_string()),
                limit: Some(25),
                assignee_ids: vec![42],
            },
        )
        .await
        .unwrap();

    assert_eq!(folders[0].id, "folder-1");
    assert_eq!(folder_lists[0].folder_id.as_deref(), Some("folder-1"));
    assert_eq!(folderless_lists[0].space_id.as_deref(), Some("space-1"));
    assert_eq!(list_tasks[0].list_id.as_deref(), Some("list-space"));
    assert_eq!(
        client.list_folders_calls().await,
        vec!["space-1".to_string()]
    );
    assert_eq!(
        client.list_folder_lists_calls().await,
        vec!["folder-1".to_string()]
    );
    assert_eq!(
        client.list_folderless_lists_calls().await,
        vec!["space-1".to_string()]
    );
    assert_eq!(
        client.list_tasks_for_list_calls().await,
        vec![("list-space".to_string(), vec![42])]
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

    let comment = service
        .create_comment("abc123", "looks good")
        .await
        .unwrap();
    assert_eq!(comment.body, "looks good");
}

#[tokio::test]
async fn enabled_service_passes_through_task_detail_user_status_assignment_and_tags() {
    let client = Arc::new(TestClickUpClient::default());
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let detail = service.fetch_task("abc123").await.unwrap();
    assert_eq!(detail.id, "abc123");
    assert_eq!(detail.description, "body");

    let statuses = service.list_statuses("space-1").await.unwrap();
    assert_eq!(statuses[0].status, "in progress");

    let current_user = service.current_user().await.unwrap();
    assert_eq!(current_user.username.as_deref(), Some("dev"));

    service.update_task_status("abc123", "done").await.unwrap();
    let assigned = service.assign_task_to_current_user("abc123").await.unwrap();
    assert_eq!(assigned.email.as_deref(), Some("dev@example.com"));
    service.clear_task_assignee("abc123").await.unwrap();
    service
        .set_task_tags("abc123", vec!["bug".to_string(), "backend".to_string()])
        .await
        .unwrap();

    assert_eq!(
        client.status_updates().await,
        vec![("abc123".to_string(), "done".to_string())]
    );
    assert_eq!(client.assignments().await, vec!["abc123".to_string()]);
    assert_eq!(client.cleared_assignees().await, vec!["abc123".to_string()]);
    assert_eq!(
        client.tag_updates().await,
        vec![(
            "abc123".to_string(),
            vec!["bug".to_string(), "backend".to_string()]
        )]
    );
}

#[tokio::test]
async fn fetch_task_retries_custom_id_lookup_with_selected_workspace() {
    let client = Arc::new(TestClickUpClient::with_fetch_task_failure("TASK-123"));
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let detail = service.fetch_task("TASK-123").await.unwrap();

    assert_eq!(detail.id, "opaque-from-custom");
    assert_eq!(detail.custom_id.as_deref(), Some("TASK-123"));
    assert_eq!(
        client.fetch_task_calls().await,
        vec!["TASK-123".to_string()]
    );
    assert_eq!(
        client.custom_task_calls().await,
        vec![("9000".to_string(), "TASK-123".to_string())]
    );
}

#[tokio::test]
async fn fetch_task_skips_custom_id_lookup_without_selected_workspace() {
    let client = Arc::new(TestClickUpClient::with_fetch_task_failure("TASK-123"));
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_token".to_string()), None)
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let error = service.fetch_task("TASK-123").await.unwrap_err();

    assert_eq!(error, "ClickUp returned HTTP 404");
    assert_eq!(
        client.fetch_task_calls().await,
        vec!["TASK-123".to_string()]
    );
    assert!(client.custom_task_calls().await.is_empty());
}

#[tokio::test]
async fn fetch_task_skips_custom_id_lookup_for_opaque_ids() {
    let client = Arc::new(TestClickUpClient::with_fetch_task_failure("123456789"));
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let error = service.fetch_task("123456789").await.unwrap_err();

    assert_eq!(error, "ClickUp returned HTTP 404");
    assert_eq!(
        client.fetch_task_calls().await,
        vec!["123456789".to_string()]
    );
    assert!(client.custom_task_calls().await.is_empty());
}

#[tokio::test]
async fn fetch_task_reports_custom_id_lookup_failure() {
    let client = Arc::new(TestClickUpClient::with_fetch_and_custom_task_failure(
        "TASK-404",
    ));
    let (service, _repo, _secret) = build_service(client.clone());

    service
        .save_settings(Some("pk_token".to_string()), Some("9000".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let error = service.fetch_task("TASK-404").await.unwrap_err();

    assert_eq!(
        error,
        "ClickUp returned HTTP 404; ClickUp custom task id lookup also failed: ClickUp custom lookup returned HTTP 404"
    );
    assert_eq!(
        client.custom_task_calls().await,
        vec![("9000".to_string(), "TASK-404".to_string())]
    );
}

#[tokio::test]
async fn empty_client_covers_minimal_success_paths() {
    let client = EmptyClickUpApiClient;
    let auth = ClickUpAuthContext {
        api_token: "pk_test".to_string(),
    };

    client.validate(&auth).await.unwrap();
    assert!(client.list_workspaces(&auth).await.unwrap().is_empty());
    assert!(client.list_spaces(&auth, "9000").await.unwrap().is_empty());
    assert!(client
        .list_tasks(
            &auth,
            "9000",
            &["space-1".to_string()],
            ClickUpTaskListOptions::default(),
        )
        .await
        .unwrap()
        .is_empty());
    assert!(client
        .list_statuses(&auth, "space-1")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(client.current_user(&auth).await.unwrap().id, 0);
    client
        .update_task_status(&auth, "abc123", "done")
        .await
        .unwrap();
    assert_eq!(
        client
            .assign_task_to_current_user(&auth, "abc123")
            .await
            .unwrap()
            .username
            .as_deref(),
        Some("Test User")
    );
    client.clear_task_assignee(&auth, "abc123").await.unwrap();
    assert_eq!(
        client
            .create_comment(&auth, "abc123", "body")
            .await
            .unwrap()
            .body,
        "body"
    );
    client
        .set_task_tags(&auth, "abc123", vec!["tag".to_string()])
        .await
        .unwrap();

    let task = client.fetch_task(&auth, "abc123").await.unwrap();
    assert_eq!(task.id, "abc123");
    assert!(task.comments.is_empty());
}

#[tokio::test]
async fn unavailable_client_returns_reason_from_all_methods() {
    let client = UnavailableClickUpApiClient::new("not available");
    let auth = ClickUpAuthContext {
        api_token: "pk_test".to_string(),
    };

    assert_eq!(client.validate(&auth).await.unwrap_err(), "not available");
    assert_eq!(
        client.list_workspaces(&auth).await.unwrap_err(),
        "not available"
    );
    assert_eq!(
        client.list_spaces(&auth, "9000").await.unwrap_err(),
        "not available"
    );
    assert_eq!(
        client
            .list_tasks(
                &auth,
                "9000",
                &["space-1".to_string()],
                ClickUpTaskListOptions::default(),
            )
            .await
            .unwrap_err(),
        "not available"
    );
    assert_eq!(
        client.fetch_task(&auth, "abc123").await.unwrap_err(),
        "not available"
    );
    assert_eq!(
        client.list_statuses(&auth, "space-1").await.unwrap_err(),
        "not available"
    );
    assert_eq!(
        client.current_user(&auth).await.unwrap_err(),
        "not available"
    );
    assert_eq!(
        client
            .update_task_status(&auth, "abc123", "done")
            .await
            .unwrap_err(),
        "not available"
    );
    assert_eq!(
        client
            .assign_task_to_current_user(&auth, "abc123")
            .await
            .unwrap_err(),
        "not available"
    );
    assert_eq!(
        client
            .clear_task_assignee(&auth, "abc123")
            .await
            .unwrap_err(),
        "not available"
    );
    assert_eq!(
        client
            .create_comment(&auth, "abc123", "body")
            .await
            .unwrap_err(),
        "not available"
    );
    assert_eq!(
        client
            .set_task_tags(&auth, "abc123", vec!["tag".to_string()])
            .await
            .unwrap_err(),
        "not available"
    );
}

#[tokio::test]
async fn trait_default_methods_report_unavailable_features() {
    let client = MinimalClickUpClient;
    let auth = ClickUpAuthContext {
        api_token: "pk_test".to_string(),
    };

    assert!(client
        .list_spaces(&auth, "9000")
        .await
        .unwrap_err()
        .contains("spaces are not available"));
    assert!(client
        .list_tasks(&auth, "9000", &[], ClickUpTaskListOptions::default())
        .await
        .unwrap_err()
        .contains("tasks are not available"));
    assert!(client
        .fetch_task(&auth, "abc123")
        .await
        .unwrap_err()
        .contains("task lookup is not available"));
    assert!(client
        .list_statuses(&auth, "space-1")
        .await
        .unwrap_err()
        .contains("statuses are not available"));
    assert!(client
        .current_user(&auth)
        .await
        .unwrap_err()
        .contains("current-user lookup is not available"));
    assert!(client
        .update_task_status(&auth, "abc123", "done")
        .await
        .unwrap_err()
        .contains("status updates are not available"));
    assert!(client
        .assign_task_to_current_user(&auth, "abc123")
        .await
        .unwrap_err()
        .contains("assignment is not available"));
    assert!(client
        .clear_task_assignee(&auth, "abc123")
        .await
        .unwrap_err()
        .contains("assignee clearing is not available"));
    assert!(client
        .create_comment(&auth, "abc123", "body")
        .await
        .unwrap_err()
        .contains("comments are not available"));
    assert!(client
        .set_task_tags(&auth, "abc123", vec!["tag".to_string()])
        .await
        .unwrap_err()
        .contains("tag updates are not available"));
}

#[tokio::test]
async fn unavailable_client_reports_reason() {
    let client = Arc::new(UnavailableClickUpApiClient::new(
        "ClickUp HTTP client unavailable",
    ));
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
