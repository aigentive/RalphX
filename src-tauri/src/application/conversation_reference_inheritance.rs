use std::collections::BTreeSet;

use crate::domain::entities::{ChatConversationId, MessageRole};
use crate::domain::repositories::ChatMessageRepository;
use crate::domain::services::ComposerIntegrationReference;
use crate::error::{AppError, AppResult};

use super::integration_reference_expansion::{
    SkippedIntegrationReference, SkippedIntegrationReferenceReason, MAX_INTEGRATION_REFERENCES,
};

pub(crate) const MAX_INHERITED_INTEGRATION_REFERENCES: usize = MAX_INTEGRATION_REFERENCES;

#[derive(Debug)]
pub(crate) struct ConversationInheritedIntegrationReferences {
    pub references: Vec<ComposerIntegrationReference>,
    pub skipped_references: Vec<SkippedIntegrationReference>,
}

/// Loads persisted composer integration references for a conversation in the
/// order that gives the latest user input precedence under the reference cap.
pub(crate) async fn collect_conversation_inherited_integration_references(
    chat_message_repo: &dyn ChatMessageRepository,
    conversation_id: &ChatConversationId,
) -> AppResult<ConversationInheritedIntegrationReferences> {
    let mut messages = chat_message_repo
        .get_by_conversation(conversation_id)
        .await?;
    let mut references = Vec::new();
    let mut seen = BTreeSet::new();
    let mut skipped_references = Vec::new();

    messages.reverse();
    for message in messages {
        if message.role != MessageRole::User {
            continue;
        }
        for reference in parse_message_integration_references(message.metadata.as_deref())? {
            if !has_reference_identity(&reference) {
                return Err(AppError::Validation(
                    "conversation reference metadata contains an integration reference without provider, kind, or id".to_string(),
                ));
            }
            if !seen.insert(integration_reference_identity(&reference)) {
                continue;
            }
            if references.len() >= MAX_INHERITED_INTEGRATION_REFERENCES {
                skipped_references.push(SkippedIntegrationReference::new(
                    &reference,
                    SkippedIntegrationReferenceReason::BudgetExceeded,
                    "Older inherited integration reference exceeded the conversation limit",
                ));
                continue;
            }
            references.push(reference);
        }
    }

    Ok(ConversationInheritedIntegrationReferences {
        references,
        skipped_references,
    })
}

fn parse_message_integration_references(
    metadata: Option<&str>,
) -> AppResult<Vec<ComposerIntegrationReference>> {
    let Some(metadata) = metadata else {
        return Ok(Vec::new());
    };
    let value = serde_json::from_str::<serde_json::Value>(metadata).map_err(|error| {
        AppError::Validation(format!(
            "conversation reference metadata is not valid JSON: {error}"
        ))
    })?;
    let object = value.as_object().ok_or_else(|| {
        AppError::Validation("conversation reference metadata must be a JSON object".to_string())
    })?;
    let Some(references) = object.get("composer_integration_references") else {
        return Ok(Vec::new());
    };

    serde_json::from_value::<Vec<ComposerIntegrationReference>>(references.clone()).map_err(
        |error| {
            AppError::Validation(format!(
                "conversation reference metadata contains malformed integration references: {error}"
            ))
        },
    )
}

fn has_reference_identity(reference: &ComposerIntegrationReference) -> bool {
    !reference.provider.trim().is_empty()
        && !reference.kind.trim().is_empty()
        && !reference.id.trim().is_empty()
}

fn integration_reference_identity(reference: &ComposerIntegrationReference) -> String {
    format!(
        "{}\n{}\n{}",
        reference.provider.trim(),
        reference.kind.trim(),
        reference.id.trim()
    )
}
