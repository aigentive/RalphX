use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use crate::application::granola_integration_service::{
    EmptyGranolaApiClient, GranolaApiClient, GranolaApiError, GranolaAuthContext,
    GranolaIntegrationService, GranolaNoteDetail, GranolaNoteListPage, GranolaNoteSummary,
    GranolaRateLimiter, GranolaRequestLimiter, GranolaTranscriptEntry, UnavailableGranolaApiClient,
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

struct WriteOnlySecretStore;

#[async_trait]
impl SecretStore for WriteOnlySecretStore {
    async fn put_secret(&self, _key: &str, _value: &str) -> Result<(), SecretStoreError> {
        Ok(())
    }

    async fn get_secret(&self, _key: &str) -> Result<Option<String>, SecretStoreError> {
        Ok(None)
    }

    async fn delete_secret(&self, _key: &str) -> Result<(), SecretStoreError> {
        Ok(())
    }
}

/// Fake `GranolaApiClient` recording the auth token it observed, so tests can
/// prove the keychain round-trip feeds the client, plus a configurable error.
#[derive(Default)]
struct TestGranolaClient {
    validate_error: Option<String>,
    list_response: Option<Result<GranolaNoteListPage, GranolaApiError>>,
    detail_response: Option<Result<GranolaNoteDetail, GranolaApiError>>,
    seen_tokens: Mutex<Vec<String>>,
    seen_list_requests: Mutex<Vec<(String, usize, Option<String>)>>,
    seen_detail_requests: Mutex<Vec<(String, String, bool)>>,
}

impl TestGranolaClient {
    fn with_validate_error(error: &str) -> Self {
        Self {
            validate_error: Some(error.to_string()),
            ..Default::default()
        }
    }

    fn with_list_response(response: Result<GranolaNoteListPage, GranolaApiError>) -> Self {
        Self {
            list_response: Some(response),
            ..Default::default()
        }
    }

    fn with_detail_response(response: Result<GranolaNoteDetail, GranolaApiError>) -> Self {
        Self {
            detail_response: Some(response),
            ..Default::default()
        }
    }

    async fn seen_tokens(&self) -> Vec<String> {
        self.seen_tokens.lock().await.clone()
    }

    async fn seen_list_requests(&self) -> Vec<(String, usize, Option<String>)> {
        self.seen_list_requests.lock().await.clone()
    }

    async fn seen_detail_requests(&self) -> Vec<(String, String, bool)> {
        self.seen_detail_requests.lock().await.clone()
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

    async fn list_notes(
        &self,
        auth: &GranolaAuthContext,
        page_size: usize,
        cursor: Option<&str>,
    ) -> Result<GranolaNoteListPage, GranolaApiError> {
        self.seen_list_requests.lock().await.push((
            auth.api_token.clone(),
            page_size,
            cursor.map(ToOwned::to_owned),
        ));
        self.list_response.clone().unwrap_or_else(|| {
            Err(GranolaApiError::ApiError(
                "unexpected list request".to_string(),
            ))
        })
    }

    async fn fetch_note_detail(
        &self,
        auth: &GranolaAuthContext,
        note_id: &str,
        include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        self.seen_detail_requests.lock().await.push((
            auth.api_token.clone(),
            note_id.to_string(),
            include_transcript,
        ));
        self.detail_response.clone().unwrap_or_else(|| {
            Err(GranolaApiError::ApiError(
                "unexpected detail request".to_string(),
            ))
        })
    }
}

#[derive(Default)]
struct CountingRateLimiter {
    waits: Mutex<usize>,
}

impl CountingRateLimiter {
    async fn wait_count(&self) -> usize {
        *self.waits.lock().await
    }
}

#[async_trait]
impl GranolaRequestLimiter for CountingRateLimiter {
    async fn wait_for_request(&self) {
        *self.waits.lock().await += 1;
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

fn service_with_rate_limiter(
    secret_store: Arc<dyn SecretStore>,
    client: Arc<dyn GranolaApiClient>,
    rate_limiter: Arc<dyn GranolaRequestLimiter>,
) -> GranolaIntegrationService {
    GranolaIntegrationService::new_with_rate_limiter(
        Arc::new(MemoryGranolaIntegrationSettingsRepository::new()),
        secret_store,
        client,
        rate_limiter,
    )
}

fn note_page() -> GranolaNoteListPage {
    GranolaNoteListPage {
        notes: vec![GranolaNoteSummary {
            id: "not_1234567890ABCD".to_string(),
            title: Some("Planning sync".to_string()),
            url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
            summary: Some("Discussed launch timing".to_string()),
            created_at: Some("2026-06-20T12:00:00Z".to_string()),
            updated_at: Some("2026-06-20T13:00:00Z".to_string()),
        }],
        has_more: true,
        cursor: Some("next-cursor".to_string()),
    }
}

fn note_detail() -> GranolaNoteDetail {
    GranolaNoteDetail {
        id: "not_1234567890ABCD".to_string(),
        title: Some("Planning sync".to_string()),
        url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
        summary: Some("Fresh summary".to_string()),
        transcript: Some(vec![GranolaTranscriptEntry {
            speaker: Some("Alex".to_string()),
            text: "Ship it".to_string(),
            start_ms: Some(100),
            end_ms: Some(250),
        }]),
    }
}

async fn enable_service(svc: &GranolaIntegrationService, token: &str) {
    svc.save_settings(Some(token.to_string()))
        .await
        .expect("save Granola token");
    svc.validate_and_enable()
        .await
        .expect("validate Granola token");
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
    assert_eq!(
        saved.validation_status,
        IntegrationValidationStatus::Pending
    );
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
    assert!(secrets
        .deleted_keys()
        .await
        .contains(&TOKEN_REF.to_string()));
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
async fn validate_and_enable_waits_for_rate_limit_before_client_request() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestGranolaClient::default());
    let limiter = Arc::new(CountingRateLimiter::default());
    let svc = service_with_rate_limiter(secrets, client.clone(), limiter.clone());
    svc.save_settings(Some("grn_secret".to_string()))
        .await
        .unwrap();

    svc.validate_and_enable().await.unwrap();

    assert_eq!(limiter.wait_count().await, 1);
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
    assert!(secrets
        .deleted_keys()
        .await
        .contains(&TOKEN_REF.to_string()));
}

#[tokio::test]
async fn save_settings_errors_when_secure_storage_read_back_is_missing() {
    let svc = service(
        Arc::new(WriteOnlySecretStore),
        Arc::new(EmptyGranolaApiClient),
    );

    let error = svc
        .save_settings(Some("grn_token".to_string()))
        .await
        .expect_err("missing read-back should fail the save");

    assert!(
        error.contains("secure storage returned no value"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn default_and_unavailable_clients_return_note_list_and_detail_errors() {
    let auth = GranolaAuthContext {
        api_token: "grn_token".to_string(),
    };
    let empty = EmptyGranolaApiClient;

    let empty_detail = empty
        .fetch_note_detail(&auth, "not_1234567890ABCD", true)
        .await
        .expect_err("empty client detail should be unavailable");
    assert!(matches!(empty_detail, GranolaApiError::ApiError(_)));
    if let GranolaApiError::ApiError(message) = empty_detail {
        assert!(message.contains("note detail fetch is unavailable"));
    }
    let empty_list = empty
        .list_notes(&auth, 5, Some("cursor"))
        .await
        .expect_err("empty client list should be unavailable");
    assert!(matches!(empty_list, GranolaApiError::ApiError(_)));

    let unavailable = UnavailableGranolaApiClient::new("Granola HTTP client unavailable");
    assert!(unavailable.is_unavailable_for_tests());
    let unavailable_detail = unavailable
        .fetch_note_detail(&auth, "not_1234567890ABCD", false)
        .await
        .expect_err("unavailable detail should fail");
    assert_eq!(
        unavailable_detail,
        GranolaApiError::ApiError("Granola HTTP client unavailable".to_string())
    );
    let unavailable_list = unavailable
        .list_notes(&auth, 10, None)
        .await
        .expect_err("unavailable list should fail");
    assert_eq!(
        unavailable_list,
        GranolaApiError::ApiError("Granola HTTP client unavailable".to_string())
    );
}

#[tokio::test]
async fn list_notes_requires_configured_and_enabled_settings() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let svc = service(
        secrets,
        Arc::new(TestGranolaClient::with_list_response(Ok(note_page()))),
    );

    let unconfigured = svc
        .list_notes(10, None)
        .await
        .expect_err("unconfigured integration should fail");
    assert!(unconfigured.contains("not configured"));

    svc.save_settings(Some("grn_token".to_string()))
        .await
        .expect("save Granola token");
    let disabled = svc
        .list_notes(10, None)
        .await
        .expect_err("pending integration should fail");
    assert!(disabled.contains("not enabled"));
}

#[tokio::test]
async fn list_notes_clamps_page_uses_token_and_waits_for_rate_limit() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestGranolaClient::with_list_response(Ok(note_page())));
    let limiter = Arc::new(CountingRateLimiter::default());
    let svc = service_with_rate_limiter(secrets, client.clone(), limiter.clone());
    enable_service(&svc, "grn_secret").await;

    let page = svc
        .list_notes(99, Some("cursor/value"))
        .await
        .expect("list Granola notes");

    assert_eq!(page.notes.len(), 1);
    assert_eq!(page.notes[0].id, "not_1234567890ABCD");
    assert!(page.has_more);
    assert_eq!(page.cursor.as_deref(), Some("next-cursor"));
    assert_eq!(limiter.wait_count().await, 2);
    assert_eq!(
        client.seen_list_requests().await,
        vec![(
            "grn_secret".to_string(),
            30,
            Some("cursor/value".to_string())
        )]
    );
}

#[tokio::test]
async fn fetch_note_detail_for_user_validates_id_uses_token_and_waits() {
    let secrets = Arc::new(RecordingSecretStore::default());
    let client = Arc::new(TestGranolaClient::with_detail_response(Ok(note_detail())));
    let limiter = Arc::new(CountingRateLimiter::default());
    let svc = service_with_rate_limiter(secrets, client.clone(), limiter.clone());

    let invalid = svc
        .fetch_note_detail_for_user("bad-note-id", true)
        .await
        .expect_err("invalid note id should fail before auth");
    assert!(invalid.contains("Granola note id is invalid"));
    assert!(client.seen_detail_requests().await.is_empty());

    enable_service(&svc, "grn_secret").await;
    let detail = svc
        .fetch_note_detail_for_user("not_1234567890ABCD", true)
        .await
        .expect("fetch Granola note detail");

    assert_eq!(detail.summary.as_deref(), Some("Fresh summary"));
    assert_eq!(detail.transcript.expect("transcript")[0].text, "Ship it");
    assert_eq!(limiter.wait_count().await, 2);
    assert_eq!(
        client.seen_detail_requests().await,
        vec![(
            "grn_secret".to_string(),
            "not_1234567890ABCD".to_string(),
            true
        )]
    );
}

#[tokio::test]
async fn fetch_note_detail_for_user_maps_granola_api_errors() {
    let rate_limited = Arc::new(TestGranolaClient::with_detail_response(Err(
        GranolaApiError::RateLimited,
    )));
    let svc = service(
        Arc::new(RecordingSecretStore::default()),
        rate_limited.clone(),
    );
    enable_service(&svc, "grn_secret").await;
    let error = svc
        .fetch_note_detail_for_user("not_1234567890ABCD", false)
        .await
        .expect_err("rate-limited request should fail");
    assert!(error.contains("rate limit"));

    let api_error = Arc::new(TestGranolaClient::with_detail_response(Err(
        GranolaApiError::ApiError("Granola failed".to_string()),
    )));
    let svc = service(Arc::new(RecordingSecretStore::default()), api_error);
    enable_service(&svc, "grn_secret").await;
    let error = svc
        .fetch_note_detail_for_user("not_1234567890ABCD", false)
        .await
        .expect_err("API error should be surfaced");
    assert_eq!(error, "Granola failed");
}

#[tokio::test(start_paused = true)]
async fn granola_rate_limiter_enforces_sustained_window_without_real_sleep() {
    let limiter = Arc::new(GranolaRateLimiter::with_limits_for_tests(
        2,
        tokio::time::Duration::from_secs(1),
        10,
        tokio::time::Duration::from_secs(5),
    ));
    limiter.wait_for_request().await;
    limiter.wait_for_request().await;

    let third_wait = tokio::spawn({
        let limiter = Arc::clone(&limiter);
        async move {
            limiter.wait_for_request().await;
        }
    });
    tokio::task::yield_now().await;
    assert!(
        !third_wait.is_finished(),
        "third request should wait for the sustained window"
    );

    tokio::time::advance(tokio::time::Duration::from_millis(999)).await;
    tokio::task::yield_now().await;
    assert!(
        !third_wait.is_finished(),
        "request should remain throttled until the full window elapses"
    );

    tokio::time::advance(tokio::time::Duration::from_millis(1)).await;
    third_wait.await.expect("rate-limited request completes");
}
