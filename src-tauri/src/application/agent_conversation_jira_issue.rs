use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::application::atlassian_integration_service::AtlassianIntegrationService;
use crate::domain::entities::{
    AgentConversationJiraIssueLink, AgentConversationJiraRefreshStatus, ChatConversationId,
    ChatMessageId, ProjectId,
};
use crate::domain::repositories::AgentConversationJiraIssueRepository;
use crate::domain::services::{
    jira_reference_from_composer_reference, primary_jira_reference_from_composer_references,
    ComposerIntegrationReference, ComposerJiraReferenceMetadata,
};
use crate::error::AppResult;

pub async fn assign_primary_jira_issue_if_absent(
    repo: &Arc<dyn AgentConversationJiraIssueRepository>,
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    references: &[ComposerIntegrationReference],
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
) -> AppResult<Option<AgentConversationJiraIssueLink>> {
    let Some(reference) = primary_jira_reference_from_composer_references(references) else {
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

pub async fn assign_primary_jira_issue_if_absent_and_refresh(
    repo: &Arc<dyn AgentConversationJiraIssueRepository>,
    integration_service: Option<&AtlassianIntegrationService>,
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    references: &[ComposerIntegrationReference],
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
) -> AppResult<Option<AgentConversationJiraIssueLink>> {
    let Some(link) = assign_primary_jira_issue_if_absent(
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
    if link.refresh_status != AgentConversationJiraRefreshStatus::NotLoaded {
        return Ok(Some(link));
    }
    let Some(integration_service) = integration_service else {
        return Ok(Some(link));
    };
    refresh_jira_issue_link(repo, integration_service, link)
        .await
        .map(Some)
}

pub async fn refresh_jira_issue_link(
    repo: &Arc<dyn AgentConversationJiraIssueRepository>,
    integration_service: &AtlassianIntegrationService,
    mut link: AgentConversationJiraIssueLink,
) -> AppResult<AgentConversationJiraIssueLink> {
    let reference = assigned_issue_to_composer_reference(&link);
    let now = Utc::now();
    match integration_service.fetch_resource_content(&reference).await {
        Ok(content) => {
            link.issue_key = content
                .key
                .clone()
                .unwrap_or_else(|| link.issue_key.clone())
                .to_ascii_uppercase();
            link.issue_url = content.url.or(link.issue_url);
            link.title = Some(content.title);
            link.status = content.status;
            link.assignee = content.assignee;
            link.reporter = content.reporter;
            link.updated_at_remote = content.updated_at_remote;
            link.description_markdown = content.description_markdown;
            link.description_text = content.description_text;
            link.acceptance_criteria_markdown = content.acceptance_criteria_markdown;
            link.acceptance_criteria_text = content.acceptance_criteria_text;
            link.comments_json =
                serde_json::to_string(&content.comments).unwrap_or_else(|_| "[]".to_string());
            link.attachments_json =
                serde_json::to_string(&content.attachments).unwrap_or_else(|_| "[]".to_string());
            link.last_refreshed_at = Some(now);
            link.refresh_status = AgentConversationJiraRefreshStatus::Loaded;
            link.refresh_error = None;
            link.updated_at = now;
        }
        Err(error) => {
            link.refresh_status = AgentConversationJiraRefreshStatus::Error;
            link.refresh_error = Some(error);
            link.updated_at = now;
        }
    }
    repo.upsert(link).await
}

pub fn manual_link_from_reference(
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    reference: ComposerJiraReferenceMetadata,
    assigned_at: DateTime<Utc>,
) -> AgentConversationJiraIssueLink {
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
    reference: ComposerJiraReferenceMetadata,
    assigned_from_message_id: Option<ChatMessageId>,
    assigned_at: DateTime<Utc>,
    manually_assigned: bool,
) -> AgentConversationJiraIssueLink {
    AgentConversationJiraIssueLink::new(
        conversation_id.clone(),
        project_id.clone(),
        reference.issue_key,
        assigned_at,
    )
    .with_reference_metadata(reference.issue_id, reference.title, reference.url)
    .with_assignment_source(assigned_from_message_id, manually_assigned)
}

pub fn assigned_issue_to_composer_reference(
    link: &AgentConversationJiraIssueLink,
) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: "jira".to_string(),
        id: link
            .issue_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| link.issue_key.clone()),
        key: Some(link.issue_key.clone()),
        title: link.title.clone(),
        url: link.issue_url.clone(),
        summary_excerpt: None,
        include_transcript: None,
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }
}

pub fn merge_assigned_jira_reference(
    assigned: Option<&AgentConversationJiraIssueLink>,
    turn_references: &[ComposerIntegrationReference],
) -> Vec<ComposerIntegrationReference> {
    let Some(assigned) = assigned else {
        return turn_references.to_vec();
    };
    let assigned_key = assigned.issue_key.to_ascii_uppercase();
    let mut merged = Vec::with_capacity(turn_references.len() + 1);
    merged.push(assigned_issue_to_composer_reference(assigned));
    for reference in turn_references {
        let same_jira = jira_reference_from_composer_reference(reference)
            .map(|reference| reference.issue_key.eq_ignore_ascii_case(&assigned_key))
            .unwrap_or(false);
        if !same_jira {
            merged.push(reference.clone());
        }
    }
    merged
}

#[cfg(test)]
#[path = "agent_conversation_jira_issue_tests.rs"]
mod tests;
