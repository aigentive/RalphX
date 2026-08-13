use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use crate::application::granola_integration_service::{
    GranolaApiClient, GranolaApiError, GranolaAuthContext, GranolaIntegrationService,
    GranolaNoteDetail, GranolaRequestLimiter, GranolaTranscriptEntry,
};
use crate::application::integration_reference_expansion::SkippedIntegrationReferenceReason;
use crate::domain::services::{ComposerIntegrationReference, SecretStore, SecretStoreError};
use crate::infrastructure::memory::MemoryGranolaIntegrationSettingsRepository;

const BASE_PROMPT: &str = "Seed conversation";
const MAX_TEST_NOTE_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct RecordingSecretStore {
    secrets: RwLock<HashMap<String, String>>,
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
        self.secrets.write().await.remove(key);
        Ok(())
    }
}

#[derive(Default)]
struct TestGranolaClient {
    seen_tokens: Mutex<Vec<String>>,
    fetches: Mutex<Vec<(String, bool)>>,
    note: Mutex<Option<GranolaNoteDetail>>,
    fetch_error: Mutex<Option<GranolaApiError>>,
}

impl TestGranolaClient {
    async fn fetches(&self) -> Vec<(String, bool)> {
        self.fetches.lock().await.clone()
    }
}

#[derive(Default)]
struct CountingRateLimiter {
    waits: Mutex<usize>,
}

impl CountingRateLimiter {
    async fn clear(&self) {
        *self.waits.lock().await = 0;
    }

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

#[async_trait]
impl GranolaApiClient for TestGranolaClient {
    async fn validate(&self, auth: &GranolaAuthContext) -> Result<(), String> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        Ok(())
    }

    async fn fetch_note_detail(
        &self,
        auth: &GranolaAuthContext,
        note_id: &str,
        include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        self.seen_tokens.lock().await.push(auth.api_token.clone());
        self.fetches
            .lock()
            .await
            .push((note_id.to_string(), include_transcript));
        if let Some(error) = self.fetch_error.lock().await.clone() {
            return Err(error);
        }
        Ok(self
            .note
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| GranolaNoteDetail {
                id: note_id.to_string(),
                title: Some("Weekly planning".to_string()),
                url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
                summary: Some("Summary decisions".to_string()),
                transcript: include_transcript.then(|| {
                    vec![GranolaTranscriptEntry {
                        speaker: Some("Alex".to_string()),
                        text: "Transcript line".to_string(),
                        start_ms: Some(1000),
                        end_ms: Some(2500),
                    }]
                }),
            }))
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

async fn enabled_service(client: Arc<dyn GranolaApiClient>) -> GranolaIntegrationService {
    let svc = service(Arc::new(RecordingSecretStore::default()), client);
    svc.save_settings(Some("grn_prompt_token".to_string()))
        .await
        .expect("save Granola token");
    svc.validate_and_enable()
        .await
        .expect("validate Granola settings");
    svc
}

fn note_reference(include_transcript: Option<bool>) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "granola".to_string(),
        kind: "note".to_string(),
        id: "not_1234567890ABCD".to_string(),
        key: None,
        title: Some("Weekly planning".to_string()),
        url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
        summary_excerpt: Some("Summary decisions".to_string()),
        include_transcript,
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }
}

fn note_reference_with_id(id: String) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        id,
        ..note_reference(None)
    }
}

fn added_prompt_bytes(expanded: &str, base: &str) -> usize {
    expanded.len().saturating_sub(base.trim_end().len())
}

fn rendered_note_bytes(expanded: &str) -> usize {
    let start = expanded
        .find("<granola_note")
        .expect("rendered Granola note start");
    let end = expanded
        .find("</granola_note>")
        .expect("rendered Granola note end")
        + "</granola_note>".len();
    end - start
}

#[tokio::test]
async fn expand_note_references_fetches_summary_only_by_default() {
    let client = Arc::new(TestGranolaClient::default());
    let svc = enabled_service(client.clone()).await;

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;

    assert!(expansion.skipped_references.is_empty());
    assert!(expansion
        .rewritten_prompt
        .contains("expanded user-selected Granola notes"));
    assert!(expansion.rewritten_prompt.contains("Summary decisions"));
    assert!(!expansion.rewritten_prompt.contains("Transcript line"));
    assert_eq!(
        client.fetches().await,
        vec![("not_1234567890ABCD".to_string(), false)]
    );
}

#[tokio::test]
async fn expand_note_references_includes_transcript_only_when_requested() {
    let client = Arc::new(TestGranolaClient::default());
    let svc = enabled_service(client.clone()).await;

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(Some(true))])
        .await;

    assert!(expansion.rewritten_prompt.contains("Transcript line"));
    assert!(expansion.rewritten_prompt.contains("<transcript_entry"));
    assert_eq!(
        client.fetches().await,
        vec![("not_1234567890ABCD".to_string(), true)]
    );
}

