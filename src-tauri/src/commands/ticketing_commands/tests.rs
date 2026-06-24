use super::*;
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use crate::application::clickup_integration_service::ClickUpAttachment;
use crate::application::linear_integration_service::LinearAttachment;
use crate::application::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianIntegrationService,
    AtlassianOAuthResource, AtlassianOAuthTokenResponse, AppState, AtlassianJiraAttachment,
    AtlassianJiraComment, ClickUpComment, ClickUpSpace, ClickUpStatus, ClickUpTaskContent,
    ClickUpTaskSummary, ClickUpUser, JiraIssueDetail, JiraProjectSummary, JiraStatusSummary,
    AtlassianResourceContent, AtlassianResourceKind, AtlassianResourceSummary,
    LinearApiClient, LinearAuthContext, LinearIntegrationService, LinearIntegrationSettings,
    LinearIntegrationSettingsRepository, LinearIssueContent, LinearIssueSummary, LinearProject,
    TeamService, TeamStateTracker, TicketingLabelResult, TicketingMutationResult,
    TicketingTicketIdentity, TicketingTransitionOption,
};
use crate::commands::unified_chat_commands::StartAgentConversationInput;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::domain::integrations::{
    AtlassianAuthMethod, AtlassianIntegrationSettings, AtlassianIntegrationSettingsRepository,
    ClickUpIntegrationSettings, IntegrationValidationStatus, ProviderTicketOperation,
    ProviderTicketOperationKind, ProviderTicketOperationStatus,
};
use crate::domain::services::{ComposerIntegrationReference, SecretStore};
use crate::infrastructure::memory::{
    MemoryAtlassianIntegrationSettingsRepository, MemoryLinearIntegrationSettingsRepository,
    MemorySecretStore,
};
use crate::tests::mock_github_service::MockGithubService;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

#[test]
fn provider_summaries_reflect_existing_integration_settings() {
    let jira = AtlassianIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Valid,
        jira_available: true,
        ..Default::default()
    };

    let linear = LinearIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Invalid,
        last_error: Some("Token rejected".to_string()),
        ..Default::default()
    };

    let jira_summary = jira_provider_summary(&jira);
    let linear_summary = linear_provider_summary(&linear);

    assert_eq!(jira_summary.provider, "jira");
    assert_eq!(jira_summary.connection_status, "connected");
    assert!(jira_summary.capabilities.supports_kanban);
    assert!(jira_summary.capabilities.status_write);
    assert!(jira_summary.capabilities.assignment_write);
    assert!(jira_summary.capabilities.comment_write);
    assert!(jira_summary.capabilities.label_write);
    assert_eq!(linear_summary.provider, "linear");
    assert_eq!(linear_summary.connection_status, "error");
    assert!(!linear_summary.capabilities.status_write);
    assert!(!linear_summary.capabilities.label_write);
    assert_eq!(
        linear_summary.error_message.as_deref(),
        Some("Token rejected")
    );
}

#[test]
fn capability_helpers_expose_label_write_flag() {
    assert!(writable_capabilities("manual").label_write);
    assert!(!read_only_capabilities("manual").label_write);
}

#[test]
fn ticketing_columns_return_provider_neutral_defaults() {
    let columns = default_ticketing_columns();

    assert_eq!(columns.len(), 4);
    assert_eq!(columns[0].id, "todo");
    assert_eq!(columns[1].category, "in_progress");
    assert_eq!(columns[2].category, "done");
}

#[test]
fn provider_validation_rejects_unknown_ticketing_provider() {
    let error = validate_provider("github").expect_err("unknown provider should fail");

    assert!(error.contains("Unknown ticketing provider"));
}

#[test]
fn provider_validation_accepts_clickup() {
    validate_provider("clickup").expect("clickup is a supported ticketing provider");
}

#[test]
fn clickup_ticket_ref_maps_to_clickup_composer_reference() {
    let reference = ticket_ref_to_composer_reference(
        "clickup",
        &TicketRefInput {
            provider: "clickup".to_string(),
            id: "task-1".to_string(),
            key: Some("RX-7".to_string()),
        },
    );

    assert_eq!(reference.provider, "clickup");
    assert_eq!(reference.kind, "clickup");
    assert_eq!(reference.id, "task-1");
    assert_eq!(reference.key.as_deref(), Some("RX-7"));
}

#[test]
fn clickup_provider_summary_reports_writeback_caps_with_manual_freshness() {
    let settings = ClickUpIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Valid,
        task_search_available: true,
        ..Default::default()
    };

    let summary = clickup_provider_summary(&settings);

    assert_eq!(summary.provider, "clickup");
    assert_eq!(summary.label, "ClickUp");
    assert_eq!(summary.connection_status, "connected");
    assert!(summary.enabled);
    // Full write-back parity (transition/assign/comment/tags) is exposed.
    assert!(summary.capabilities.status_write);
    assert!(summary.capabilities.assignment_write);
    assert!(summary.capabilities.comment_write);
    assert!(summary.capabilities.label_write);
    assert!(summary.capabilities.supports_kanban);
    // ClickUp has no webhook reconciliation, so freshness is manual like Jira. The
    // deferred start-work/conversation-link affordance is gated client-side, so
    // there is no backend capability flag for it to assert here.
    assert_eq!(summary.capabilities.freshness, "manual");
    assert!(summary.permission_message.is_none());
    assert!(summary.error_message.is_none());
}

#[test]
fn clickup_provider_summary_reflects_disabled_error_and_limited_states() {
    let disconnected = clickup_provider_summary(&ClickUpIntegrationSettings::default());
    assert_eq!(disconnected.connection_status, "disconnected");
    assert!(!disconnected.enabled);
    // No accidental write affordance when not connected.
    assert!(!disconnected.capabilities.status_write);
    assert!(!disconnected.capabilities.label_write);

    let errored = clickup_provider_summary(&ClickUpIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Invalid,
        last_error: Some("Token rejected".to_string()),
        ..Default::default()
    });
    assert_eq!(errored.connection_status, "error");
    assert!(!errored.enabled);
    assert_eq!(errored.error_message.as_deref(), Some("Token rejected"));

    let limited = clickup_provider_summary(&ClickUpIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Valid,
        task_search_available: false,
        ..Default::default()
    });
    assert_eq!(limited.connection_status, "permission_limited");
    assert!(!limited.enabled);
    assert!(limited.permission_message.is_some());
}

#[test]
fn clickup_space_maps_to_project_container() {
    let container = clickup_space_to_container(ClickUpSpace {
        id: "space-1".to_string(),
        name: "Platform".to_string(),
        private: false,
    });

    assert_eq!(container.provider, "clickup");
    assert_eq!(container.id, "space-1");
    assert!(container.key.is_none());
    assert_eq!(container.name, "Platform");
    // Spaces reuse the existing `project` kind (no shared enum widening).
    assert_eq!(container.kind, "project");
    assert!(container.parent_id.is_none());
}

#[test]
fn clickup_status_maps_into_column_with_name_derived_id() {
    let column = clickup_status_to_column(
        ClickUpStatus {
            id: Some("status-99".to_string()),
            status: "In Progress".to_string(),
            status_type: "custom".to_string(),
            category: "in_progress".to_string(),
            color: Some("#abcdef".to_string()),
            orderindex: Some(1),
        },
        1,
    );

    // The column id is derived from the status NAME (not the optional ClickUp
    // status id) so it matches the ticket state id for kanban grouping.
    assert_eq!(column.id, state_id("In Progress"));
    assert_eq!(column.name, "In Progress");
    assert_eq!(column.category, "in_progress");
    assert_eq!(column.order, 1);
    assert_eq!(column.color.as_deref(), Some("#abcdef"));
}

