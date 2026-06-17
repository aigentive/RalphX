use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::domain::entities::{
    AgentConversationJiraIssueLink, ChatConversationId, ChatMessageId, ProjectId,
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
mod tests {
    use super::*;

    fn jira_ref(key: &str) -> ComposerIntegrationReference {
        ComposerIntegrationReference {
            provider: "atlassian".to_string(),
            kind: "jira".to_string(),
            id: key.to_string(),
            key: Some(key.to_string()),
            title: Some(format!("{key} title")),
            url: Some(format!("https://jira.test/browse/{key}")),
        }
    }

    #[test]
    fn merge_assigned_jira_reference_dedupes_same_turn_reference() {
        let assigned = AgentConversationJiraIssueLink::new(
            ChatConversationId::from_string("conv-1"),
            ProjectId::from_string("project-1".to_string()),
            "RX-42".to_string(),
            Utc::now(),
        );

        let merged = merge_assigned_jira_reference(Some(&assigned), &[jira_ref("rx-42")]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].key.as_deref(), Some("RX-42"));
    }

    #[test]
    fn merge_assigned_jira_reference_keeps_different_turn_references() {
        let assigned = AgentConversationJiraIssueLink::new(
            ChatConversationId::from_string("conv-1"),
            ProjectId::from_string("project-1".to_string()),
            "RX-42".to_string(),
            Utc::now(),
        );

        let merged = merge_assigned_jira_reference(Some(&assigned), &[jira_ref("RX-77")]);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].key.as_deref(), Some("RX-42"));
        assert_eq!(merged[1].key.as_deref(), Some("RX-77"));
    }
}
