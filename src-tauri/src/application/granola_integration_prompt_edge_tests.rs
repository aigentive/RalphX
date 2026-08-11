use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use crate::application::granola_integration_service::{
    GranolaApiClient, GranolaApiError, GranolaAuthContext, GranolaIntegrationService,
    GranolaNoteDetail, GranolaTranscriptEntry,
};
use crate::application::integration_reference_expansion::SkippedIntegrationReferenceReason;
use crate::domain::services::{ComposerIntegrationReference, SecretStore, SecretStoreError};
use crate::infrastructure::memory::MemoryGranolaIntegrationSettingsRepository;

const BASE_PROMPT: &str = "Seed conversation";
const TOKEN_REF: &str = "integrations/granola/default/api-token";

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
    fetches: Mutex<Vec<(String, bool)>>,
    note: Mutex<Option<GranolaNoteDetail>>,
}

impl TestGranolaClient {
    async fn fetches(&self) -> Vec<(String, bool)> {
        self.fetches.lock().await.clone()
    }
}

#[async_trait]
impl GranolaApiClient for TestGranolaClient {
    async fn validate(&self, _auth: &GranolaAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn fetch_note_detail(
        &self,
        _auth: &GranolaAuthContext,
        note_id: &str,
        include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        self.fetches
            .lock()
            .await
            .push((note_id.to_string(), include_transcript));
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
                transcript: None,
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

#[tokio::test]
async fn expand_note_references_skips_bad_reference_shapes_without_fetching() {
    let client = Arc::new(TestGranolaClient::default());
    let svc = enabled_service(client.clone()).await;
    let mut unsupported_kind = note_reference(None);
    unsupported_kind.kind = "folder".to_string();

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[unsupported_kind])
        .await;
    assert_eq!(expansion.rewritten_prompt, BASE_PROMPT);
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::UnsupportedReference
    );

    let mut invalid_id = note_reference(None);
    invalid_id.id = "bad-note-id".to_string();
    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[invalid_id])
        .await;
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::UnsupportedReference
    );
    assert!(client.fetches().await.is_empty());
}

#[tokio::test]
async fn expand_note_references_skips_disabled_and_missing_keychain_secret_without_fetching() {
    let disabled_client = Arc::new(TestGranolaClient::default());
    let disabled = service(
        Arc::new(RecordingSecretStore::default()),
        disabled_client.clone() as Arc<dyn GranolaApiClient>,
    );
    disabled
        .save_settings(Some("grn_pending_token".to_string()))
        .await
        .expect("save token without validation");

    let expansion = disabled
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::IntegrationDisabled
    );
    assert!(disabled_client.fetches().await.is_empty());

    let secrets = Arc::new(RecordingSecretStore::default());
    let missing_secret_client = Arc::new(TestGranolaClient::default());
    let missing_secret = service(secrets.clone(), missing_secret_client.clone());
    missing_secret
        .save_settings(Some("grn_valid_then_deleted".to_string()))
        .await
        .expect("save token");
    missing_secret
        .validate_and_enable()
        .await
        .expect("validate token");
    secrets
        .delete_secret(TOKEN_REF)
        .await
        .expect("delete token");

    let expansion = missing_secret
        .expand_note_references_for_prompt(BASE_PROMPT, &[note_reference(None)])
        .await;
    assert_eq!(
        expansion.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::MissingCredentials
    );
    assert!(missing_secret_client.fetches().await.is_empty());
}

#[tokio::test]
async fn expand_note_references_uses_reference_fallbacks_and_escapes_attributes() {
    let client = Arc::new(TestGranolaClient::default());
    *client.note.lock().await = Some(GranolaNoteDetail {
        id: "not_1234567890ABCD".to_string(),
        title: None,
        url: None,
        summary: None,
        transcript: Some(vec![GranolaTranscriptEntry {
            speaker: Some("A&B <lead>".to_string()),
            text: "Transcript line".to_string(),
            start_ms: None,
            end_ms: None,
        }]),
    });
    let svc = enabled_service(client).await;
    let mut reference = note_reference(Some(true));
    reference.title = Some("Weekly & \"planning\" <demo>".to_string());
    reference.url = Some("https://granola.ai/notes/not_1234567890ABCD?a=1&b=<x>".to_string());
    reference.summary_excerpt = Some("Fallback summary".to_string());

    let expansion = svc
        .expand_note_references_for_prompt(BASE_PROMPT, &[reference])
        .await;

    assert!(expansion
        .rewritten_prompt
        .contains("title=\"Weekly &amp; &quot;planning&quot; &lt;demo&gt;\""));
    assert!(expansion
        .rewritten_prompt
        .contains("url=\"https://granola.ai/notes/not_1234567890ABCD?a=1&amp;b=&lt;x&gt;\""));
    assert!(expansion.rewritten_prompt.contains("Fallback summary"));
    assert!(expansion
        .rewritten_prompt
        .contains("speaker=\"A&amp;B &lt;lead&gt;\""));
}

#[tokio::test]
async fn expand_note_references_skips_when_total_budget_cannot_fit_wrapper() {
    let client = Arc::new(TestGranolaClient::default());
    let svc = enabled_service(client.clone()).await;

    for total_budget in [0, 1] {
        let expansion = svc
            .expand_note_references_for_prompt_with_budget(
                BASE_PROMPT,
                &[note_reference(None)],
                total_budget,
            )
            .await;
        assert_eq!(expansion.rewritten_prompt, BASE_PROMPT);
        assert_eq!(
            expansion.skipped_references[0].reason,
            SkippedIntegrationReferenceReason::BudgetExceeded
        );
    }
    assert!(client.fetches().await.is_empty());
}