#[test]
fn clickup_summary_maps_status_assignee_tags_and_project() {
    let ticket = clickup_summary_to_ticket(ClickUpTaskSummary {
        id: "task-1".to_string(),
        custom_id: Some("RX-7".to_string()),
        name: "Wire ClickUp dashboard".to_string(),
        url: Some("https://app.clickup.com/t/task-1".to_string()),
        status_name: Some("In Progress".to_string()),
        status_type: Some("custom".to_string()),
        status_category: Some("in_progress".to_string()),
        status_color: Some("#112233".to_string()),
        assignees: vec!["Reef Agent".to_string(), "Second Person".to_string()],
        assignee_ids: vec![42, 7],
        tags: vec!["backend".to_string(), "clickup".to_string()],
        space_id: Some("space-1".to_string()),
        list_name: Some("Sprint 1".to_string()),
        updated_at: Some("2026-06-20T12:00:00Z".to_string()),
    });

    assert_eq!(ticket.ref_.provider, "clickup");
    assert_eq!(ticket.ref_.id, "task-1");
    // ClickUp custom id is the human-readable key.
    assert_eq!(ticket.ref_.key.as_deref(), Some("RX-7"));
    assert_eq!(ticket.title, "Wire ClickUp dashboard");
    // State id is name-derived so it aligns with the column id for kanban grouping.
    assert_eq!(ticket.state.id, state_id("In Progress"));
    assert_eq!(ticket.state.name, "In Progress");
    // Category comes from the already-derived status.type mapping.
    assert_eq!(ticket.state.category, "in_progress");
    assert_eq!(ticket.state.color.as_deref(), Some("#112233"));
    // The first assignee still fills the legacy single assignee slot.
    assert_eq!(
        ticket.assignee.as_ref().map(|person| person.name.as_str()),
        Some("Reef Agent")
    );
    assert_eq!(
        ticket
            .assignees
            .iter()
            .map(|person| person.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Reef Agent", "Second Person"]
    );
    // ClickUp tags surface as labels.
    assert_eq!(
        ticket.labels,
        vec!["backend".to_string(), "clickup".to_string()]
    );
    assert_eq!(ticket.project.as_deref(), Some("Sprint 1"));
    assert_eq!(ticket.updated_at, "2026-06-20T12:00:00Z");
    assert_eq!(
        ticket.url.as_deref(),
        Some("https://app.clickup.com/t/task-1")
    );
    assert_eq!(ticket.association_count, 0);
    assert!(!ticket.current_user_assigned);
}

#[test]
fn clickup_summary_detects_current_user_assignment_by_username_or_email() {
    let summary = ClickUpTaskSummary {
        id: "task-1".to_string(),
        custom_id: None,
        name: "Current user task".to_string(),
        url: None,
        status_name: None,
        status_type: None,
        status_category: None,
        status_color: None,
        assignees: vec!["agent@example.com".to_string()],
        assignee_ids: Vec::new(),
        tags: Vec::new(),
        space_id: None,
        list_name: Some("Sprint 42".to_string()),
        updated_at: None,
    };
    let user = ClickUpUser {
        id: 42,
        username: Some("Agent".to_string()),
        email: Some("agent@example.com".to_string()),
    };
    let other = ClickUpUser {
        id: 43,
        username: Some("Someone Else".to_string()),
        email: Some("else@example.com".to_string()),
    };

    assert!(clickup_summary_assigned_to_user(&summary, &user));
    assert!(!clickup_summary_assigned_to_user(&summary, &other));

    let id_only_summary = ClickUpTaskSummary {
        assignees: vec!["A".to_string()],
        assignee_ids: vec![42],
        ..summary
    };
    assert!(clickup_summary_assigned_to_user(&id_only_summary, &user));

    let username_summary = ClickUpTaskSummary {
        assignees: vec!["AGENT".to_string()],
        assignee_ids: Vec::new(),
        ..id_only_summary
    };
    assert!(clickup_summary_assigned_to_user(&username_summary, &user));
}

#[test]
fn clickup_summary_derives_state_when_status_fields_missing() {
    let ticket = clickup_summary_to_ticket(ClickUpTaskSummary {
        id: "task-2".to_string(),
        custom_id: None,
        name: "No status".to_string(),
        url: None,
        status_name: None,
        status_type: None,
        status_category: None,
        status_color: None,
        assignees: Vec::new(),
        assignee_ids: Vec::new(),
        tags: Vec::new(),
        space_id: None,
        list_name: None,
        updated_at: None,
    });

    assert!(ticket.ref_.key.is_none());
    assert_eq!(ticket.state.name, "Provider result");
    assert_eq!(ticket.state.category, state_category("Provider result"));
    assert_eq!(ticket.state.id, state_id("Provider result"));
    assert!(ticket.assignee.is_none());
    assert!(ticket.labels.is_empty());
    assert!(ticket.project.is_none());
    assert!(!ticket.updated_at.is_empty());
}

#[test]
fn clickup_content_maps_description_comments_and_creator() {
    let detail = clickup_content_to_detail(ClickUpTaskContent {
        id: "task-1".to_string(),
        custom_id: Some("RX-7".to_string()),
        name: "Wire ClickUp dashboard".to_string(),
        url: Some("https://app.clickup.com/t/task-1".to_string()),
        description: "Implement the ClickUp arms.".to_string(),
        status_name: Some("Done".to_string()),
        status_type: Some("done".to_string()),
        status_category: Some("done".to_string()),
        creator: Some("Reporter Person".to_string()),
        assignees: vec!["Reef Agent".to_string()],
        tags: vec!["backend".to_string()],
        comments: vec![ClickUpComment {
            id: "comment-1".to_string(),
            body: "Looks good".to_string(),
            author_id: Some(7),
            author_name: Some("Commenter".to_string()),
            created_at: Some("2026-06-21T09:00:00Z".to_string()),
            attachments: vec![ClickUpAttachment {
                id: Some("comment-att-1".to_string()),
                filename: "comment-image.jpg".to_string(),
                mime_type: Some("image/jpeg".to_string()),
                size: Some(1024),
                url: Some("https://attachments.clickup.test/comment-image.jpg".to_string()),
            }],
            replies: vec![ClickUpComment {
                id: "reply-1".to_string(),
                body: "Thread reply".to_string(),
                author_id: Some(8),
                author_name: Some("Responder".to_string()),
                created_at: Some("2026-06-21T09:05:00Z".to_string()),
                attachments: Vec::new(),
                replies: Vec::new(),
            }],
        }],
        attachments: vec![ClickUpAttachment {
            id: Some("att-1".to_string()),
            filename: "screenshot.png".to_string(),
            mime_type: Some("image/png".to_string()),
            size: Some(4096),
            url: Some("https://attachments.clickup.test/screenshot.png".to_string()),
        }],
        updated_at: Some("2026-06-21T10:00:00Z".to_string()),
        space_id: Some("space-1".to_string()),
        list_name: Some("Sprint 1".to_string()),
    });

    assert_eq!(detail.summary.ref_.provider, "clickup");
    assert_eq!(detail.summary.ref_.id, "task-1");
    assert_eq!(detail.summary.ref_.key.as_deref(), Some("RX-7"));
    assert_eq!(detail.summary.state.category, "done");
    assert_eq!(detail.summary.state.id, state_id("Done"));
    assert_eq!(
        detail
            .summary
            .reporter
            .as_ref()
            .map(|person| person.name.as_str()),
        Some("Reporter Person")
    );
    assert_eq!(
        detail
            .summary
            .assignee
            .as_ref()
            .map(|person| person.name.as_str()),
        Some("Reef Agent")
    );
    assert_eq!(detail.summary.labels, vec!["backend".to_string()]);
    assert_eq!(
        detail.description_markdown.as_deref(),
        Some("Implement the ClickUp arms.")
    );
    assert_eq!(
        detail.description_text.as_deref(),
        Some("Implement the ClickUp arms.")
    );
    assert!(detail.acceptance_criteria_markdown.is_none());
    assert_eq!(detail.attachments.len(), 1);
    assert_eq!(detail.attachments[0].filename, "screenshot.png");
    assert_eq!(
        detail.attachments[0].mime_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(detail.attachments[0].size, Some(4096));
    assert_eq!(
        detail.attachments[0].url.as_deref(),
        Some("https://attachments.clickup.test/screenshot.png")
    );
    assert!(detail.transitions.is_empty());
    assert_eq!(detail.comments.len(), 1);
    let comment = &detail.comments[0];
    assert_eq!(comment.id.as_deref(), Some("comment-1"));
    assert_eq!(comment.body_markdown, "Looks good");
    assert_eq!(comment.body_text, "Looks good");
    assert_eq!(comment.replies.len(), 1);
    assert_eq!(comment.replies[0].body_text, "Thread reply");
    assert_eq!(comment.attachments.len(), 1);
    assert_eq!(comment.attachments[0].filename, "comment-image.jpg");
    assert_eq!(
        comment.author.as_ref().map(|person| person.name.as_str()),
        Some("Commenter")
    );
    assert_eq!(comment.created_at.as_deref(), Some("2026-06-21T09:00:00Z"));
    assert!(comment.updated_at.is_none());
}

#[test]
fn clickup_content_maps_empty_provider_payload_with_fallbacks() {
    let detail = clickup_content_to_detail(ClickUpTaskContent {
        id: "task-empty".to_string(),
        custom_id: None,
        name: "Sparse ClickUp task".to_string(),
        url: None,
        description: String::new(),
        status_name: None,
        status_type: None,
        status_category: None,
        creator: None,
        assignees: Vec::new(),
        tags: Vec::new(),
        comments: Vec::new(),
        attachments: Vec::new(),
        updated_at: None,
        space_id: None,
        list_name: None,
    });

    assert_eq!(detail.summary.ref_.provider, "clickup");
    assert_eq!(detail.summary.ref_.id, "task-empty");
    assert!(detail.summary.ref_.key.is_none());
    assert_eq!(detail.summary.state.name, "Provider result");
    assert_eq!(detail.summary.state.id, state_id("Provider result"));
    assert_eq!(
        detail.summary.state.category,
        state_category("Provider result")
    );
    assert!(detail.summary.assignee.is_none());
    assert!(detail.summary.assignees.is_empty());
    assert!(detail.summary.reporter.is_none());
    assert!(detail.summary.labels.is_empty());
    assert!(detail.summary.project.is_none());
    assert!(detail.summary.url.is_none());
    assert!(!detail.summary.updated_at.is_empty());
    assert_eq!(detail.description_markdown.as_deref(), Some(""));
    assert_eq!(detail.description_text.as_deref(), Some(""));
    assert!(detail.comments.is_empty());
    assert!(detail.attachments.is_empty());
    assert!(detail.transitions.is_empty());
    assert!(detail.fetched_at.is_some());
}

#[test]
fn clickup_comment_mapper_preserves_sparse_nested_comments() {
    let comment = ticket_comment_from_clickup_comment(ClickUpComment {
        id: "root".to_string(),
        body: "Root".to_string(),
        author_id: None,
        author_name: None,
        created_at: None,
        attachments: vec![ClickUpAttachment {
            id: None,
            filename: "capture.png".to_string(),
            mime_type: None,
            size: None,
            url: None,
        }],
        replies: vec![ClickUpComment {
            id: "reply".to_string(),
            body: "Reply".to_string(),
            author_id: Some(99),
            author_name: Some("Responder".to_string()),
            created_at: Some("2026-06-23T12:00:00Z".to_string()),
            attachments: Vec::new(),
            replies: Vec::new(),
        }],
    });

    assert_eq!(comment.id.as_deref(), Some("root"));
    assert!(comment.author.is_none());
    assert!(comment.created_at.is_none());
    assert_eq!(comment.attachments.len(), 1);
    assert_eq!(comment.attachments[0].filename, "capture.png");
    assert!(comment.attachments[0].url.is_none());
    assert_eq!(comment.replies.len(), 1);
    assert_eq!(comment.replies[0].body_text, "Reply");
    assert_eq!(
        comment.replies[0]
            .author
            .as_ref()
            .map(|person| person.name.as_str()),
        Some("Responder")
    );
    assert_eq!(
        comment.replies[0].created_at.as_deref(),
        Some("2026-06-23T12:00:00Z")
    );
}

#[test]
fn clickup_ticket_state_id_aligns_with_column_id_for_kanban() {
    // Kanban groups tickets by `state.id == column.id`. ClickUp tasks carry no
    // status id, so both sides must derive the id from the same status name.
    let column = clickup_status_to_column(
        ClickUpStatus {
            id: None,
            status: "In Review".to_string(),
            status_type: "custom".to_string(),
            category: "in_progress".to_string(),
            color: None,
            orderindex: Some(2),
        },
        0,
    );
    let ticket = clickup_summary_to_ticket(ClickUpTaskSummary {
        id: "task-9".to_string(),
        custom_id: None,
        name: "Review me".to_string(),
        url: None,
        status_name: Some("In Review".to_string()),
        status_type: Some("custom".to_string()),
        status_category: Some("in_progress".to_string()),
        status_color: None,
        assignees: Vec::new(),
        assignee_ids: Vec::new(),
        tags: Vec::new(),
        space_id: Some("space-1".to_string()),
        list_name: None,
        updated_at: None,
    });

    assert_eq!(ticket.state.id, column.id);
    assert_eq!(ticket.state.category, column.category);
}

#[test]
fn clickup_batch_associations_resolve_to_empty_without_error() {
    // ClickUp conversation-linking is deferred, so batched association matching
    // returns empty rather than hitting the unknown-provider error path.
    let project_id = ProjectId::from_string("proj-clickup".to_string());
    let reference = ticket_ref_to_composer_reference(
        "clickup",
        &TicketRefInput {
            provider: "clickup".to_string(),
            id: "task-1".to_string(),
            key: None,
        },
    );

    let associations =
        linked_agent_conversation_associations_from_batch("clickup", &project_id, &reference, &[])
            .expect("clickup batch associations resolve without a provider error");

    assert!(associations.is_empty());
}

#[tokio::test]
async fn list_ticketing_providers_includes_clickup() {
    let state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let app = build_ticketing_start_app(state, execution_state);

    let providers = list_ticketing_providers(None, app.state())
        .await
        .expect("providers list resolves");

    let provider_ids: Vec<&str> = providers
        .iter()
        .map(|provider| provider.provider.as_str())
        .collect();
    assert!(provider_ids.contains(&"clickup"));
    assert!(provider_ids.contains(&"jira"));
    assert!(provider_ids.contains(&"linear"));
}

#[tokio::test]
async fn clickup_conversation_associations_are_empty_pending_followup() {
    // ClickUp conversation-linking is deferred, so ClickUp tickets must resolve
    // to zero associations (not a fallthrough provider error) so the unified list
    // can hydrate association counts for ClickUp like the other providers.
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-clickup-assoc").await;

    let associations = project_ticket_conversation_associations(&state, "clickup", &project_id)
        .await
        .expect("clickup associations resolve without a provider error");

    assert!(associations.is_empty());
}

#[derive(Default)]
struct FakeLinearTicketingClient {
    issues: Mutex<Vec<LinearIssueSummary>>,
    projects: Mutex<Vec<LinearProject>>,
    search_limits: Mutex<Vec<usize>>,
    list_projects_first: Mutex<Vec<usize>>,
}

#[async_trait]
impl LinearApiClient for FakeLinearTicketingClient {
    async fn validate(&self, _auth: &LinearAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn search_issues(
        &self,
        _auth: &LinearAuthContext,
        _query: &str,
        limit: usize,
    ) -> Result<Vec<LinearIssueSummary>, String> {
        self.search_limits.lock().unwrap().push(limit);
        Ok(self.issues.lock().unwrap().clone())
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
            body: String::new(),
            state_name: None,
            assignee: None,
            creator: None,
            updated_at: None,
            comments: Vec::new(),
            attachments: Vec::new(),
            labels: Vec::new(),
            project: None,
        })
    }

    async fn list_projects(
        &self,
        _auth: &LinearAuthContext,
        first: usize,
    ) -> Result<Vec<LinearProject>, String> {
        self.list_projects_first.lock().unwrap().push(first);
        Ok(self.projects.lock().unwrap().clone())
    }
}

#[derive(Default)]
struct FakeAtlassianTicketingClient {
    projects: Mutex<Vec<JiraProjectSummary>>,
    list_project_limits: Mutex<Vec<usize>>,
}

#[async_trait]
impl AtlassianApiClient for FakeAtlassianTicketingClient {
    async fn validate(
        &self,
        _auth: &AtlassianAuthContext,
    ) -> Result<AtlassianConnectivity, String> {
        Ok(AtlassianConnectivity {
            jira_available: true,
            confluence_available: true,
        })
    }

    async fn search(
        &self,
        _auth: &AtlassianAuthContext,
        _kind: AtlassianResourceKind,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, String> {
        Ok(Vec::new())
    }

    async fn fetch(
        &self,
        _auth: &AtlassianAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<AtlassianResourceContent, String> {
        Ok(AtlassianResourceContent {
            kind: reference
                .kind
                .parse::<AtlassianResourceKind>()
                .unwrap_or(AtlassianResourceKind::Jira),
            id: reference.id.clone(),
            key: reference.key.clone(),
            title: reference
                .title
                .clone()
                .unwrap_or_else(|| reference.id.clone()),
            url: reference.url.clone(),
            body: String::new(),
            status: None,
            assignee: None,
            reporter: None,
            updated_at_remote: None,
            description_markdown: None,
            description_text: None,
            acceptance_criteria_markdown: None,
            acceptance_criteria_text: None,
            comments: Vec::new(),
            attachments: Vec::new(),
        })
    }

    async fn assign_jira_issue_to_current_user(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn list_jira_projects(
        &self,
        _auth: &AtlassianAuthContext,
        limit: usize,
    ) -> Result<Vec<JiraProjectSummary>, String> {
        self.list_project_limits.lock().unwrap().push(limit);
        Ok(self.projects.lock().unwrap().clone())
    }

    async fn exchange_oauth_code(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _code: &str,
        _redirect_uri: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        Err("not used by ticketing command tests".to_string())
    }

    async fn refresh_oauth_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _refresh_token: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        Err("not used by ticketing command tests".to_string())
    }

    async fn oauth_accessible_resources(
        &self,
        _access_token: &str,
    ) -> Result<Vec<AtlassianOAuthResource>, String> {
        Ok(Vec::new())
    }
}

async fn valid_linear_service(
    client: Arc<FakeLinearTicketingClient>,
) -> Arc<LinearIntegrationService> {
    let repo = Arc::new(MemoryLinearIntegrationSettingsRepository::new());
    let secret_store = Arc::new(MemorySecretStore::new());
    secret_store
        .put_secret("linear-token", "token-value")
        .await
        .unwrap();
    repo.upsert(&LinearIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("linear-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        issue_search_available: true,
        ..Default::default()
    })
    .await
    .unwrap();
    LinearIntegrationService::new(repo, secret_store, client).into()
}

async fn valid_atlassian_service(
    client: Arc<FakeAtlassianTicketingClient>,
) -> Arc<AtlassianIntegrationService> {
    let repo = Arc::new(MemoryAtlassianIntegrationSettingsRepository::new());
    let secret_store = Arc::new(MemorySecretStore::new());
    secret_store
        .put_secret("atlassian-token", "token-value")
        .await
        .unwrap();
    repo.upsert(&AtlassianIntegrationSettings {
        enabled: true,
        auth_method: AtlassianAuthMethod::ApiToken,
        site_url: Some("https://example.atlassian.net".to_string()),
        email: Some("agent@example.com".to_string()),
        token_secret_ref: Some("atlassian-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        jira_available: true,
        confluence_available: true,
        ..Default::default()
    })
    .await
    .unwrap();
    AtlassianIntegrationService::new(repo, secret_store, client).into()
}

fn linear_issue_summary(
    id: &str,
    title: &str,
    assignee: Option<&str>,
    labels: &[&str],
) -> LinearIssueSummary {
    LinearIssueSummary {
        id: id.to_string(),
        key: Some(id.to_string()),
        title: title.to_string(),
        url: None,
        excerpt: None,
        state_id: Some("state-todo".to_string()),
        state_name: Some("Todo".to_string()),
        state_category: Some("todo".to_string()),
        state_color: None,
        assignee: assignee.map(str::to_string),
        updated_at: Some("2026-06-24T10:00:00Z".to_string()),
        labels: labels.iter().map(|label| label.to_string()).collect(),
        project: Some("Platform".to_string()),
    }
}

#[tokio::test]
async fn list_ticketing_containers_uses_expanded_provider_limits() {
    let linear_client = Arc::new(FakeLinearTicketingClient::default());
    linear_client
        .projects
        .lock()
        .unwrap()
        .push(LinearProject {
            id: "linear-project-1".to_string(),
            name: "Linear Project".to_string(),
        });
    let atlassian_client = Arc::new(FakeAtlassianTicketingClient::default());
    atlassian_client
        .projects
        .lock()
        .unwrap()
        .push(JiraProjectSummary {
            id: "10001".to_string(),
            key: "RX".to_string(),
            name: "RalphX".to_string(),
        });
    let mut state = AppState::new_test();
    state.linear_integration_service = valid_linear_service(Arc::clone(&linear_client)).await;
    state.atlassian_integration_service =
        valid_atlassian_service(Arc::clone(&atlassian_client)).await;
    let app = build_ticketing_start_app(state, Arc::new(ExecutionState::new()));

    let jira = list_ticketing_containers("jira".to_string(), None, app.state())
        .await
        .expect("jira containers should load");
    let linear = list_ticketing_containers("linear".to_string(), None, app.state())
        .await
        .expect("linear containers should load");

    assert_eq!(jira[0].id, "RX");
    assert_eq!(linear[0].id, "linear-project-1");
    assert_eq!(
        atlassian_client
            .list_project_limits
            .lock()
            .unwrap()
            .as_slice(),
        &[TICKETING_CONTAINER_LIMIT]
    );
    assert_eq!(
        linear_client
            .list_projects_first
            .lock()
            .unwrap()
            .as_slice(),
        &[TICKETING_CONTAINER_LIMIT]
    );
}

#[tokio::test]
async fn list_tickets_builds_paged_response_from_provider_summaries() {
    let linear_client = Arc::new(FakeLinearTicketingClient::default());
    linear_client.issues.lock().unwrap().extend([
        linear_issue_summary("LIN-1", "First ticket", Some("Ada"), &["backend"]),
        linear_issue_summary("LIN-2", "Second ticket", Some("Grace"), &["backend"]),
    ]);
    let mut state = AppState::new_test();
    state.linear_integration_service = valid_linear_service(Arc::clone(&linear_client)).await;
    let app = build_ticketing_start_app(state, Arc::new(ExecutionState::new()));

    let page = list_tickets(
        ListTicketsQuery {
            provider: PROVIDER_LINEAR.to_string(),
            project_id: None,
            container_id: None,
            cursor: None,
            limit: Some(1),
            filters: Some(TicketFiltersInput {
                text: Some("ticket".to_string()),
                assignee: None,
                state_ids: None,
                labels: Some(vec!["backend".to_string()]),
            }),
            sort: Some("updated".to_string()),
        },
        app.state(),
    )
    .await
    .expect("ticket page should load");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].ref_.id, "LIN-1");
    assert_eq!(page.total, Some(2));
    assert_eq!(page.next_cursor.as_deref(), Some("offset:1"));
    assert_eq!(linear_client.search_limits.lock().unwrap().as_slice(), &[2]);
}

#[tokio::test]
async fn list_ticket_filter_options_builds_truncated_provider_response() {
    let linear_client = Arc::new(FakeLinearTicketingClient::default());
    linear_client.issues.lock().unwrap().extend([
        linear_issue_summary("LIN-1", "First ticket", Some("Ada"), &[]),
        linear_issue_summary("LIN-2", "Second ticket", Some("Grace"), &[]),
    ]);
    let mut state = AppState::new_test();
    state.linear_integration_service = valid_linear_service(Arc::clone(&linear_client)).await;
    let app = build_ticketing_start_app(state, Arc::new(ExecutionState::new()));

    let options = list_ticket_filter_options(
        ListTicketFilterOptionsQuery {
            provider: PROVIDER_LINEAR.to_string(),
            project_id: Some("project-1".to_string()),
            container_id: None,
            limit: Some(1),
            filters: Some(TicketFiltersInput {
                text: Some("ticket".to_string()),
                assignee: None,
                state_ids: None,
                labels: None,
            }),
        },
        app.state(),
    )
    .await
    .expect("filter options should load");

    assert_eq!(options.assignees, vec!["Ada"]);
    assert!(options.sprints.is_empty());
    assert!(options.truncated);
    assert!(!options.complete);
    assert_eq!(linear_client.search_limits.lock().unwrap().as_slice(), &[2]);
}

#[test]
fn ticket_identity_preserves_project_scope_for_mutation_service() {
    let identity = ticket_identity(
        "linear",
        &TicketRefInput {
            provider: "linear".to_string(),
            id: "issue-1".to_string(),
            key: Some("LIN-1".to_string()),
        },
        Some("project-1".to_string()),
    );

    assert_eq!(identity.provider, "linear");
    assert_eq!(identity.id, "issue-1");
    assert_eq!(identity.key.as_deref(), Some("LIN-1"));
    assert_eq!(identity.local_project_id.as_deref(), Some("project-1"));
}

#[test]
fn mutation_response_maps_operation_status_and_linked_flag() {
    let now = chrono::Utc::now();
    let response = ticket_mutation_response(TicketingMutationResult {
        ticket: TicketingTicketIdentity {
            provider: "jira".to_string(),
            id: "10001".to_string(),
            key: Some("JRA-1".to_string()),
            local_project_id: Some("project-1".to_string()),
        },
        operation: ProviderTicketOperation {
            id: "operation-1".to_string(),
            provider: "jira".to_string(),
            external_kind: "jira".to_string(),
            external_id: "JRA-1".to_string(),
            external_key: Some("JRA-1".to_string()),
            link_id: Some("link-1".to_string()),
            local_project_id: Some("project-1".to_string()),
            operation: ProviderTicketOperationKind::Transition,
            client_operation_id: "client-op-1".to_string(),
            status: ProviderTicketOperationStatus::Succeeded,
            provider_operation_id: Some("31".to_string()),
            error_message: None,
            metadata_json: None,
            last_attempt_at: Some(now),
            completed_at: Some(now),
            created_at: now,
            updated_at: now,
        },
        idempotent: false,
        transition: Some(TicketingTransitionOption {
            to_state_id: "done".to_string(),
            provider_transition_id: Some("31".to_string()),
            name: "Done".to_string(),
            category: "done".to_string(),
            disabled_reason: None,
        }),
        assignee: None,
        comment: None,
        labels: None,
    });

    assert_eq!(response.ticket_ref.provider, "jira");
    assert_eq!(response.ticket_ref.key.as_deref(), Some("JRA-1"));
    assert_eq!(response.operation.operation, "transition");
    assert_eq!(response.operation.status, "succeeded");
    assert!(response.operation.linked);
    assert_eq!(
        response
            .transition
            .unwrap()
            .provider_transition_id
            .as_deref(),
        Some("31")
    );
}

#[test]
fn mutation_response_maps_label_result_payload() {
    let now = chrono::Utc::now();
    let response = ticket_mutation_response(TicketingMutationResult {
        ticket: TicketingTicketIdentity {
            provider: "jira".to_string(),
            id: "10001".to_string(),
            key: Some("JRA-1".to_string()),
            local_project_id: Some("project-1".to_string()),
        },
        operation: ProviderTicketOperation {
            id: "operation-1".to_string(),
            provider: "jira".to_string(),
            external_kind: "jira".to_string(),
            external_id: "JRA-1".to_string(),
            external_key: Some("JRA-1".to_string()),
            link_id: None,
            local_project_id: Some("project-1".to_string()),
            operation: ProviderTicketOperationKind::SetLabels,
            client_operation_id: "client-op-1".to_string(),
            status: ProviderTicketOperationStatus::Succeeded,
            provider_operation_id: None,
            error_message: None,
            metadata_json: None,
            last_attempt_at: Some(now),
            completed_at: Some(now),
            created_at: now,
            updated_at: now,
        },
        idempotent: false,
        transition: None,
        assignee: None,
        comment: None,
        labels: Some(TicketingLabelResult {
            labels: vec!["bug".to_string(), "frontend".to_string()],
        }),
    });

    assert_eq!(response.operation.operation, "set_labels");
    let labels = response.labels.expect("labels payload should map through");
    assert_eq!(
        labels.labels,
        vec!["bug".to_string(), "frontend".to_string()]
    );
}

#[test]
fn ticket_summary_filters_match_text_status_assignee_and_labels() {
    let items = vec![
        ticket_summary_fixture(
            "LIN-1",
            "Fix filter updates",
            "In Progress",
            Some("Reef Agent"),
            &["backend", "linear"],
        ),
        ticket_summary_fixture("LIN-2", "Polish ticket cards", "Done", None, &["frontend"]),
    ];

    let filtered = filter_ticket_summaries(
        items,
        Some(&TicketFiltersInput {
            text: Some("filter".to_string()),
            assignee: Some("reef".to_string()),
            state_ids: Some(vec!["in_progress".to_string()]),
            labels: Some(vec!["linear".to_string()]),
        }),
    );

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].ref_.key.as_deref(), Some("LIN-1"));
}

