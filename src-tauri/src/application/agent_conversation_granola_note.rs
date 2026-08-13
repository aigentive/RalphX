use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::application::granola_integration_service::{
    is_valid_granola_note_id, GranolaIntegrationService, GranolaNoteDetail,
};
use crate::domain::entities::{
    AgentConversationGranolaNoteLink, AgentConversationGranolaRefreshStatus, ChatConversationId,
    ChatMessageId, ProjectId,
};
use crate::domain::repositories::AgentConversationGranolaNoteRepository;
use crate::domain::services::ComposerIntegrationReference;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerGranolaReferenceMetadata {
    pub note_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub include_transcript: bool,
}

pub async fn assign_primary_granola_note_if_absent(
    repo: &Arc<dyn AgentConversationGranolaNoteRepository>,
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    references: &[ComposerIntegrationReference],
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
) -> AppResult<Option<AgentConversationGranolaNoteLink>> {
    let Some(reference) = primary_granola_reference_from_composer_references(references) else {
        return Ok(None);
    };
    let link = link_from_reference(
        conversation_id,
        project_id,
        reference,
        assigned_from_message_id,
        assigned_at,
        false,
    );
    repo.insert_if_absent(link).await.map(Some)
}

pub async fn assign_primary_granola_note_if_absent_and_refresh(
    repo: &Arc<dyn AgentConversationGranolaNoteRepository>,
    integration_service: Option<&GranolaIntegrationService>,
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    references: &[ComposerIntegrationReference],
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
) -> AppResult<Option<AgentConversationGranolaNoteLink>> {
    let Some(link) = assign_primary_granola_note_if_absent(
        repo,
        conversation_id,
        project_id,
        references,
        assigned_from_message_id,
        assigned_at,
    )
    .await?
    else {
        return Ok(None);
    };
    if link.refresh_status != AgentConversationGranolaRefreshStatus::NotLoaded {
        return Ok(Some(link));
    }
    let Some(integration_service) = integration_service else {
        return Ok(Some(link));
    };
    refresh_granola_note_link(repo, integration_service, link)
        .await
        .map(Some)
}

pub async fn refresh_granola_note_link(
    repo: &Arc<dyn AgentConversationGranolaNoteRepository>,
    integration_service: &GranolaIntegrationService,
    mut link: AgentConversationGranolaNoteLink,
) -> AppResult<AgentConversationGranolaNoteLink> {
    let now = Utc::now();
    match integration_service
        .fetch_note_detail_for_user(&link.note_id, link.include_transcript)
        .await
    {
        Ok(note) => {
            apply_note_detail(&mut link, note, now);
        }
        Err(error) => {
            link.refresh_status = AgentConversationGranolaRefreshStatus::Error;
            link.refresh_error = Some(error);
            link.updated_at = now;
        }
    }
    repo.upsert(link).await
}

pub fn manual_link_from_reference(
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    reference: ComposerGranolaReferenceMetadata,
    assigned_at: DateTime<Utc>,
) -> AgentConversationGranolaNoteLink {
    link_from_reference(
        conversation_id,
        project_id,
        reference,
        None,
        assigned_at,
        true,
    )
}

fn link_from_reference(
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    reference: ComposerGranolaReferenceMetadata,
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
    manually_assigned: bool,
) -> AgentConversationGranolaNoteLink {
    AgentConversationGranolaNoteLink::new(
        conversation_id.clone(),
        project_id.clone(),
        reference.note_id,
        assigned_at,
    )
    .with_reference_metadata(
        reference.title,
        reference.url,
        reference.summary,
        reference.include_transcript,
    )
    .with_assignment_source(assigned_from_message_id, manually_assigned)
}

fn apply_note_detail(
    link: &mut AgentConversationGranolaNoteLink,
    note: GranolaNoteDetail,
    now: DateTime<Utc>,
) {
    let transcript_json = transcript_json(&note);
    link.note_id = note.id;
    link.note_url = note.url.or(link.note_url.take());
    link.title = note.title.or(link.title.take());
    link.summary_markdown = note.summary.or(link.summary_markdown.take());
    link.transcript_json = transcript_json;
    link.last_refreshed_at = Some(now);
    link.refresh_status = AgentConversationGranolaRefreshStatus::Loaded;
    link.refresh_error = None;
    link.updated_at = now;
}

fn transcript_json(note: &GranolaNoteDetail) -> String {
    let entries = note
        .transcript
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    json!({
                        "speaker": entry.speaker,
                        "text": entry.text,
                        "startMs": entry.start_ms,
                        "endMs": entry.end_ms,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

pub fn assigned_note_to_composer_reference(
    link: &AgentConversationGranolaNoteLink,
) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "granola".to_string(),
        kind: "note".to_string(),
        id: link.note_id.clone(),
        key: None,
        title: link.title.clone(),
        url: link.note_url.clone(),
        summary_excerpt: link.summary_markdown.clone(),
        include_transcript: Some(link.include_transcript),
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }
}

pub fn merge_assigned_granola_reference(
    assigned: Option<&AgentConversationGranolaNoteLink>,
    turn_references: &[ComposerIntegrationReference],
) -> Vec<ComposerIntegrationReference> {
    let Some(assigned) = assigned else {
        return turn_references.to_vec();
    };
    let assigned_reference = assigned_note_to_composer_reference(assigned);
    let mut merged = Vec::with_capacity(turn_references.len() + 1);
    merged.push(assigned_reference.clone());
    for reference in turn_references {
        if !is_same_granola_reference(reference, &assigned_reference) {
            merged.push(reference.clone());
        }
    }
    merged
}

pub fn granola_reference_from_composer_reference(
    reference: &ComposerIntegrationReference,
) -> Option<ComposerGranolaReferenceMetadata> {
    if reference.provider != "granola" || reference.kind != "note" {
        return None;
    }
    let note_id = reference.id.trim();
    if !is_valid_granola_note_id(note_id) {
        return None;
    }
    Some(ComposerGranolaReferenceMetadata {
        note_id: note_id.to_string(),
        title: reference
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        url: reference
            .url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        summary: reference
            .summary_excerpt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        include_transcript: reference.include_transcript.unwrap_or(true),
    })
}

pub fn primary_granola_reference_from_composer_references(
    references: &[ComposerIntegrationReference],
) -> Option<ComposerGranolaReferenceMetadata> {
    references
        .iter()
        .find_map(granola_reference_from_composer_reference)
}

fn is_same_granola_reference(
    reference: &ComposerIntegrationReference,
    assigned: &ComposerIntegrationReference,
) -> bool {
    granola_reference_from_composer_reference(reference)
        .is_some_and(|reference| reference.note_id == assigned.id)
}

#[cfg(test)]
#[path = "agent_conversation_granola_note_tests.rs"]
mod tests;
