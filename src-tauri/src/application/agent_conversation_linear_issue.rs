use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::application::linear_integration_service::LinearIntegrationService;
use crate::domain::entities::{
    AgentConversationLinearIssueLink, AgentConversationLinearRefreshStatus, ChatConversationId,
    ChatMessageId, ProjectId,
};
use crate::domain::repositories::AgentConversationLinearIssueRepository;
use crate::domain::services::ComposerIntegrationReference;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerLinearReferenceMetadata {
    pub issue_id: String,
    pub issue_key: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
}

pub async fn assign_primary_linear_issue_if_absent(
    repo: &Arc<dyn AgentConversationLinearIssueRepository>,
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    references: &[ComposerIntegrationReference],
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
) -> AppResult<Option<AgentConversationLinearIssueLink>> {
    let Some(reference) = primary_linear_reference_from_composer_references(references) else {
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

pub async fn assign_primary_linear_issue_if_absent_and_refresh(
    repo: &Arc<dyn AgentConversationLinearIssueRepository>,
    integration_service: Option<&LinearIntegrationService>,
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    references: &[ComposerIntegrationReference],
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
) -> AppResult<Option<AgentConversationLinearIssueLink>> {
    let Some(link) = assign_primary_linear_issue_if_absent(
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
    if link.refresh_status != AgentConversationLinearRefreshStatus::NotLoaded {
        return Ok(Some(link));
    }
    let Some(integration_service) = integration_service else {
        return Ok(Some(link));
    };
    refresh_linear_issue_link(repo, integration_service, link)
        .await
        .map(Some)
}

pub async fn refresh_linear_issue_link(
    repo: &Arc<dyn AgentConversationLinearIssueRepository>,
    integration_service: &LinearIntegrationService,
    mut link: AgentConversationLinearIssueLink,
) -> AppResult<AgentConversationLinearIssueLink> {
    let reference = assigned_issue_to_composer_reference(&link);
    let now = Utc::now();
    match integration_service.fetch_issue_content(&reference).await {
        Ok(content) => {
            link.issue_id = content.id;
            link.issue_key = content.key.or(link.issue_key);
            link.issue_url = content.url.or(link.issue_url);
            link.title = Some(content.title);
            link.status = content.state_name;
            link.assignee = content.assignee;
            link.reporter = content.creator;
            link.updated_at_remote = content.updated_at;
            link.description_markdown = Some(content.body.clone());
            link.description_text = Some(content.body);
            link.comments_json = "[]".to_string();
            link.attachments_json = "[]".to_string();
            link.last_refreshed_at = Some(now);
            link.refresh_status = AgentConversationLinearRefreshStatus::Loaded;
            link.refresh_error = None;
            link.updated_at = now;
        }
        Err(error) => {
            link.refresh_status = AgentConversationLinearRefreshStatus::Error;
            link.refresh_error = Some(error);
            link.updated_at = now;
        }
    }
    repo.upsert(link).await
}

pub fn manual_link_from_reference(
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    reference: ComposerLinearReferenceMetadata,
    assigned_at: DateTime<Utc>,
) -> AgentConversationLinearIssueLink {
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
    reference: ComposerLinearReferenceMetadata,
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
    manually_assigned: bool,
) -> AgentConversationLinearIssueLink {
    AgentConversationLinearIssueLink::new(
        conversation_id.clone(),
        project_id.clone(),
        reference.issue_id,
        assigned_at,
    )
    .with_reference_metadata(reference.issue_key, reference.title, reference.url)
    .with_assignment_source(assigned_from_message_id, manually_assigned)
}

pub fn assigned_issue_to_composer_reference(
    link: &AgentConversationLinearIssueLink,
) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "linear".to_string(),
        kind: "linear".to_string(),
        id: link.issue_id.clone(),
        key: link.issue_key.clone(),
        title: link.title.clone(),
        url: link.issue_url.clone(),
        summary_excerpt: None,
        include_transcript: None,
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }
}

pub fn merge_assigned_linear_reference(
    assigned: Option<&AgentConversationLinearIssueLink>,
    turn_references: &[ComposerIntegrationReference],
) -> Vec<ComposerIntegrationReference> {
    let Some(assigned) = assigned else {
        return turn_references.to_vec();
    };
    let assigned_reference = assigned_issue_to_composer_reference(assigned);
    let mut merged = Vec::with_capacity(turn_references.len() + 1);
    merged.push(assigned_reference.clone());
    for reference in turn_references {
        if !is_same_linear_reference(reference, &assigned_reference) {
            merged.push(reference.clone());
        }
    }
    merged
}

pub fn linear_reference_from_composer_reference(
    reference: &ComposerIntegrationReference,
) -> Option<ComposerLinearReferenceMetadata> {
    if reference.provider != "linear" || reference.kind != "linear" {
        return None;
    }
    let issue_id = reference.id.trim();
    if issue_id.is_empty()
        || issue_id.contains('\0')
        || issue_id.contains('\n')
        || issue_id.contains('\r')
    {
        return None;
    }
    Some(ComposerLinearReferenceMetadata {
        issue_id: issue_id.to_string(),
        issue_key: reference
            .key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
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
    })
}

pub fn primary_linear_reference_from_composer_references(
    references: &[ComposerIntegrationReference],
) -> Option<ComposerLinearReferenceMetadata> {
    references
        .iter()
        .find_map(linear_reference_from_composer_reference)
}

fn is_same_linear_reference(
    reference: &ComposerIntegrationReference,
    assigned: &ComposerIntegrationReference,
) -> bool {
    let Some(reference) = linear_reference_from_composer_reference(reference) else {
        return false;
    };
    if reference.issue_id == assigned.id {
        return true;
    }
    match (reference.issue_key.as_deref(), assigned.key.as_deref()) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        _ => false,
    }
}

#[cfg(test)]
#[path = "agent_conversation_linear_issue_tests.rs"]
mod tests;