#[test]
fn ticket_summary_filters_remove_rows_without_requested_metadata() {
    let items = vec![ticket_summary_fixture(
        "LIN-2",
        "Polish ticket cards",
        "Done",
        None,
        &[],
    )];

    let filtered = filter_ticket_summaries(
        items,
        Some(&TicketFiltersInput {
            text: None,
            assignee: Some("me".to_string()),
            state_ids: None,
            labels: Some(vec!["backend".to_string()]),
        }),
    );

    assert!(filtered.is_empty());
}

#[test]
fn linear_detail_maps_provider_comments() {
    let detail = linear_content_to_detail(LinearIssueContent {
        id: "issue-1".to_string(),
        key: Some("LIN-1".to_string()),
        title: "Commented issue".to_string(),
        url: None,
        body: "Issue body".to_string(),
        state_name: Some("In Progress".to_string()),
        assignee: None,
        creator: None,
        updated_at: None,
        comments: vec![LinearComment {
            id: "comment-1".to_string(),
            body: "Provider **comment**".to_string(),
            author_id: Some("user-1".to_string()),
            author_name: Some("Reviewer".to_string()),
            created_at: Some("2026-06-21T08:00:00Z".to_string()),
            updated_at: None,
        }],
        attachments: vec![LinearAttachment {
            id: "attachment-1".to_string(),
            title: "Design mock".to_string(),
            subtitle: Some("Figma".to_string()),
            url: "https://uploads.linear.app/design.png".to_string(),
        }],
        labels: vec!["backend".to_string()],
        project: Some("Platform".to_string()),
    });

    assert_eq!(detail.comments.len(), 1);
    assert_eq!(detail.comments[0].body_markdown, "Provider **comment**");
    assert_eq!(
        detail.comments[0]
            .author
            .as_ref()
            .map(|author| author.name.as_str()),
        Some("Reviewer")
    );
    assert_eq!(detail.summary.labels, vec!["backend".to_string()]);
    assert_eq!(detail.summary.project.as_deref(), Some("Platform"));
    assert_eq!(detail.attachments.len(), 1);
    assert_eq!(detail.attachments[0].filename, "Design mock");
    assert_eq!(
        detail.attachments[0].url.as_deref(),
        Some("https://uploads.linear.app/design.png")
    );
}

