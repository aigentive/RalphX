use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use crate::application::granola_integration_service::{
    EmptyGranolaApiClient, GranolaApiClient, GranolaAuthContext, GranolaIntegrationService,
    UnavailableGranolaApiClient,
};
use crate::domain::integrations::IntegrationValidationStatus;
use crate::domain::services::{SecretStore, SecretStoreError};
use crate::infrastructure::memory::MemoryGranolaIntegrationSettingsRepository;

/// The single, stable keychain reference for the Granola token (no per-save UUID).
const TOKEN_REF: &str = "integrations/granola/default/api-token";

/// In-memory `SecretStore` that records the keys it stores and deletes so tests
/// can assert the write → read-back → clear token lifecycle.
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

/// Fake `GranolaApiClient` recording the auth token it observed, so tests can
/// prove the keychain round-trip feeds the client, plus a configurable error.
#[derive(Default)]
struct TestGranolaClient {
    validate_error: Option<String>,
    seen_tokens: Mutex<Vec<String>>,
}

impl TestGranolaClient {
    fn with_validate_error(error: &str) -> Self {
        Self {
            validate_error: Some(error.to_string()),
            ..Default::default()
        }
    }

    async fn seen_tokens(&self) -> Vec<String> {
        self.seen_tokens.lock().await.clone()
    }
}

#[async_trait]
impl GranolaApiClient for TestGranolaClient {
    async fn validate(&self, auth: &GranolaAuthContext) -> Result<(), String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        match &self.validate_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

fn service(
    secret_store: Arc<dyn SecretStore>,
    client: Arc<dyn GranolaApiClient>,
) -> GranolaIntegrationService {
    GranolaIntegrationService::new(
        Arc::new(MemoryGranolaIntegrationSettingsRepository::new()),
        secret_store,
        client,
    )
}

#[tokio::test]
async fn get_settings_returns_not_configured_defaults() {
    let svc = service(
        Arc::new(RecordingSecretStore::default()),
        Arc::new(EmptyGranolaApiClient),
    );

    let settings = svc.get_settings().await.unwrap();

    assert!(!settings.enabled);
    assert!(settings.token_secret_ref.is_none());
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
}

#[tokio::test]
async fn save_settings_stores_token_under_fixed_ref_and_returns_pending() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let svc = service(secrets.clone(), Arc::new(EmptyGranolaApiClient));

    let saved = svc
        .save_settings(Some("grn_live_token".to_string()))
        .await
        .unwrap();

    assert!(
        !saved.enabled,
        "saving a token returns a pending, not-enabled state"
    );
    assert_eq!(saved.validation_status, IntegrationValidationStatus::Pending);
    assert_eq!(saved.token_secret_ref.as_deref(), Some(TOKEN_REF));
    assert!(saved.last_validated_at.is_none());
    assert!(saved.last_error.is_none());

    // Raw token lives only in the keychain, under the fixed ref.
    assert_eq!(
        secrets.stored(TOKEN_REF).await.as_deref(),
        Some("grn_live_token")
    );
    assert_eq!(secrets.stored_count().await, 1);
    // The persisted ref must never be the raw token itself.
    assert_ne!(saved.token_secret_ref.as_deref(), Some("grn_live_token"));
}

#[tokio::test]
async fn save_settings_trims_surrounding_whitespace_before_storing() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let svc = service(secrets.clone(), Arc::new(EmptyGranolaApiClient));

    svc.save_settings(Some("  grn_padded_token  ".to_string()))
        .await
        .unwrap();

    assert_eq!(
        secrets.stored(TOKEN_REF).await.as_deref(),
        Some("grn_padded_token")
    );
}

