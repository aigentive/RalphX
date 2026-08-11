use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use tokio::sync::RwLock;

use super::*;
use crate::application::{
    LinearApiClient, LinearAuthContext, LinearIntegrationService, LinearIntegrationSettings,
    LinearIntegrationSettingsRepository, LinearIssueContent, LinearIssueSummary,
};
use crate::domain::entities::AgentConversationJiraIssueLink;
use crate::infrastructure::memory::{
    MemoryAgentConversationLinearIssueRepository, MemorySecretStore,
};

#[derive(Default)]
struct TestLinearSettingsRepo {
    settings: RwLock<LinearIntegrationSettings>,
}

#[async_trait]
impl LinearIntegrationSettingsRepository for TestLinearSettingsRepo {
    async fn get(&self) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &LinearIntegrationSettings,
    ) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        *self.settings.write().await = settings.clone();
        Ok(settings.clone())
    }
}

struct RichLinearClient;

#[async_trait]
impl LinearApiClient for RichLinearClient {
    async fn validate(&self, _auth: &LinearAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn search_issues(
        &self,
        _auth: &LinearAuthContext,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<LinearIssueSummary>, String> {
        Ok(Vec::new())
    }

    async fn fetch_issue(
        &self,
        _auth: &LinearAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<LinearIssueContent, String> {
        Ok(LinearIssueContent {
            id: reference.id.clone(),
            key: reference.key.clone(),
            title: reference
                .title
                .clone()
                .unwrap_or_else(|| reference.id.clone()),
            url: reference.url.clone(),
            body: "Issue body".to_string(),
            state_name: Some("In Progress".to_string()),
            assignee: Some("A. User".to_string()),
            creator: Some("C. User".to_string()),
            updated_at: Some("2026-06-18T08:00:00Z".to_string()),
            comments: Vec::new(),
            attachments: Vec::new(),
            labels: Vec::new(),
            project: None,
        })
    }
}

fn assigned_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-18T18:14:05Z")
        .expect("timestamp")
        .with_timezone(&Utc)
}

fn linear_ref(id: &str, key: Option<&str>) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "linear".to_string(),
        kind: "linear".to_string(),
        id: id.to_string(),
        key: key.map(str::to_string),
        title: Some(format!("{} title", key.unwrap_or(id))),
        url: key.map(|value| format!("https://linear.app/acme/issue/{value}/example")),
        summary_excerpt: None,
        include_transcript: None,
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }
}

fn jira_ref(key: &str) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: "jira".to_string(),
        id: key.to_string(),
        key: Some(key.to_string()),
        title: Some(format!("{key} title")),
        url: Some(format!("https://jira.test/browse/{key}")),
        summary_excerpt: None,
        include_transcript: None,
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }
}

#[test]
fn merge_assigned_linear_reference_dedupes_same_turn_reference_by_id() {
    let assigned = AgentConversationLinearIssueLink::new(
        ChatConversationId::new(),
        ProjectId::from_string("project-1".to_string()),
        "539068e2-ae88-4d09-bd75-22eb4a59612f".to_string(),
        Utc::now(),
    )
    .with_reference_metadata(Some("LIN-123".to_string()), None, None);

    let merged = merge_assigned_linear_reference(
        Some(&assigned),
        &[linear_ref(
            "539068e2-ae88-4d09-bd75-22eb4a59612f",
            Some("LIN-123"),
        )],
    );

    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f".to_string()
    );
    assert_eq!(merged[0].key.as_deref(), Some("LIN-123"));
}

#[test]
fn merge_assigned_linear_reference_preserves_cross_provider_references() {
    let assigned = AgentConversationLinearIssueLink::new(
        ChatConversationId::new(),
        ProjectId::from_string("project-1".to_string()),
        "539068e2-ae88-4d09-bd75-22eb4a59612f".to_string(),
        Utc::now(),
    )
    .with_reference_metadata(Some("LIN-123".to_string()), None, None);

    let merged = merge_assigned_linear_reference(
        Some(&assigned),
        &[
            jira_ref("RX-42"),
            linear_ref("639068e2-ae88-4d09-bd75-22eb4a59612f", Some("LIN-456")),
        ],
    );

    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].provider, "linear");
    assert_eq!(merged[1].provider, "atlassian");
    assert_eq!(merged[2].key.as_deref(), Some("LIN-456"));
}

#[test]
fn assigned_jira_and_linear_references_coexist_after_provider_scoped_merges() {
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());
    let assigned_jira = AgentConversationJiraIssueLink::new(
        conversation_id.clone(),
        project_id.clone(),
        "RX-42".to_string(),
        Utc::now(),
    )
    .with_reference_metadata(Some("10042".to_string()), None, None);
    let assigned_linear = AgentConversationLinearIssueLink::new(
        conversation_id,
        project_id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f".to_string(),
        Utc::now(),
    )
    .with_reference_metadata(Some("LIN-123".to_string()), None, None);

    let with_jira =
        crate::application::agent_conversation_jira_issue::merge_assigned_jira_reference(
            Some(&assigned_jira),
            &[
                jira_ref("RX-42"),
                linear_ref("639068e2-ae88-4d09-bd75-22eb4a59612f", Some("LIN-456")),
            ],
        );
    let merged = merge_assigned_linear_reference(Some(&assigned_linear), &with_jira);

    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].provider, "linear");
    assert_eq!(merged[0].key.as_deref(), Some("LIN-123"));
    assert_eq!(merged[1].provider, "atlassian");
    assert_eq!(merged[1].key.as_deref(), Some("RX-42"));
    assert_eq!(merged[2].provider, "linear");
    assert_eq!(merged[2].key.as_deref(), Some("LIN-456"));
}