#[test]
fn jira_summary_maps_with_empty_metadata_and_provider_state() {
    let summary = jira_summary_to_ticket(AtlassianResourceSummary {
        kind: AtlassianResourceKind::Jira,
        id: "10001".to_string(),
        key: Some("JRA-1".to_string()),
        title: "Investigate flaky merge".to_string(),
        url: Some("https://jira.test/browse/JRA-1".to_string()),
        excerpt: Some("excerpt is ignored by the summary mapper".to_string()),
    });

    assert_eq!(summary.ref_.provider, "jira");
    assert_eq!(summary.ref_.id, "10001");
    assert_eq!(summary.ref_.key.as_deref(), Some("JRA-1"));
    assert_eq!(summary.title, "Investigate flaky merge");
    assert_eq!(
        summary.url.as_deref(),
        Some("https://jira.test/browse/JRA-1")
    );
    // Jira search summaries carry no assignee/reporter/labels/project metadata.
    assert!(summary.assignee.is_none());
    assert!(summary.reporter.is_none());
    assert!(summary.labels.is_empty());
    assert!(summary.project.is_none());
    assert!(summary.priority.is_none());
    assert_eq!(summary.association_count, 0);
    // Jira summaries fall back to a synthetic provider-result state.
    assert_eq!(summary.state.name, "Provider result");
    assert_eq!(summary.state.category, "other");
    assert!(!summary.updated_at.is_empty());
}

#[test]
fn jira_status_category_rank_orders_todo_progress_done_then_other() {
    // Jira has no status order field; category is the only stable signal, so
    // columns must read To Do → In Progress → Done, with unknowns last.
    assert!(jira_status_category_rank("todo") < jira_status_category_rank("in_progress"));
    assert!(jira_status_category_rank("in_progress") < jira_status_category_rank("done"));
    assert!(jira_status_category_rank("done") < jira_status_category_rank("other"));
    assert_eq!(
        jira_status_category_rank("anything-unknown"),
        jira_status_category_rank("other")
    );
}

#[test]
fn jira_summary_without_key_keeps_none_key() {
    let summary = jira_summary_to_ticket(AtlassianResourceSummary {
        kind: AtlassianResourceKind::Jira,
        id: "10002".to_string(),
        key: None,
        title: "No key issue".to_string(),
        url: None,
        excerpt: None,
    });

    assert!(summary.ref_.key.is_none());
    assert!(summary.url.is_none());
}

#[test]
fn linear_summary_prefers_provider_state_fields_when_present() {
    let summary = linear_summary_to_ticket(LinearIssueSummary {
        id: "issue-1".to_string(),
        key: Some("LIN-1".to_string()),
        title: "Wire dashboard sync".to_string(),
        url: Some("https://linear.app/issue/LIN-1".to_string()),
        excerpt: None,
        state_id: Some("state-123".to_string()),
        state_name: Some("In Progress".to_string()),
        state_category: Some("started".to_string()),
        state_color: Some("#abcdef".to_string()),
        assignee: Some("Reef Agent".to_string()),
        updated_at: Some("2026-06-20T12:00:00Z".to_string()),
        labels: vec!["frontend".to_string(), "linear".to_string()],
        project: Some("Platform".to_string()),
    });

    assert_eq!(summary.ref_.provider, "linear");
    // Provider-supplied state id/category/color win over derivation.
    assert_eq!(summary.state.id, "state-123");
    assert_eq!(summary.state.name, "In Progress");
    assert_eq!(summary.state.category, "started");
    assert_eq!(summary.state.color.as_deref(), Some("#abcdef"));
    assert_eq!(
        summary.assignee.as_ref().map(|person| person.name.as_str()),
        Some("Reef Agent")
    );
    assert_eq!(
        summary.labels,
        vec!["frontend".to_string(), "linear".to_string()]
    );
    assert_eq!(summary.project.as_deref(), Some("Platform"));
    assert_eq!(summary.updated_at, "2026-06-20T12:00:00Z");
}

#[test]
fn linear_summary_derives_state_when_provider_fields_missing() {
    let summary = linear_summary_to_ticket(LinearIssueSummary {
        id: "issue-2".to_string(),
        key: None,
        title: "Untriaged issue".to_string(),
        url: None,
        excerpt: None,
        state_id: None,
        state_name: Some("Done".to_string()),
        state_category: None,
        state_color: None,
        assignee: None,
        updated_at: None,
        labels: Vec::new(),
        project: None,
    });

    // With only a state name, the id and category are derived from it.
    assert_eq!(summary.state.name, "Done");
    assert_eq!(summary.state.id, "done");
    assert_eq!(summary.state.category, "done");
    assert!(summary.assignee.is_none());
    assert!(summary.labels.is_empty());
    // Missing updated_at falls back to a generated timestamp.
    assert!(!summary.updated_at.is_empty());
}

