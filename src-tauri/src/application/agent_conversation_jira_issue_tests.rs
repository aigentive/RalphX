use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use super::*;
use crate::infrastructure::memory::MemoryAgentConversationJiraIssueRepository;

fn assigned_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-17T12:00:00Z")
        .expect("timestamp")
        .with_timezone(&Utc)
}

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

fn confluence_ref(id: &str) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: "confluence".to_string(),
        id: id.to_string(),
        key: None,
        title: Some("Confluence page".to_string()),
        url: Some(format!("https://jira.test/wiki/{id}")),
    }
}

#[test]
fn merge_assigned_jira_reference_dedupes_same_turn_reference() {
    let assigned = AgentConversationJiraIssueLink::new(
        ChatConversationId::new(),
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
        ChatConversationId::new(),
        ProjectId::from_string("project-1".to_string()),
        "RX-42".to_string(),
        Utc::now(),
    );

    let merged = merge_assigned_jira_reference(Some(&assigned), &[jira_ref("RX-77")]);

    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].key.as_deref(), Some("RX-42"));
    assert_eq!(merged[1].key.as_deref(), Some("RX-77"));
}

#[tokio::test]
async fn assign_primary_jira_issue_uses_first_jira_reference_once() {
    let repo: Arc<dyn AgentConversationJiraIssueRepository> =
        Arc::new(MemoryAgentConversationJiraIssueRepository::new());
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());
    let message_id = ChatMessageId::from_string("msg-1");

    let assigned = assign_primary_jira_issue_if_absent(
        &repo,
        &conversation_id,
        &project_id,
        &[
            confluence_ref("page-1"),
            jira_ref("RX-42"),
            jira_ref("RX-77"),
        ],
        Some(message_id.clone()),
        assigned_at(),
    )
    .await
    .expect("assign primary")
    .expect("assigned link");

    assert_eq!(assigned.issue_key, "RX-42");
    assert_eq!(assigned.title.as_deref(), Some("RX-42 title"));
    assert_eq!(
        assigned
            .assigned_from_message_id
            .as_ref()
            .map(ChatMessageId::as_str),
        Some(message_id.as_str())
    );
    assert!(!assigned.manually_assigned);

    let returned_existing = assign_primary_jira_issue_if_absent(
        &repo,
        &conversation_id,
        &project_id,
        &[jira_ref("RX-77")],
        Some(ChatMessageId::from_string("msg-2")),
        assigned_at() + Duration::minutes(1),
    )
    .await
    .expect("second assign")
    .expect("existing link");

    assert_eq!(returned_existing.issue_key, "RX-42");
    let stored = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load link")
        .expect("stored link");
    assert_eq!(stored.issue_key, "RX-42");
    assert_eq!(
        stored
            .assigned_from_message_id
            .as_ref()
            .map(ChatMessageId::as_str),
        Some("msg-1")
    );
}

#[tokio::test]
async fn assign_primary_jira_issue_ignores_messages_without_jira_references() {
    let repo: Arc<dyn AgentConversationJiraIssueRepository> =
        Arc::new(MemoryAgentConversationJiraIssueRepository::new());
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());

    let assigned = assign_primary_jira_issue_if_absent(
        &repo,
        &conversation_id,
        &project_id,
        &[confluence_ref("page-1")],
        None,
        assigned_at(),
    )
    .await
    .expect("assign skipped");

    assert!(assigned.is_none());
    assert!(repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load link")
        .is_none());
}

#[test]
fn manual_link_from_reference_marks_manual_assignment_metadata() {
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());
    let link = manual_link_from_reference(
        &conversation_id,
        &project_id,
        ComposerJiraReferenceMetadata {
            issue_key: "RX-42".to_string(),
            issue_id: Some("10042".to_string()),
            title: Some("Fix Jira tab".to_string()),
            url: Some("https://jira.test/browse/RX-42".to_string()),
        },
        assigned_at(),
    );

    assert_eq!(link.conversation_id, conversation_id);
    assert_eq!(link.project_id, project_id);
    assert_eq!(link.issue_key, "RX-42");
    assert_eq!(link.issue_id.as_deref(), Some("10042"));
    assert_eq!(link.title.as_deref(), Some("Fix Jira tab"));
    assert!(link.manually_assigned);
    assert!(link.assigned_from_message_id.is_none());
}