#[tokio::test]
async fn expand_note_references_waits_for_rate_limit_before_each_fetch() {
    let client = Arc::new(TestGranolaClient::default());
    let limiter = Arc::new(CountingRateLimiter::default());
    let svc = service_with_rate_limiter(
        Arc::new(RecordingSecretStore::default()),
        client.clone(),
        limiter.clone(),
    );
    svc.save_settings(Some("grn_prompt_token".to_string()))
        .await
        .expect("save Granola token");
    svc.validate_and_enable()
        .await
        .expect("validate Granola settings");
    limiter.clear().await;

    let references = vec![
        note_reference_with_id("not_00000000000001".to_string()),
        note_reference_with_id("not_00000000000002".to_string()),
        note_reference_with_id("not_00000000000003".to_string()),
    ];
    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &references)
        .await;

    assert!(expansion.skipped_references.is_empty());
    assert_eq!(client.fetches().await.len(), 3);
    assert_eq!(limiter.wait_count().await, 3);
}

#[tokio::test]
async fn expand_note_references_skips_missing_credentials_without_fetching() {
    let client = Arc::new(TestGranolaClient::default());
    let svc = service(
        Arc::new(RecordingSecretStore::default()),
        client.clone() as Arc<dyn GranolaApiClient>,
    );

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;

    assert_eq!(expansion.rewritten_prompt, BASE_PROMPT);
    assert_eq!(expansion.skipped_references.len(), 1);
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::MissingCredentials
    );
    assert!(client.fetches().await.is_empty());
}

#[tokio::test]
async fn expand_note_references_reports_not_found_and_rate_limit_as_redacted_skips() {
    let client = Arc::new(TestGranolaClient::default());
    *client.fetch_error.lock().await = Some(GranolaApiError::NotFound);
    let svc = enabled_service(client.clone()).await;

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;

    assert_eq!(expansion.rewritten_prompt, BASE_PROMPT);
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::NotFound
    );
    assert!(!expansion.skipped_references[0].message.contains("grn_"));

    *client.fetch_error.lock().await = Some(GranolaApiError::RateLimited);
    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::RateLimited
    );

    *client.fetch_error.lock().await = Some(GranolaApiError::ApiError(
        "upstream error with grn_prompt_token".to_string(),
    ));
    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::ApiError
    );
    assert_eq!(
        expansion.skipped_references[0].message,
        "Granola API request failed"
    );
    assert!(!expansion.skipped_references[0].message.contains("grn_"));
}

#[tokio::test]
async fn expand_note_references_applies_per_note_budget() {
    let client = Arc::new(TestGranolaClient::default());
    *client.note.lock().await = Some(GranolaNoteDetail {
        id: "not_1234567890ABCD".to_string(),
        title: Some("Large note".to_string()),
        url: None,
        summary: Some("x".repeat(70 * 1024)),
        transcript: None,
    });
    let svc = enabled_service(client).await;

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;

    assert!(expansion.rewritten_prompt.contains("truncated=\"true\""));
    assert!(expansion.rewritten_prompt.contains("bytes=\"71680\""));
}

#[tokio::test]
async fn expand_note_references_counts_note_wrapper_in_per_note_budget() {
    let client = Arc::new(TestGranolaClient::default());
    *client.note.lock().await = Some(GranolaNoteDetail {
        id: "not_1234567890ABCD".to_string(),
        title: Some("Large note".to_string()),
        url: None,
        summary: Some("x".repeat(70 * 1024)),
        transcript: None,
    });
    let svc = enabled_service(client).await;

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;

    assert!(expansion.rewritten_prompt.contains("truncated=\"true\""));
    assert!(
        rendered_note_bytes(&expansion.rewritten_prompt) <= MAX_TEST_NOTE_BYTES,
        "rendered note, including tags and fences, must stay within the per-note budget"
    );
}

#[tokio::test]
async fn expand_note_references_counts_block_wrapper_in_total_budget() {
    let client = Arc::new(TestGranolaClient::default());
    *client.note.lock().await = Some(GranolaNoteDetail {
        id: "not_1234567890ABCD".to_string(),
        title: Some("Large note".to_string()),
        url: None,
        summary: Some("x".repeat(70 * 1024)),
        transcript: None,
    });
    let svc = enabled_service(client).await;
    let total_budget = 1024;

    let expansion = svc
        .expand_note_references_for_prompt_with_budget(
            BASE_PROMPT,
            &[note_reference(None)],
            total_budget,
        )
        .await;

    assert!(
        added_prompt_bytes(&expansion.rewritten_prompt, BASE_PROMPT) <= total_budget,
        "Granola block, including wrapper/header bytes, must honor total budget"
    );
}

#[tokio::test]
async fn expand_note_references_reports_extra_notes_as_budget_skips() {
    let client = Arc::new(TestGranolaClient::default());
    let svc = enabled_service(client.clone()).await;
    let references = (0..17)
        .map(|index| note_reference_with_id(format!("not_{index:014}")))
        .collect::<Vec<_>>();

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &references)
        .await;

    assert_eq!(client.fetches().await.len(), 16);
    assert_eq!(expansion.skipped_references.len(), 1);
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::BudgetExceeded
    );
}