#[test]
fn linear_summary_falls_back_to_provider_result_state_name() {
    let summary = linear_summary_to_ticket(LinearIssueSummary {
        id: "issue-3".to_string(),
        key: None,
        title: "No state issue".to_string(),
        url: None,
        excerpt: None,
        state_id: None,
        state_name: None,
        state_category: None,
        state_color: None,
        assignee: None,
        updated_at: None,
        labels: Vec::new(),
        project: None,
    });

    assert_eq!(summary.state.name, "Provider result");
    assert_eq!(summary.state.id, "provider_result");
    assert_eq!(summary.state.category, "other");
}

#[test]
fn jira_content_maps_description_comments_and_attachments() {
    let detail = jira_content_to_detail(AtlassianResourceContent {
        kind: AtlassianResourceKind::Jira,
        id: "10001".to_string(),
        key: Some("JRA-1".to_string()),
        title: "Detailed issue".to_string(),
        url: Some("https://jira.test/browse/JRA-1".to_string()),
        body: "Body fallback".to_string(),
        status: Some("In Review".to_string()),
        assignee: Some("Assignee Name".to_string()),
        reporter: Some("Reporter Name".to_string()),
        updated_at_remote: Some("2026-06-20T09:00:00Z".to_string()),
        description_markdown: None,
        description_text: Some("Plain description".to_string()),
        acceptance_criteria_markdown: Some("- criterion".to_string()),
        acceptance_criteria_text: None,
        comments: vec![AtlassianJiraComment {
            id: Some("comment-1".to_string()),
            author: Some("Commenter".to_string()),
            body_markdown: "Comment **md**".to_string(),
            body_text: "Comment md".to_string(),
            created_at: Some("2026-06-20T09:30:00Z".to_string()),
            updated_at: None,
        }],
        attachments: vec![AtlassianJiraAttachment {
            id: Some("attachment-1".to_string()),
            filename: "diagram.png".to_string(),
            mime_type: Some("image/png".to_string()),
            size: Some(2048),
            author: Some("Uploader".to_string()),
            content_url: Some("https://jira.test/attachment/1".to_string()),
            thumbnail_url: Some("https://jira.test/thumb/1".to_string()),
            created_at: Some("2026-06-20T09:45:00Z".to_string()),
        }],
    });

    // status maps into the ticket state name.
    assert_eq!(detail.summary.state.name, "In Review");
    assert_eq!(detail.summary.state.category, "in_progress");
    assert_eq!(
        detail.summary.assignee.as_ref().map(|p| p.name.as_str()),
        Some("Assignee Name")
    );
    assert_eq!(
        detail.summary.reporter.as_ref().map(|p| p.name.as_str()),
        Some("Reporter Name")
    );
    assert_eq!(detail.summary.updated_at, "2026-06-20T09:00:00Z");
    // No description_markdown means it falls back to the body.
    assert_eq!(
        detail.description_markdown.as_deref(),
        Some("Body fallback")
    );
    assert_eq!(
        detail.description_text.as_deref(),
        Some("Plain description")
    );
    assert_eq!(
        detail.acceptance_criteria_markdown.as_deref(),
        Some("- criterion")
    );
    assert_eq!(detail.comments.len(), 1);
    assert_eq!(detail.comments[0].id.as_deref(), Some("comment-1"));
    assert_eq!(detail.comments[0].body_markdown, "Comment **md**");
    assert_eq!(
        detail.comments[0].author.as_ref().map(|p| p.name.as_str()),
        Some("Commenter")
    );
    assert_eq!(detail.attachments.len(), 1);
    assert_eq!(detail.attachments[0].filename, "diagram.png");
    assert_eq!(
        detail.attachments[0].mime_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(detail.attachments[0].size, Some(2048));
    assert_eq!(
        detail.attachments[0].url.as_deref(),
        Some("https://jira.test/attachment/1")
    );
    // Jira detail mapper never carries transitions inline.
    assert!(detail.transitions.is_empty());
    assert!(detail.fetched_at.is_some());
}

#[test]
fn jira_content_prefers_description_markdown_over_body() {
    let detail = jira_content_to_detail(AtlassianResourceContent {
        kind: AtlassianResourceKind::Jira,
        id: "10003".to_string(),
        key: None,
        title: "Markdown issue".to_string(),
        url: None,
        body: "Body fallback".to_string(),
        status: None,
        assignee: None,
        reporter: None,
        updated_at_remote: None,
        description_markdown: Some("# Real markdown".to_string()),
        description_text: None,
        acceptance_criteria_markdown: None,
        acceptance_criteria_text: None,
        comments: Vec::new(),
        attachments: Vec::new(),
    });

    assert_eq!(
        detail.description_markdown.as_deref(),
        Some("# Real markdown")
    );
    // status=None falls back to the synthetic provider-result state.
    assert_eq!(detail.summary.state.name, "Provider result");
    assert!(detail.summary.assignee.is_none());
    assert!(detail.comments.is_empty());
    assert!(detail.attachments.is_empty());
    assert!(!detail.summary.updated_at.is_empty());
}

#[test]
fn linear_content_uses_body_for_description_and_creator_for_reporter() {
    let detail = linear_content_to_detail(LinearIssueContent {
        id: "issue-7".to_string(),
        key: Some("LIN-7".to_string()),
        title: "Body issue".to_string(),
        url: Some("https://linear.app/issue/LIN-7".to_string()),
        body: "Linear body text".to_string(),
        state_name: Some("Todo".to_string()),
        assignee: Some("Owner".to_string()),
        creator: Some("Creator".to_string()),
        updated_at: Some("2026-06-20T10:00:00Z".to_string()),
        comments: Vec::new(),
        attachments: Vec::new(),
        labels: vec!["urgent".to_string()],
        project: Some("Roadmap".to_string()),
    });

    assert_eq!(
        detail.description_markdown.as_deref(),
        Some("Linear body text")
    );
    assert_eq!(detail.description_text.as_deref(), Some("Linear body text"));
    assert!(detail.acceptance_criteria_markdown.is_none());
    assert_eq!(
        detail.summary.assignee.as_ref().map(|p| p.name.as_str()),
        Some("Owner")
    );
    assert_eq!(
        detail.summary.reporter.as_ref().map(|p| p.name.as_str()),
        Some("Creator")
    );
    assert_eq!(detail.summary.state.name, "Todo");
    assert_eq!(detail.summary.state.category, "todo");
    assert_eq!(detail.summary.updated_at, "2026-06-20T10:00:00Z");
    assert!(detail.attachments.is_empty());
    assert!(detail.comments.is_empty());
}

#[test]
fn state_category_classifies_known_state_keywords() {
    assert_eq!(state_category("Done"), "done");
    assert_eq!(state_category("Completed"), "done");
    assert_eq!(state_category("In Progress"), "in_progress");
    assert_eq!(state_category("Started review"), "in_progress");
    assert_eq!(state_category("To Do"), "todo");
    assert_eq!(state_category("Backlog"), "todo");
    assert_eq!(state_category("Something else"), "other");
}

#[test]
fn state_id_normalizes_into_kebab_snake_form() {
    assert_eq!(state_id("In Progress"), "in_progress");
    assert_eq!(state_id("To Do"), "to_do");
    assert_eq!(state_id("Done"), "done");
}

#[test]
fn ticket_state_combines_id_name_and_category() {
    let state = ticket_state("In Progress");
    assert_eq!(state.id, "in_progress");
    assert_eq!(state.name, "In Progress");
    assert_eq!(state.category, "in_progress");
    assert!(state.color.is_none());
}

#[test]
fn linear_workflow_state_maps_into_column_with_order() {
    let column = linear_workflow_state_to_column(
        LinearWorkflowState {
            id: "state-1".to_string(),
            name: "In Progress".to_string(),
            category: "started".to_string(),
            color: Some("#112233".to_string()),
        },
        2,
    );

    assert_eq!(column.id, "state-1");
    assert_eq!(column.name, "In Progress");
    assert_eq!(column.category, "started");
    assert_eq!(column.order, 2);
    assert_eq!(column.color.as_deref(), Some("#112233"));
}

#[test]
fn jira_project_maps_to_container_keyed_by_project_key() {
    let container = jira_project_to_container(JiraProjectSummary {
        id: "10000".to_string(),
        key: "RX".to_string(),
        name: "RalphX".to_string(),
    });

    assert_eq!(container.provider, "jira");
    // Container id is the project KEY (statuses + JQL both key on the key).
    assert_eq!(container.id, "RX");
    assert_eq!(container.key.as_deref(), Some("RX"));
    assert_eq!(container.name, "RalphX");
    assert_eq!(container.kind, "project");
    assert!(container.parent_id.is_none());
}

#[test]
fn jira_status_maps_into_column_preserving_real_id_and_category() {
    let column = jira_status_to_column(
        JiraStatusSummary {
            id: "3".to_string(),
            name: "In Progress".to_string(),
            category: "in_progress".to_string(),
        },
        1,
    );

    assert_eq!(column.id, "3");
    assert_eq!(column.name, "In Progress");
    assert_eq!(column.category, "in_progress");
    assert_eq!(column.order, 1);
}

#[test]
fn jira_issue_detail_maps_with_full_metadata() {
    let ticket = jira_issue_detail_to_ticket(JiraIssueDetail {
        key: "RX-1".to_string(),
        title: "Fix merge race".to_string(),
        status_id: Some("3".to_string()),
        status_name: Some("In Progress".to_string()),
        status_category: Some("in_progress".to_string()),
        assignee_name: Some("A. Dev".to_string()),
        assignee_avatar: Some("https://avatar/48".to_string()),
        labels: vec!["backend".to_string(), "urgent".to_string()],
        updated: Some("2026-06-20T10:00:00.000+0000".to_string()),
        priority: Some("High".to_string()),
        url: Some("https://example.atlassian.net/browse/RX-1".to_string()),
    });

    assert_eq!(ticket.ref_.provider, "jira");
    assert_eq!(ticket.ref_.id, "RX-1");
    assert_eq!(ticket.ref_.key.as_deref(), Some("RX-1"));
    assert_eq!(ticket.title, "Fix merge race");
    assert_eq!(ticket.state.id, "3");
    assert_eq!(ticket.state.name, "In Progress");
    assert_eq!(ticket.state.category, "in_progress");
    let assignee = ticket.assignee.expect("assignee present");
    assert_eq!(assignee.name, "A. Dev");
    assert_eq!(assignee.avatar_url.as_deref(), Some("https://avatar/48"));
    assert_eq!(ticket.labels, vec!["backend", "urgent"]);
    assert_eq!(ticket.priority.as_deref(), Some("High"));
    assert_eq!(ticket.updated_at, "2026-06-20T10:00:00.000+0000");
    assert_eq!(
        ticket.url.as_deref(),
        Some("https://example.atlassian.net/browse/RX-1")
    );
}

#[test]
fn jira_issue_detail_derives_state_when_status_missing() {
    // No status fields → derive id/category from the fallback name.
    let ticket = jira_issue_detail_to_ticket(JiraIssueDetail {
        key: "RX-2".to_string(),
        title: "No status".to_string(),
        status_id: None,
        status_name: None,
        status_category: None,
        assignee_name: None,
        assignee_avatar: None,
        labels: Vec::new(),
        updated: None,
        priority: None,
        url: None,
    });

    assert_eq!(ticket.state.name, "Provider result");
    assert_eq!(ticket.state.category, state_category("Provider result"));
    assert_eq!(ticket.state.id, state_id("Provider result"));
    assert!(ticket.assignee.is_none());
    assert!(ticket.labels.is_empty());
    assert!(ticket.priority.is_none());
    // Falls back to a fresh timestamp rather than panicking on a missing value.
    assert!(!ticket.updated_at.is_empty());
}

#[test]
fn container_selected_key_normalizes_presence_and_absence() {
    // container_id present → selected project key drives the project-scoped path.
    assert_eq!(container_selected_key(Some("RX")), Some("RX"));
    assert_eq!(container_selected_key(Some("  RX  ")), Some("RX"));
    // Absent / blank → None drives the global text-search fallback path.
    assert_eq!(container_selected_key(None), None);
    assert_eq!(container_selected_key(Some("   ")), None);
    assert_eq!(container_selected_key(Some("")), None);
}

#[test]
fn filter_ticket_summaries_returns_all_when_no_filters() {
    let items = vec![
        ticket_summary_fixture("LIN-1", "First", "Todo", None, &[]),
        ticket_summary_fixture("LIN-2", "Second", "Done", None, &[]),
    ];

    let filtered = filter_ticket_summaries(items, None);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn ticket_page_from_loaded_summaries_applies_filters_offset_and_next_cursor() {
    let items = vec![
        ticket_summary_fixture("LIN-1", "First", "Todo", None, &["backend"]),
        ticket_summary_fixture("LIN-2", "Second", "Done", None, &["frontend"]),
        ticket_summary_fixture("LIN-3", "Third", "Todo", None, &["backend"]),
    ];
    let filters = TicketFiltersInput {
        text: None,
        assignee: None,
        state_ids: None,
        labels: Some(vec!["backend".to_string()]),
    };

    let (page_items, next_cursor, total_loaded) =
        ticket_page_from_loaded_summaries(items, Some(&filters), 0, 1);

    assert_eq!(total_loaded, 2);
    assert_eq!(page_items.len(), 1);
    assert_eq!(page_items[0].ref_.key.as_deref(), Some("LIN-1"));
    assert_eq!(next_cursor.as_deref(), Some("offset:1"));
}

#[test]
fn ticket_page_from_loaded_summaries_omits_next_cursor_at_end() {
    let items = vec![
        ticket_summary_fixture("LIN-1", "First", "Todo", None, &[]),
        ticket_summary_fixture("LIN-2", "Second", "Done", None, &[]),
    ];

    let (page_items, next_cursor, total_loaded) =
        ticket_page_from_loaded_summaries(items, None, 1, 40);

    assert_eq!(total_loaded, 2);
    assert_eq!(page_items.len(), 1);
    assert_eq!(page_items[0].ref_.key.as_deref(), Some("LIN-2"));
    assert!(next_cursor.is_none());
}

#[test]
fn ticket_offset_cursor_round_trips_and_rejects_invalid_values() {
    let encoded = encode_ticket_offset_cursor(42);

    assert_eq!(encoded, "offset:42");
    assert_eq!(decode_ticket_offset_cursor(Some(&encoded)).unwrap(), 42);
    assert_eq!(decode_ticket_offset_cursor(Some("   ")).unwrap(), 0);
    assert_eq!(
        decode_ticket_offset_cursor(Some("cursor:42")).unwrap_err(),
        "Unsupported ticket cursor"
    );
    assert_eq!(
        decode_ticket_offset_cursor(Some("offset:not-a-number")).unwrap_err(),
        "Invalid ticket cursor"
    );
}

#[test]
fn ticket_filter_options_collects_assignees_and_clickup_current_user_sprints() {
    let mut current_sprint =
        ticket_summary_fixture("CU-1", "Current sprint", "Todo", Some("Zed"), &["backend"]);
    current_sprint.ref_.provider = "clickup".to_string();
    current_sprint.assignees = vec![named_person("Ada"), named_person("Zed")];
    current_sprint.project = Some("Sprint 42".to_string());
    current_sprint.current_user_assigned = true;

    let mut other_assignee =
        ticket_summary_fixture("CU-2", "Backlog", "Todo", Some("Grace"), &["backend"]);
    other_assignee.ref_.provider = "clickup".to_string();
    other_assignee.project = Some("Backlog".to_string());
    other_assignee.current_user_assigned = false;

    let mut filtered_out =
        ticket_summary_fixture("CU-3", "Unrelated", "Todo", Some("Hidden"), &["frontend"]);
    filtered_out.ref_.provider = "clickup".to_string();
    filtered_out.project = Some("Sprint Hidden".to_string());
    filtered_out.current_user_assigned = true;

    let filters = TicketFiltersInput {
        text: None,
        assignee: None,
        state_ids: None,
        labels: Some(vec!["backend".to_string()]),
    };

    let options = ticket_filter_options_from_loaded_summaries(
        PROVIDER_CLICKUP,
        vec![current_sprint, other_assignee, filtered_out],
        Some(&filters),
        10,
        false,
    );

    assert_eq!(options.assignees, vec!["Ada", "Grace", "Zed"]);
    assert_eq!(options.sprints, vec!["Sprint 42"]);
    assert!(options.complete);
    assert!(!options.truncated);
}

#[test]
fn ticket_filter_options_marks_truncation_from_provider_or_limit() {
    let items = vec![
        ticket_summary_fixture("LIN-1", "First", "Todo", Some("A"), &[]),
        ticket_summary_fixture("LIN-2", "Second", "Todo", Some("B"), &[]),
    ];

    let options =
        ticket_filter_options_from_loaded_summaries(PROVIDER_LINEAR, items, None, 1, true);

    assert_eq!(options.assignees, vec!["A"]);
    assert!(options.sprints.is_empty());
    assert!(!options.complete);
    assert!(options.truncated);
}

#[tokio::test]
async fn load_ticket_summaries_routes_jira_project_scope_before_disabled_error() {
    let state = AppState::new_test();

    let error = load_ticket_summaries(&state, PROVIDER_JIRA, Some("RX"), "ignored", 10)
        .await
        .expect_err("disabled Jira integration should fail");

    assert!(!error.trim().is_empty());
}

#[tokio::test]
async fn load_ticket_summaries_routes_jira_global_search_before_disabled_error() {
    let state = AppState::new_test();

    let error = load_ticket_summaries(&state, PROVIDER_JIRA, None, "merge", 10)
        .await
        .expect_err("disabled Jira integration should fail");

    assert!(!error.trim().is_empty());
}

#[tokio::test]
async fn load_ticket_summaries_routes_linear_before_disabled_error() {
    let state = AppState::new_test();

    let error = load_ticket_summaries(&state, PROVIDER_LINEAR, None, "merge", 10)
        .await
        .expect_err("disabled Linear integration should fail");

    assert!(!error.trim().is_empty());
}

#[tokio::test]
async fn load_ticket_summaries_routes_clickup_space_scope_before_disabled_error() {
    let state = AppState::new_test();

    let error = load_ticket_summaries(&state, PROVIDER_CLICKUP, Some("space-1"), "merge", 10)
        .await
        .expect_err("disabled ClickUp integration should fail");

    assert!(!error.trim().is_empty());
}

#[test]
fn ticket_matches_filters_with_empty_filter_input_keeps_all() {
    let ticket = ticket_summary_fixture("LIN-1", "First", "Todo", None, &[]);
    let empty = TicketFiltersInput {
        text: None,
        assignee: None,
        state_ids: None,
        labels: None,
    };
    assert!(ticket_matches_filters(&ticket, &empty));
}

#[test]
fn ticket_matches_filters_text_matches_provider_id_case_insensitively() {
    let ticket = ticket_summary_fixture("LIN-99", "Some title", "Todo", None, &[]);
    let by_id = TicketFiltersInput {
        text: Some("lin-99".to_string()),
        assignee: None,
        state_ids: None,
        labels: None,
    };
    assert!(ticket_matches_filters(&ticket, &by_id));

    let by_title = TicketFiltersInput {
        text: Some("SOME".to_string()),
        assignee: None,
        state_ids: None,
        labels: None,
    };
    assert!(ticket_matches_filters(&ticket, &by_title));

    let miss = TicketFiltersInput {
        text: Some("absent".to_string()),
        assignee: None,
        state_ids: None,
        labels: None,
    };
    assert!(!ticket_matches_filters(&ticket, &miss));
}

#[test]
fn ticket_matches_filters_state_id_matches_category_alias() {
    // ticket_summary_fixture sets state via ticket_state(state_name).
    let ticket = ticket_summary_fixture("LIN-1", "Title", "In Progress", None, &[]);
    let by_category = TicketFiltersInput {
        text: None,
        assignee: None,
        state_ids: Some(vec!["in_progress".to_string()]),
        labels: None,
    };
    assert!(ticket_matches_filters(&ticket, &by_category));

    let by_state_id = TicketFiltersInput {
        text: None,
        assignee: None,
        state_ids: Some(vec!["in_progress".to_string(), "other".to_string()]),
        labels: None,
    };
    assert!(ticket_matches_filters(&ticket, &by_state_id));

    let miss = TicketFiltersInput {
        text: None,
        assignee: None,
        state_ids: Some(vec!["done".to_string()]),
        labels: None,
    };
    assert!(!ticket_matches_filters(&ticket, &miss));
}

#[test]
fn ticket_matches_filters_assignee_matches_any_assignee() {
    let mut ticket = ticket_summary_fixture("CU-1", "Multi-assignee task", "Todo", None, &[]);
    ticket.assignees = vec![named_person("Ada Lovelace"), named_person("Grace Hopper")];

    let by_second_assignee = TicketFiltersInput {
        text: None,
        assignee: Some("Grace".to_string()),
        state_ids: None,
        labels: None,
    };
    assert!(ticket_matches_filters(&ticket, &by_second_assignee));

    let miss = TicketFiltersInput {
        text: None,
        assignee: Some("Katherine".to_string()),
        state_ids: None,
        labels: None,
    };
    assert!(!ticket_matches_filters(&ticket, &miss));
}

#[test]
fn ticket_matches_filters_requires_all_labels_present() {
    let ticket = ticket_summary_fixture("LIN-1", "Title", "Todo", None, &["backend", "linear"]);
    let all_present = TicketFiltersInput {
        text: None,
        assignee: None,
        state_ids: None,
        labels: Some(vec!["Backend".to_string(), "LINEAR".to_string()]),
    };
    // Label matching is case-insensitive.
    assert!(ticket_matches_filters(&ticket, &all_present));

    let missing_one = TicketFiltersInput {
        text: None,
        assignee: None,
        state_ids: None,
        labels: Some(vec!["backend".to_string(), "frontend".to_string()]),
    };
    assert!(!ticket_matches_filters(&ticket, &missing_one));
}

fn ticket_summary_fixture(
    key: &str,
    title: &str,
    state_name: &str,
    assignee: Option<&str>,
    labels: &[&str],
) -> TicketSummaryResponse {
    let assignee = assignee.map(named_person);
    let assignees = assignee.iter().cloned().collect();
    TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: "linear".to_string(),
            id: key.to_ascii_lowercase(),
            key: Some(key.to_string()),
        },
        title: title.to_string(),
        state: ticket_state(state_name),
        assignee,
        assignees,
        reporter: None,
        labels: labels.iter().map(|label| label.to_string()).collect(),
        project: None,
        priority: None,
        updated_at: now_string(),
        url: None,
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
    }
}