#[tokio::test]
async fn save_settings_blank_token_deletes_secret_and_resets_to_not_configured() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let svc = service(secrets.clone(), Arc::new(EmptyGranolaApiClient));
    svc.save_settings(Some("grn_token".to_string()))
        .await
        .unwrap();
    svc.validate_and_enable()
        .await
        .expect("validate enables the integration");

    let cleared = svc.save_settings(Some(String::new())).await.unwrap();

    assert!(!cleared.enabled);
    assert!(cleared.token_secret_ref.is_none());
    assert_eq!(
        cleared.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(cleared.last_error.is_none());
    assert!(cleared.last_validated_at.is_none());
    assert!(secrets.deleted_keys().await.contains(&TOKEN_REF.to_string()));
    assert_eq!(secrets.stored_count().await, 0);
}

#[tokio::test]
async fn validate_and_enable_feeds_keychain_token_to_client_and_enables() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestGranolaClient::default());
    let svc = service(secrets.clone(), client.clone());
    svc.save_settings(Some("grn_secret".to_string()))
        .await
        .unwrap();

    let validated = svc.validate_and_enable().await.unwrap();

    assert!(validated.enabled);
    assert_eq!(
        validated.validation_status,
        IntegrationValidationStatus::Valid
    );
    assert!(validated.last_error.is_none());
    assert!(validated.last_validated_at.is_some());
    // The client received the raw token resolved from the keychain by ref.
    assert_eq!(client.seen_tokens().await, vec!["grn_secret".to_string()]);
}

#[tokio::test]
async fn validate_and_enable_without_token_reports_missing_token() {
    let svc = service(
        Arc::new(RecordingSecretStore::default()),
        Arc::new(EmptyGranolaApiClient),
    );

    let error = svc
        .validate_and_enable()
        .await
        .expect_err("validation without a token should fail");

    assert!(
        error.contains("Granola API token is required"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn validate_and_enable_marks_invalid_on_client_error() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestGranolaClient::with_validate_error(
        "Granola rejected the token",
    ));
    let svc = service(secrets, client);
    svc.save_settings(Some("grn_bad".to_string()))
        .await
        .unwrap();

    let validated = svc.validate_and_enable().await.unwrap();

    assert!(!validated.enabled);
    assert_eq!(
        validated.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        validated.last_error.as_deref(),
        Some("Granola rejected the token")
    );
    assert!(validated.last_validated_at.is_some());
}

#[tokio::test]
async fn validate_and_enable_with_unavailable_client_degrades_to_invalid() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let svc = service(
        secrets,
        Arc::new(UnavailableGranolaApiClient::new(
            "Granola HTTP client unavailable",
        )),
    );
    svc.save_settings(Some("grn_token".to_string()))
        .await
        .unwrap();

    let validated = svc.validate_and_enable().await.unwrap();

    assert!(!validated.enabled);
    assert_eq!(
        validated.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        validated.last_error.as_deref(),
        Some("Granola HTTP client unavailable")
    );
}

#[tokio::test]
async fn resaving_token_reuses_fixed_ref_without_orphaning_secrets() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let svc = service(secrets.clone(), Arc::new(EmptyGranolaApiClient));

    let first = svc
        .save_settings(Some("grn_first".to_string()))
        .await
        .unwrap();
    let second = svc
        .save_settings(Some("grn_second".to_string()))
        .await
        .unwrap();

    // Stable ref, single keychain entry, latest value wins.
    assert_eq!(first.token_secret_ref, second.token_secret_ref);
    assert_eq!(second.token_secret_ref.as_deref(), Some(TOKEN_REF));
    assert_eq!(secrets.stored_count().await, 1);
    assert_eq!(
        secrets.stored(TOKEN_REF).await.as_deref(),
        Some("grn_second")
    );
}

#[tokio::test]
async fn save_settings_errors_when_secure_storage_read_back_mismatches() {
    let secrets = Arc::new(MismatchingSecretStore::default());
    let svc = service(secrets.clone(), Arc::new(EmptyGranolaApiClient));

    let error = svc
        .save_settings(Some("grn_token".to_string()))
        .await
        .expect_err("read-back mismatch should fail the save");

    assert!(
        error.contains("different value"),
        "unexpected error: {error}"
    );
    // The just-written ref is cleaned up to avoid a dangling secret.
    assert!(secrets.deleted_keys().await.contains(&TOKEN_REF.to_string()));
}
