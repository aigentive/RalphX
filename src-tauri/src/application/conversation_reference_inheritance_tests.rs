use chrono::{Duration, Utc};

use crate::application::conversation_reference_inheritance::{
    collect_conversation_inherited_integration_references, MAX_INHERITED_INTEGRATION_REFERENCES,
};
use crate::application::integration_reference_expansion::SkippedIntegrationReferenceReason;
use crate::domain::entities::{ChatConversationId, ChatMessage, ProjectId};
use crate::domain::repositories::ChatMessageRepository;
use crate::error::AppError;
use crate::infrastructure::memory::MemoryChatMessageRepository;

fn integration_reference(id: impl Into<String>) -> serde_json::Value {
    let id = id.into();
    serde_json::json!({
        "provider": "atlassian",
        "kind": "jira",
        "id": id,
    })
}

async fn create_user_message(
    repository: &MemoryChatMessageRepository,
    conversation_id: &ChatConversationId,
    metadata: Option<String>,
    created_at: chrono::DateTime<Utc>,
) {
    let mut message = ChatMessage::user_in_project(ProjectId::new(), "reference context");
    message.conversation_id = Some(conversation_id.clone());
    message.metadata = metadata;
    message.created_at = created_at;
    repository
        .create(message)
        .await
        .expect("user message should persist");
}

#[tokio::test]
async fn collector_returns_newest_user_references_deduplicated_and_capped() {
    let repository = MemoryChatMessageRepository::new();
    let conversation_id = ChatConversationId::new();
    let now = Utc::now();

    create_user_message(
        &repository,
        &conversation_id,
        Some(
            serde_json::json!({
                "composer_integration_references": [integration_reference("OLD-1")]
            })
            .to_string(),
        ),
        now - Duration::minutes(1),
    )
    .await;
    create_user_message(
        &repository,
        &conversation_id,
        Some(
            serde_json::json!({
                "hidden_from_ui": true,
                "composer_integration_references": [
                    integration_reference("NEW-1"),
                    integration_reference("NEW-2"),
                    integration_reference("NEW-1"),
                    integration_reference("NEW-3"),
                    integration_reference("NEW-4"),
                    integration_reference("NEW-5"),
                    integration_reference("NEW-6"),
                    integration_reference("NEW-7"),
                    integration_reference("NEW-8")
                ]
            })
            .to_string(),
        ),
        now,
    )
    .await;

    let inherited =
        collect_conversation_inherited_integration_references(&repository, &conversation_id)
            .await
            .expect("valid metadata should be collected");

    assert_eq!(
        inherited.references.len(),
        MAX_INHERITED_INTEGRATION_REFERENCES
    );
    assert_eq!(inherited.references[0].id, "NEW-1");
    assert_eq!(inherited.references[7].id, "NEW-8");
    assert!(!inherited
        .references
        .iter()
        .any(|reference| reference.id == "OLD-1"));
    assert_eq!(inherited.skipped_references.len(), 1);
    assert_eq!(inherited.skipped_references[0].id, "OLD-1");
    assert_eq!(
        inherited.skipped_references[0].reason,
        SkippedIntegrationReferenceReason::BudgetExceeded
    );
}

#[tokio::test]
async fn collector_rejects_malformed_user_reference_metadata() {
    let repository = MemoryChatMessageRepository::new();
    let conversation_id = ChatConversationId::new();

    create_user_message(
        &repository,
        &conversation_id,
        Some("{not-json".to_string()),
        Utc::now(),
    )
    .await;

    let error =
        collect_conversation_inherited_integration_references(&repository, &conversation_id)
            .await
            .expect_err("malformed metadata must not silently remove inherited references");

    assert!(matches!(error, AppError::Validation(_)));
    assert!(error
        .to_string()
        .contains("conversation reference metadata"));
}

#[tokio::test]
async fn collector_rejects_invalid_metadata_shapes_and_reference_identities() {
    let cases = [
        (
            serde_json::json!([]).to_string(),
            "metadata must be a JSON object",
        ),
        (
            serde_json::json!({
                "composer_integration_references": {"provider": "atlassian"}
            })
            .to_string(),
            "malformed integration references",
        ),
        (
            serde_json::json!({
                "composer_integration_references": [{
                    "provider": "",
                    "kind": "jira",
                    "id": "RX-1"
                }]
            })
            .to_string(),
            "without provider, kind, or id",
        ),
    ];

    for (metadata, expected_message) in cases {
        let repository = MemoryChatMessageRepository::new();
        let conversation_id = ChatConversationId::new();
        create_user_message(&repository, &conversation_id, Some(metadata), Utc::now()).await;

        let error =
            collect_conversation_inherited_integration_references(&repository, &conversation_id)
                .await
                .expect_err("invalid metadata must fail closed");

        assert!(matches!(error, AppError::Validation(_)));
        assert!(
            error.to_string().contains(expected_message),
            "unexpected validation error: {error}"
        );
    }
}