fn build_ticketing_start_app(
    state: AppState,
    execution_state: Arc<ExecutionState>,
) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(execution_state)
        .manage(Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        ))))
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_ticket_start_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let origin = temp.path().join("origin.git");
    let repo = temp.path().join("repo");

    let output = Command::new("git")
        .args(["init", "--bare", origin.to_str().expect("origin path")])
        .output()
        .expect("git init bare should run");
    assert!(output.status.success());
    let output = Command::new("git")
        .args(["init", "-b", "main", repo.to_str().expect("repo path")])
        .output()
        .expect("git init should run");
    assert!(output.status.success());

    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "main\n").expect("write readme");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );

    (temp, repo)
}

async fn seed_ticketing_project(state: &AppState, id: &str) -> ProjectId {
    let project_id = ProjectId::from_string(id.to_string());
    let mut project = Project::new(
        format!("{id} project"),
        format!("/tmp/{id}-project-worktree"),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    project_id
}

async fn seed_ticketing_project_with_working_directory(
    state: &AppState,
    id: &str,
    working_directory: String,
) -> ProjectId {
    let project_id = ProjectId::from_string(id.to_string());
    let mut project = Project::new(format!("{id} project"), working_directory);
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    project_id
}

fn ticket_start_input(
    project_id: &ProjectId,
    ticket_ref: TicketRefInput,
) -> StartRalphxWorkFromTicketInput {
    StartRalphxWorkFromTicketInput {
        start: StartAgentConversationInput {
            project_id: project_id.as_str().to_string(),
            content: "Start work from the ticket".to_string(),
            conversation_id: None,
            provider_harness: None,
            model_override: None,
            logical_effort: None,
            mode: Some("chat".to_string()),
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
        },
        ticket_ref,
    }
}

#[tokio::test]
async fn start_work_from_ticket_queues_message_and_links_jira_after_successful_start() {
    // Seed harness availability so the start runtime check passes on sandboxed CI
    // runners that have no real agent CLI on PATH (the probe is otherwise ambient).
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-start-jira").await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_ticketing_start_app(state, Arc::clone(&execution_state));

    let response = start_ralphx_work_from_ticket(
        ticket_start_input(
            &project_id,
            TicketRefInput {
                provider: "jira".to_string(),
                id: "10001".to_string(),
                key: Some("RAL-42".to_string()),
            },
        ),
        app.state(),
        app.state(),
        app.state(),
        app.handle().clone(),
    )
    .await
    .expect("ticket start should succeed while paused by queuing the send");

    assert_eq!(response.conversation.context_id, project_id.as_str());
    assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
    assert!(response.workspace.is_none());
    assert!(response.send_result.was_queued);
    let queued = app
        .state::<AppState>()
        .message_queue
        .get_queued(ChatContextType::Project, response.conversation.id.as_str());
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].composer_integration_references.len(), 1);
    assert_eq!(
        queued[0].composer_integration_references[0].provider,
        "atlassian"
    );
    assert_eq!(queued[0].composer_integration_references[0].kind, "jira");
    assert_eq!(
        queued[0].composer_integration_references[0].key.as_deref(),
        Some("RAL-42")
    );

    let conversation_id = ChatConversationId::from_string(response.conversation.id.clone());
    let linked = app
        .state::<AppState>()
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("jira link lookup should succeed")
        .expect("jira issue should be linked after start succeeds");
    assert_eq!(linked.issue_key, "RAL-42");
    assert_eq!(linked.issue_id.as_deref(), Some("10001"));
    assert!(linked.manually_assigned);
}

