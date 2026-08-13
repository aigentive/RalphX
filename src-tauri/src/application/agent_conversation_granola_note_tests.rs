use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;

use crate::application::agent_conversation_granola_note::{
    assign_primary_granola_note_if_absent_and_refresh, assigned_note_to_composer_reference,
    merge_assigned_granola_reference,
};
use crate::application::granola_integration_service::{
    GranolaApiClient, GranolaApiError, GranolaAuthContext, GranolaIntegrationService,
    GranolaNoteDetail, GranolaTranscriptEntry,
};
use crate::domain::entities::{ChatConversationId, ChatMessageId, ProjectId};
use crate::domain::integrations::{
    GranolaIntegrationSettings, GranolaIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::domain::repositories::AgentConversationGranolaNoteRepository;
use crate::domain::services::{ComposerIntegrationReference, SecretStore};
use crate::infrastructure::memory::{
    MemoryAgentConversationGranolaNoteRepository, MemoryGranolaIntegrationSettingsRepository,
    MemorySecretStore,
};

struct TestGranolaClient {
    note: Mutex<Option<GranolaNoteDetail>>,
}

#[async_trait]
impl GranolaApiClient for TestGranolaClient {
    async fn validate(&self, _auth: &GranolaAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn fetch_note_detail(
        &self,
        _auth: &GranolaAuthContext,
        _note_id: &str,
        _include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        Ok(self
            .note
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| GranolaNoteDetail {
                id: "not_1234567890ABCD".to_string(),
                title: Some("Planning sync".to_string()),
                url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
                summary: Some("Summary".to_string()),
                transcript: Some(vec![GranolaTranscriptEntry {
                    speaker: Some("Alex".to_string()),
                    text: "Ship it".to_string(),
                    start_ms: Some(100),
                    end_ms: Some(200),
                }]),
            }))
    }
}

async fn service() -> GranolaIntegrationService {
    let settings_repo = Arc::new(MemoryGranolaIntegrationSettingsRepository::new());
    let secret_store = Arc::new(MemorySecretStore::new());
    secret_store
        .put_secret("integrations/granola/default/api-token", "grn_test")
        .await
        .unwrap();
    settings_repo
        .upsert(&GranolaIntegrationSettings {
            enabled: true,
            token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            last_validated_at: Some(Utc::now()),
            last_error: None,
            updated_at: Utc::now(),
        })
        .await
        .unwrap();
    GranolaIntegrationService::new(
        settings_repo,
        secret_store,
        Arc::new(TestGranolaClient {
            note: Mutex::new(None),
        }),
    )
}

fn granola_reference() -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "granola".to_string(),
        kind: "note".to_string(),
        id: "not_1234567890ABCD".to_string(),
        key: None,
        title: Some("Planning sync".to_string()),
        url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
        summary_excerpt: Some("Initial summary".to_string()),
        include_transcript: Some(true),
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }
}

#[tokio::test]
async fn assigns_and_refreshes_primary_granola_note_from_composer_references() {
    let repo: Arc<dyn AgentConversationGranolaNoteRepository> =
        Arc::new(MemoryAgentConversationGranolaNoteRepository::new());
    let service = service().await;

    let assigned = assign_primary_granola_note_if_absent_and_refresh(
        &repo,
        Some(&service),
        &ChatConversationId::from_string("conversation-1".to_string()),
        &ProjectId::from_string("project-1".to_string()),
        &[granola_reference()],
        Some(ChatMessageId::from_string("message-1".to_string())),
        Utc::now(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(assigned.note_id, "not_1234567890ABCD");
    assert_eq!(assigned.title.as_deref(), Some("Planning sync"));
    assert_eq!(assigned.summary_markdown.as_deref(), Some("Summary"));
    assert!(assigned.transcript_json.contains("Ship it"));
}

#[test]
fn merges_bound_granola_reference_before_turn_references_without_duplicates() {
    let link = crate::domain::entities::AgentConversationGranolaNoteLink::new(
        ChatConversationId::from_string("conversation-1".to_string()),
        ProjectId::from_string("project-1".to_string()),
        "not_1234567890ABCD".to_string(),
        Utc::now(),
    )
    .with_reference_metadata(
        Some("Planning sync".to_string()),
        Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
        Some("Summary".to_string()),
        true,
    );

    let merged = merge_assigned_granola_reference(Some(&link), &[granola_reference()]);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0], assigned_note_to_composer_reference(&link));
}