#[tokio::test]
async fn assign_primary_linear_issue_uses_first_linear_reference_once() {
    let repo: Arc<dyn AgentConversationLinearIssueRepository> =
        Arc::new(MemoryAgentConversationLinearIssueRepository::new());
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());
    let message_id = ChatMessageId::from_string("msg-1");

    let assigned = assign_primary_linear_issue_if_absent(
        &repo,
        &conversation_id,
        &project_id,
        &[
            jira_ref("RX-42"),
            linear_ref("539068e2-ae88-4d09-bd75-22eb4a59612f", Some("LIN-123")),
            linear_ref("639068e2-ae88-4d09-bd75-22eb4a59612f", Some("LIN-456")),
        ],
        Some(message_id.clone()),
        assigned_at(),
    )
    .await
    .expect("assign primary")
    .expect("assigned link");

    assert_eq!(assigned.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(assigned.issue_key.as_deref(), Some("LIN-123"));
    assert_eq!(
        assigned
            .assigned_from_message_id
            .as_ref()
            .map(ChatMessageId::as_str),
        Some(message_id.as_str())
    );
    assert!(!assigned.manually_assigned);

    let returned_existing = assign_primary_linear_issue_if_absent(
        &repo,
        &conversation_id,
        &project_id,
        &[linear_ref(
            "639068e2-ae88-4d09-bd75-22eb4a59612f",
            Some("LIN-456"),
        )],
        Some(ChatMessageId::from_string("msg-2")),
        assigned_at() + Duration::minutes(1),
    )
    .await
    .expect("second assign")
    .expect("existing link");

    assert_eq!(
        returned_existing.issue_id,
        "539068e2-ae88-4d09-bd75-22eb4a59612f"
    );
    let stored = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load link")
        .expect("stored link");
    assert_eq!(
        stored
            .assigned_from_message_id
            .as_ref()
            .map(ChatMessageId::as_str),
        Some("msg-1")
    );
}

#[tokio::test]
async fn assign_primary_linear_issue_fetches_details_at_link_time() {
    let linear_integration_service = LinearIntegrationService::new(
        Arc::new(TestLinearSettingsRepo::default()),
        Arc::new(MemorySecretStore::new()),
        Arc::new(RichLinearClient),
    );
    linear_integration_service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .expect("save Linear settings");
    linear_integration_service
        .validate_and_enable()
        .await
        .expect("validate Linear settings");
    let repo: Arc<dyn AgentConversationLinearIssueRepository> =
        Arc::new(MemoryAgentConversationLinearIssueRepository::new());
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());

    let assigned = assign_primary_linear_issue_if_absent_and_refresh(
        &repo,
        Some(&linear_integration_service),
        &conversation_id,
        &project_id,
        &[linear_ref(
            "539068e2-ae88-4d09-bd75-22eb4a59612f",
            Some("LIN-123"),
        )],
        Some(ChatMessageId::from_string("msg-1")),
        assigned_at(),
    )
    .await
    .expect("assign primary")
    .expect("assigned link");

    assert_eq!(
        assigned.refresh_status,
        AgentConversationLinearRefreshStatus::Loaded
    );
    assert_eq!(assigned.title.as_deref(), Some("LIN-123 title"));
    assert_eq!(assigned.assignee.as_deref(), Some("A. User"));
    assert_eq!(assigned.reporter.as_deref(), Some("C. User"));
    assert_eq!(
        assigned.updated_at_remote.as_deref(),
        Some("2026-06-18T08:00:00Z")
    );
    assert!(assigned.last_refreshed_at.is_some());
}

#[tokio::test]
async fn assign_primary_linear_issue_ignores_invalid_and_non_linear_references() {
    let repo: Arc<dyn AgentConversationLinearIssueRepository> =
        Arc::new(MemoryAgentConversationLinearIssueRepository::new());
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-1".to_string());

    let assigned = assign_primary_linear_issue_if_absent(
        &repo,
        &conversation_id,
        &project_id,
        &[jira_ref("RX-42"), linear_ref("bad\nvalue", Some("LIN-123"))],
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
        ComposerLinearReferenceMetadata {
            issue_id: "539068e2-ae88-4d09-bd75-22eb4a59612f".to_string(),
            issue_key: Some("LIN-123".to_string()),
            title: Some("Fix Linear tab".to_string()),
            url: Some("https://linear.app/acme/issue/LIN-123/example".to_string()),
        },
        assigned_at(),
    );

    assert_eq!(link.conversation_id, conversation_id);
    assert_eq!(link.project_id, project_id);
    assert_eq!(link.issue_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(link.issue_key.as_deref(), Some("LIN-123"));
    assert_eq!(link.title.as_deref(), Some("Fix Linear tab"));
    assert!(link.manually_assigned);
    assert!(link.assigned_from_message_id.is_none());
}