#[tokio::test]
async fn start_work_from_ticket_queues_message_for_clickup_without_link_table() {
    // Seed harness availability so the start runtime check passes on sandboxed CI
    // runners that have no real agent CLI on PATH (the probe is otherwise ambient).
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-start-clickup").await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_ticketing_start_app(state, Arc::clone(&execution_state));

    let response = start_ralphx_work_from_ticket(
        ticket_start_input(
            &project_id,
            TicketRefInput {
                provider: "clickup".to_string(),
                id: "8689abc".to_string(),
                key: Some("CU-42".to_string()),
            },
        ),
        app.state(),
        app.state(),
        app.state(),
        app.handle().clone(),
    )
    .await
    .expect("clickup ticket start should succeed without a link table");

    assert_eq!(response.conversation.context_id, project_id.as_str());
    assert_eq!(response.conversation.title.as_deref(), Some("CU-42"));
    assert!(response.send_result.was_queued);
    let queued = app
        .state::<AppState>()
        .message_queue
        .get_queued(ChatContextType::Project, response.conversation.id.as_str());
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].composer_integration_references.len(), 1);
    assert_eq!(
        queued[0].composer_integration_references[0].provider,
        "clickup"
    );
    assert_eq!(queued[0].composer_integration_references[0].kind, "clickup");
    assert_eq!(
        queued[0].composer_integration_references[0].key.as_deref(),
        Some("CU-42")
    );
}

#[tokio::test]
async fn start_agent_conversation_with_ticket_default_base_uses_canonical_branch() {
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let (_temp, repo) = init_ticket_start_repo();
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let project_id = seed_ticketing_project_with_working_directory(
        &state,
        "ticket-start-service",
        repo.to_string_lossy().into_owned(),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_ticketing_start_app(state, Arc::clone(&execution_state));

    let result = AgentConversationStartService::new(AgentConversationStartDeps {
        state: app.state::<AppState>().inner(),
        execution_state: app.state::<Arc<ExecutionState>>().inner(),
        team_service: Some(app.state::<Arc<TeamService>>().inner().clone()),
        app_handle: app.handle().clone(),
    })
    .start(StartAgentConversationInput {
        project_id: project_id.as_str().to_string(),
        content: "Start from attached ticket".to_string(),
        conversation_id: None,
        provider_harness: None,
        model_override: None,
        logical_effort: None,
        mode: Some("edit".to_string()),
        base_ref_kind: Some("project_default".to_string()),
        base_ref: Some("main".to_string()),
        base_display_name: Some("Project default (main)".to_string()),
        base_source_pull_request: None,
        composer_project_references: Vec::new(),
        composer_integration_references: vec![ComposerIntegrationReference {
            provider: "atlassian".to_string(),
            kind: "jira".to_string(),
            id: "10077".to_string(),
            key: Some("RX-77".to_string()),
            title: Some("Ticket with default base".to_string()),
            url: None,
        }],
        composer_artifact_references: Vec::new(),
    })
    .await
    .expect("start should succeed by queueing while paused");

    let workspace = result.workspace.expect("edit mode creates a workspace");
    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::LocalBranch
    );
    assert_eq!(workspace.base_ref, "ralphx/ticket/jira-rx-77");
    assert_eq!(
        workspace.base_display_name.as_deref(),
        Some("Ticket RX-77 (ralphx/ticket/jira-rx-77)")
    );
    assert_eq!(github.state().push_branch_calls, 1);
    assert_eq!(
        github.state().last_push_branch_name.as_deref(),
        Some("ralphx/ticket/jira-rx-77")
    );
    assert!(result.send_result.was_queued);
}

#[tokio::test]
async fn start_work_from_ticket_does_not_link_when_existing_conversation_is_invalid() {
    // Seed harness availability so the start runtime check passes on sandboxed CI
    // runners that have no real agent CLI on PATH (the probe is otherwise ambient).
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-start-link-rollback").await;
    let other_project_id = seed_ticketing_project(&state, "ticket-start-link-rollback-other").await;
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(other_project_id))
        .await
        .expect("conversation should be created");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_ticketing_start_app(state, execution_state);
    let mut input = ticket_start_input(
        &project_id,
        TicketRefInput {
            provider: "jira".to_string(),
            id: "10002".to_string(),
            key: Some("RAL-43".to_string()),
        },
    );
    input.start.conversation_id = Some(conversation.id.as_str().to_string());

    let error = start_ralphx_work_from_ticket(
        input,
        app.state(),
        app.state(),
        app.state(),
        app.handle().clone(),
    )
    .await
    .expect_err("start should fail before ticket link upsert");

    assert!(error.contains("does not belong to project"));
    let linked = app
        .state::<AppState>()
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("jira link lookup should succeed");
    assert!(linked.is_none());
}

#[tokio::test]
async fn get_ticket_associations_returns_linked_agent_conversations() {
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-associations-jira").await;
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.set_title("Started from RX-77");
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be created");
    let ticket_ref = TicketRefInput {
        provider: "jira".to_string(),
        id: "10077".to_string(),
        key: Some("RX-77".to_string()),
    };
    let ticket_reference = ticket_ref_to_composer_reference("jira", &ticket_ref);
    link_started_ticket_to_conversation(
        &state,
        "jira",
        &conversation.id,
        &project_id,
        &ticket_reference,
    )
    .await
    .expect("ticket link should be persisted");
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let associations = get_ticket_associations(
        "jira".to_string(),
        ticket_ref,
        project_id.as_str().to_string(),
        app.state(),
    )
    .await
    .expect("ticket associations should load");

    assert_eq!(associations.conversations.len(), 1);
    let linked = &associations.conversations[0];
    assert_eq!(linked.id, conversation.id.as_str());
    assert_eq!(linked.title, "Started from RX-77");
    assert_eq!(linked.status.as_deref(), Some("edit"));
    assert!(linked.active);
    assert_eq!(linked.deep_link.view, "agents");
    assert_eq!(linked.deep_link.id, conversation.id.as_str());
    // The deep link carries the project so the agents view can select the exact
    // conversation rather than only switching views.
    assert_eq!(
        linked.deep_link.project_id.as_deref(),
        Some(project_id.as_str())
    );
}

#[tokio::test]
async fn list_ticket_rows_include_linked_agent_conversation_count() {
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-list-counts-jira").await;
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("conversation should be created");
    let ticket_ref = TicketRefInput {
        provider: "jira".to_string(),
        id: "10077".to_string(),
        key: Some("RX-77".to_string()),
    };
    let ticket_reference = ticket_ref_to_composer_reference("jira", &ticket_ref);
    link_started_ticket_to_conversation(
        &state,
        "jira",
        &conversation.id,
        &project_id,
        &ticket_reference,
    )
    .await
    .expect("ticket link should be persisted");

    let hydrated = hydrate_ticket_association_counts(
        &state,
        "jira",
        Some(project_id.as_str()),
        vec![hydrate_input_summary(ticket_ref)],
    )
    .await
    .expect("association counts should hydrate");

    assert_eq!(hydrated[0].association_count, 1);
}

/// Seed an Edit-mode workspace for a conversation, optionally carrying a PR
/// number/url/status and an explicit `updated_at` used to rank representative PRs.
async fn seed_workspace_with_pr(
    state: &AppState,
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
    pr_number: Option<i64>,
    pr_url: Option<&str>,
    pr_status: Option<&str>,
    updated_at: chrono::DateTime<chrono::Utc>,
) {
    let mut workspace = AgentConversationWorkspace::new(
        *conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Current branch (main)".to_string()),
        None,
        format!("agent/{conversation_id}"),
        format!("/tmp/worktrees/{conversation_id}"),
    );
    workspace.publication_pr_number = pr_number;
    workspace.publication_pr_url = pr_url.map(str::to_string);
    workspace.publication_pr_status = pr_status.map(str::to_string);
    workspace.updated_at = updated_at;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be created");
}

fn hydrate_input_summary(ticket_ref: TicketRefInput) -> TicketSummaryResponse {
    TicketSummaryResponse {
        ref_: ticket_ref,
        title: "Linked ticket".to_string(),
        state: ticket_state("To Do"),
        assignee: None,
        assignees: Vec::new(),
        reporter: None,
        labels: Vec::new(),
        project: None,
        priority: None,
        updated_at: now_string(),
        url: None,
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
    }
}

#[tokio::test]
async fn hydrate_populates_representative_closed_pr_when_no_open_pr_exists() {
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-closed-pr-jira").await;
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("conversation should be created");
    let ticket_ref = TicketRefInput {
        provider: "jira".to_string(),
        id: "10381".to_string(),
        key: Some("RX-381".to_string()),
    };
    let ticket_reference = ticket_ref_to_composer_reference("jira", &ticket_ref);
    link_started_ticket_to_conversation(
        &state,
        "jira",
        &conversation.id,
        &project_id,
        &ticket_reference,
    )
    .await
    .expect("ticket link should be persisted");
    // Only a CLOSED PR exists for this ticket — the representative PR must still
    // be populated, while open_pr_count stays 0.
    seed_workspace_with_pr(
        &state,
        &conversation.id,
        &project_id,
        Some(381),
        Some("https://github.com/x/y/pull/381"),
        Some("closed"),
        chrono::Utc::now(),
    )
    .await;

    let hydrated = hydrate_ticket_association_counts(
        &state,
        "jira",
        Some(project_id.as_str()),
        vec![hydrate_input_summary(ticket_ref)],
    )
    .await
    .expect("association counts should hydrate");

    assert_eq!(hydrated[0].association_count, 1);
    assert_eq!(hydrated[0].open_pr_number, Some(381));
    assert_eq!(
        hydrated[0].open_pr_url.as_deref(),
        Some("https://github.com/x/y/pull/381")
    );
    assert_eq!(hydrated[0].open_pr_status.as_deref(), Some("closed"));
    // open_pr_count counts only OPEN PRs, so a closed-only ticket stays at 0.
    assert_eq!(hydrated[0].open_pr_count, 0);
}

#[tokio::test]
async fn hydrate_prefers_open_pr_over_more_recent_closed_pr() {
    let state = AppState::new_test();
    let project_id = seed_ticketing_project(&state, "ticket-open-vs-closed-jira").await;
    let open_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("open conversation should be created");
    let closed_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("closed conversation should be created");
    let ticket_ref = TicketRefInput {
        provider: "jira".to_string(),
        id: "10500".to_string(),
        key: Some("RX-500".to_string()),
    };
    let ticket_reference = ticket_ref_to_composer_reference("jira", &ticket_ref);
    for conversation_id in [&open_conversation.id, &closed_conversation.id] {
        link_started_ticket_to_conversation(
            &state,
            "jira",
            conversation_id,
            &project_id,
            &ticket_reference,
        )
        .await
        .expect("ticket link should be persisted");
    }
    let now = chrono::Utc::now();
    // The closed PR is MORE recent, but an open PR must still win.
    seed_workspace_with_pr(
        &state,
        &open_conversation.id,
        &project_id,
        Some(100),
        Some("https://github.com/x/y/pull/100"),
        Some("open"),
        now - chrono::Duration::hours(2),
    )
    .await;
    seed_workspace_with_pr(
        &state,
        &closed_conversation.id,
        &project_id,
        Some(101),
        Some("https://github.com/x/y/pull/101"),
        Some("closed"),
        now,
    )
    .await;

    let hydrated = hydrate_ticket_association_counts(
        &state,
        "jira",
        Some(project_id.as_str()),
        vec![hydrate_input_summary(ticket_ref)],
    )
    .await
    .expect("association counts should hydrate");

    assert_eq!(hydrated[0].open_pr_number, Some(100));
    assert_eq!(hydrated[0].open_pr_status.as_deref(), Some("open"));
    assert_eq!(hydrated[0].open_pr_count, 1);
}

#[test]
fn pull_request_items_map_pr_and_branch_only_workspaces() {
    let summaries = vec![
        TicketPrBranchSummary {
            conversation_id: "conv-1".to_string(),
            branch_name: "ralphx/p/agent-1".to_string(),
            base_ref: "main".to_string(),
            pr_number: Some(42),
            pr_url: Some("https://github.com/x/y/pull/42".to_string()),
            pr_status: Some("open".to_string()),
            is_open: true,
        },
        TicketPrBranchSummary {
            conversation_id: "conv-2".to_string(),
            branch_name: "ralphx/p/agent-2".to_string(),
            base_ref: "main".to_string(),
            pr_number: None,
            pr_url: None,
            pr_status: None,
            is_open: false,
        },
    ];

    let items = pull_request_association_items(&summaries, "project-1");

    assert_eq!(items.len(), 2);
    let pr = &items[0];
    assert_eq!(pr.title, "PR #42");
    assert_eq!(pr.subtitle.as_deref(), Some("ralphx/p/agent-1"));
    assert_eq!(pr.status.as_deref(), Some("open"));
    assert!(pr.active);
    assert_eq!(pr.id, "https://github.com/x/y/pull/42");
    assert_eq!(pr.deep_link.view, "agents");
    assert_eq!(pr.deep_link.id, "conv-1");
    assert_eq!(pr.deep_link.project_id.as_deref(), Some("project-1"));
    assert_eq!(pr.branch_name.as_deref(), Some("ralphx/p/agent-1"));
    assert_eq!(pr.base_ref.as_deref(), Some("main"));
    assert_eq!(pr.pr_number, Some(42));
    assert_eq!(pr.pr_url.as_deref(), Some("https://github.com/x/y/pull/42"));

    let branch_only = &items[1];
    assert_eq!(branch_only.title, "ralphx/p/agent-2");
    assert_eq!(branch_only.status.as_deref(), Some("branch"));
    assert!(!branch_only.active);
    assert_eq!(branch_only.id, "conv-2");
    assert_eq!(branch_only.branch_name.as_deref(), Some("ralphx/p/agent-2"));
    assert_eq!(branch_only.base_ref.as_deref(), Some("main"));
    assert_eq!(branch_only.pr_number, None);
    assert_eq!(branch_only.pr_url, None);
}

fn base_test_start_input() -> StartAgentConversationInput {
    StartAgentConversationInput {
        project_id: "project-1".to_string(),
        content: "Start work".to_string(),
        conversation_id: None,
        provider_harness: None,
        model_override: None,
        logical_effort: None,
        mode: Some("edit".to_string()),
        base_ref_kind: Some("project_default".to_string()),
        base_ref: Some("client-supplied-branch".to_string()),
        base_display_name: Some("Client base".to_string()),
        base_source_pull_request: None,
        composer_project_references: Vec::new(),
        composer_integration_references: Vec::new(),
        composer_artifact_references: Vec::new(),
    }
}

#[test]
fn ticket_ref_issue_key_prefers_key_then_falls_back_to_id() {
    let with_key = TicketRefInput {
        provider: "linear".to_string(),
        id: "abc-123".to_string(),
        key: Some("WISE-24".to_string()),
    };
    assert_eq!(ticket_ref_issue_key(&with_key), "WISE-24");

    let without_key = TicketRefInput {
        provider: "linear".to_string(),
        id: "abc-123".to_string(),
        key: None,
    };
    assert_eq!(ticket_ref_issue_key(&without_key), "abc-123");

    let blank_key = TicketRefInput {
        provider: "linear".to_string(),
        id: "abc-123".to_string(),
        key: Some("   ".to_string()),
    };
    assert_eq!(ticket_ref_issue_key(&blank_key), "abc-123");
}

#[test]
fn workspace_modes_inherit_canonical_branch_chat_does_not() {
    assert!(ticket_start_inherits_canonical_branch(Some("edit")));
    assert!(ticket_start_inherits_canonical_branch(Some("plan")));
    // Default (unset) is edit, which is workspace-creating.
    assert!(ticket_start_inherits_canonical_branch(None));
    // Chat-only ticket starts create no workspace, so they skip base injection.
    assert!(!ticket_start_inherits_canonical_branch(Some("chat")));
}

#[test]
fn ticket_start_applies_canonical_branch_only_for_default_base() {
    let start = base_test_start_input();
    assert!(ticket_start_should_apply_canonical_branch(&start));

    let mut local = base_test_start_input();
    local.base_ref_kind = Some("local_branch".to_string());
    local.base_ref = Some("feature/existing".to_string());
    assert!(!ticket_start_should_apply_canonical_branch(&local));

    let mut invalid = base_test_start_input();
    invalid.base_ref_kind = Some("not-a-base-kind".to_string());
    assert!(!ticket_start_should_apply_canonical_branch(&invalid));

    let mut pr = base_test_start_input();
    pr.base_ref_kind = Some("local_branch".to_string());
    pr.base_ref = Some("feature/pr-head".to_string());
    pr.base_source_pull_request =
        Some(crate::application::agent_conversation_start_service::AgentWorkspaceSourcePullRequestInput {
        number: 42,
        url: Some("https://github.com/x/y/pull/42".to_string()),
        title: Some("PR #42".to_string()),
        head_ref_name: "feature/pr-head".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: None,
    });
    assert!(!ticket_start_should_apply_canonical_branch(&pr));
}

#[test]
fn apply_canonical_base_overwrites_to_local_branch() {
    let mut start = base_test_start_input();

    apply_ticket_canonical_branch_base(&mut start, "WISE-24", "ralphx/ticket/linear-wise-24");

    assert_eq!(start.base_ref_kind.as_deref(), Some("local_branch"));
    assert_eq!(
        start.base_ref.as_deref(),
        Some("ralphx/ticket/linear-wise-24")
    );
    assert_eq!(
        start.base_display_name.as_deref(),
        Some("Ticket WISE-24 (ralphx/ticket/linear-wise-24)")
    );
}
