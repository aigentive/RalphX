use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::*;
use crate::application::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianCredential,
    AtlassianIntegrationService, AtlassianResourceContent, AtlassianResourceKind,
    AtlassianResourceSummary, ClickUpApiClient, ClickUpAuthContext, ClickUpComment,
    ClickUpIntegrationService, ClickUpStatus, ClickUpTaskContent, ClickUpUser, ClickUpWorkspace,
    EmptyClickUpApiClient, LinearApiClient, LinearAuthContext, LinearIntegrationService,
    LinearIssueContent, LinearIssueSummary, LinearLabel, LinearUser,
};
use crate::domain::integrations::{
    AtlassianAuthMethod, ExternalIssueLinkUpsert, ExternalIssueLocalObject,
    ProviderTicketOperationStatus,
};
use crate::domain::services::ComposerIntegrationReference;
use crate::infrastructure::memory::{
    MemoryAtlassianIntegrationSettingsRepository, MemoryClickUpIntegrationSettingsRepository,
    MemoryExternalIssueLinkRepository, MemoryLinearIntegrationSettingsRepository,
    MemorySecretStore,
};

#[derive(Default)]
struct RecordingEventSink {
    events: Mutex<Vec<TicketingOperationEvent>>,
}

impl RecordingEventSink {
    fn events(&self) -> Vec<TicketingOperationEvent> {
        self.events.lock().expect("events lock").clone()
    }
}

impl TicketingEventSink for RecordingEventSink {
    fn emit_ticketing_operation_event(&self, event: TicketingOperationEvent) {
        self.events.lock().expect("events lock").push(event);
    }
}

#[derive(Default)]
struct RecordingLinearClient {
    updates: tokio::sync::Mutex<Vec<(String, String)>>,
    assignments: tokio::sync::Mutex<Vec<String>>,
    assignment_clears: tokio::sync::Mutex<Vec<String>>,
    comments: tokio::sync::Mutex<Vec<(String, String)>>,
    label_updates: tokio::sync::Mutex<Vec<(String, Vec<String>)>>,
}

#[async_trait]
impl LinearApiClient for RecordingLinearClient {
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
            title: reference.id.clone(),
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

    async fn list_workflow_states(
        &self,
        _auth: &LinearAuthContext,
        _team_id: Option<&str>,
    ) -> Result<Vec<LinearWorkflowState>, String> {
        Ok(vec![
            LinearWorkflowState {
                id: "todo".to_string(),
                name: "Todo".to_string(),
                category: "todo".to_string(),
                color: None,
            },
            LinearWorkflowState {
                id: "done".to_string(),
                name: "Done".to_string(),
                category: "done".to_string(),
                color: None,
            },
        ])
    }

    async fn current_user(&self, _auth: &LinearAuthContext) -> Result<LinearUser, String> {
        Ok(LinearUser {
            id: "user-1".to_string(),
            name: Some("A. User".to_string()),
        })
    }

    async fn update_issue_state(
        &self,
        _auth: &LinearAuthContext,
        issue_id: &str,
        state_id: &str,
    ) -> Result<(), String> {
        self.updates
            .lock()
            .await
            .push((issue_id.to_string(), state_id.to_string()));
        Ok(())
    }

    async fn assign_issue_to_current_user(
        &self,
        _auth: &LinearAuthContext,
        issue_id: &str,
    ) -> Result<LinearUser, String> {
        self.assignments.lock().await.push(issue_id.to_string());
        Ok(LinearUser {
            id: "user-1".to_string(),
            name: Some("A. User".to_string()),
        })
    }

    async fn clear_issue_assignee(
        &self,
        _auth: &LinearAuthContext,
        issue_id: &str,
    ) -> Result<(), String> {
        self.assignment_clears
            .lock()
            .await
            .push(issue_id.to_string());
        Ok(())
    }

    async fn create_comment(
        &self,
        _auth: &LinearAuthContext,
        issue_id: &str,
        body_markdown: &str,
    ) -> Result<LinearComment, String> {
        self.comments
            .lock()
            .await
            .push((issue_id.to_string(), body_markdown.to_string()));
        Ok(LinearComment {
            id: "comment-1".to_string(),
            body: body_markdown.to_string(),
            author_id: Some("user-1".to_string()),
            author_name: Some("A. User".to_string()),
            created_at: Some("2026-06-20T08:00:00Z".to_string()),
            updated_at: Some("2026-06-20T08:00:00Z".to_string()),
        })
    }

    async fn list_issue_team_labels(
        &self,
        _auth: &LinearAuthContext,
        _issue_id: &str,
    ) -> Result<Vec<LinearLabel>, String> {
        Ok(vec![
            LinearLabel {
                id: "label-bug".to_string(),
                name: "Bug".to_string(),
            },
            LinearLabel {
                id: "label-feature".to_string(),
                name: "Feature".to_string(),
            },
        ])
    }

    async fn update_issue_labels(
        &self,
        _auth: &LinearAuthContext,
        issue_id: &str,
        label_ids: Vec<String>,
    ) -> Result<(), String> {
        self.label_updates
            .lock()
            .await
            .push((issue_id.to_string(), label_ids));
        Ok(())
    }
}

#[derive(Default)]
struct RecordingAtlassianClient {
    transitions: tokio::sync::Mutex<Vec<(String, String)>>,
    assignments: tokio::sync::Mutex<Vec<String>>,
    assignment_clears: tokio::sync::Mutex<Vec<String>>,
    comments: tokio::sync::Mutex<Vec<(String, String)>>,
    label_writes: tokio::sync::Mutex<Vec<(String, Vec<String>)>>,
}

#[async_trait]
impl AtlassianApiClient for RecordingAtlassianClient {
    async fn validate(&self, auth: &AtlassianAuthContext) -> Result<AtlassianConnectivity, String> {
        assert_eq!(auth.site_url, "https://jira.test");
        assert!(matches!(
            auth.credential,
            AtlassianCredential::ApiToken { .. }
        ));
        Ok(AtlassianConnectivity {
            jira_available: true,
            confluence_available: false,
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
            kind: AtlassianResourceKind::Jira,
            id: reference.id.clone(),
            key: reference.key.clone(),
            title: reference.id.clone(),
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
            issue_type: None,
            labels: Vec::new(),
            priority: None,
            parent_key: None,
            children: Vec::new(),
        })
    }

    async fn assign_jira_issue_to_current_user(
        &self,
        _auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<(), String> {
        self.assignments.lock().await.push(issue_key.to_string());
        Ok(())
    }

    async fn clear_jira_issue_assignee(
        &self,
        _auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<(), String> {
        self.assignment_clears
            .lock()
            .await
            .push(issue_key.to_string());
        Ok(())
    }

    async fn list_jira_issue_transitions(
        &self,
        _auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<Vec<AtlassianJiraTransition>, String> {
        assert_eq!(issue_key, "JRA-1");
        Ok(vec![AtlassianJiraTransition {
            provider_transition_id: "31".to_string(),
            to_state_id: "done".to_string(),
            name: "Done".to_string(),
            category: "done".to_string(),
        }])
    }

    async fn transition_jira_issue(
        &self,
        _auth: &AtlassianAuthContext,
        issue_key: &str,
        transition_id: &str,
    ) -> Result<(), String> {
        self.transitions
            .lock()
            .await
            .push((issue_key.to_string(), transition_id.to_string()));
        Ok(())
    }

    async fn add_jira_comment(
        &self,
        _auth: &AtlassianAuthContext,
        issue_key: &str,
        body_markdown: &str,
    ) -> Result<AtlassianJiraComment, String> {
        self.comments
            .lock()
            .await
            .push((issue_key.to_string(), body_markdown.to_string()));
        Ok(AtlassianJiraComment {
            id: Some("comment-1".to_string()),
            author: Some("A. User".to_string()),
            body_markdown: body_markdown.to_string(),
            body_text: body_markdown.to_string(),
            created_at: Some("2026-06-20T08:00:00Z".to_string()),
            updated_at: Some("2026-06-20T08:00:00Z".to_string()),
        })
    }

    async fn set_jira_issue_labels(
        &self,
        _auth: &AtlassianAuthContext,
        issue_key: &str,
        labels: Vec<String>,
    ) -> Result<(), String> {
        self.label_writes
            .lock()
            .await
            .push((issue_key.to_string(), labels));
        Ok(())
    }

    async fn exchange_oauth_code(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _code: &str,
        _redirect_uri: &str,
    ) -> Result<crate::application::AtlassianOAuthTokenResponse, String> {
        Err("not used".to_string())
    }

    async fn refresh_oauth_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _refresh_token: &str,
    ) -> Result<crate::application::AtlassianOAuthTokenResponse, String> {
        Err("not used".to_string())
    }

    async fn oauth_accessible_resources(
        &self,
        _access_token: &str,
    ) -> Result<Vec<crate::application::AtlassianOAuthResource>, String> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct RecordingClickUpClient {
    status_updates: tokio::sync::Mutex<Vec<(String, String)>>,
    assignments: tokio::sync::Mutex<Vec<String>>,
    assignment_clears: tokio::sync::Mutex<Vec<String>>,
    comments: tokio::sync::Mutex<Vec<(String, String)>>,
    tag_writes: tokio::sync::Mutex<Vec<(String, Vec<String>)>>,
}

#[async_trait]
impl ClickUpApiClient for RecordingClickUpClient {
    async fn validate(&self, _auth: &ClickUpAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_workspaces(
        &self,
        _auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        Ok(vec![ClickUpWorkspace {
            id: "team-1".to_string(),
            name: "Team One".to_string(),
            color: None,
        }])
    }

    async fn fetch_task(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        Ok(ClickUpTaskContent {
            id: task_id.to_string(),
            custom_id: None,
            name: task_id.to_string(),
            url: None,
            description: String::new(),
            status_name: Some("to do".to_string()),
            status_type: Some("open".to_string()),
            status_category: Some("todo".to_string()),
            creator: None,
            assignees: Vec::new(),
            watchers: Vec::new(),
            tags: Vec::new(),
            comments: Vec::new(),
            attachments: Vec::new(),
            updated_at: None,
            space_id: Some("space-1".to_string()),
            list_name: None,
        })
    }

    async fn list_statuses(
        &self,
        _auth: &ClickUpAuthContext,
        _space_id: &str,
    ) -> Result<Vec<ClickUpStatus>, String> {
        Ok(vec![
            ClickUpStatus {
                id: None,
                status: "to do".to_string(),
                status_type: "open".to_string(),
                category: "todo".to_string(),
                color: None,
                orderindex: Some(0),
            },
            ClickUpStatus {
                id: None,
                status: "complete".to_string(),
                status_type: "done".to_string(),
                category: "done".to_string(),
                color: None,
                orderindex: Some(1),
            },
        ])
    }

    async fn current_user(&self, _auth: &ClickUpAuthContext) -> Result<ClickUpUser, String> {
        Ok(ClickUpUser {
            id: 7,
            username: Some("A. User".to_string()),
            email: None,
        })
    }

    async fn update_task_status(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
        status_name: &str,
    ) -> Result<(), String> {
        self.status_updates
            .lock()
            .await
            .push((task_id.to_string(), status_name.to_string()));
        Ok(())
    }

    async fn assign_task_to_current_user(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpUser, String> {
        self.assignments.lock().await.push(task_id.to_string());
        Ok(ClickUpUser {
            id: 7,
            username: Some("A. User".to_string()),
            email: None,
        })
    }

    async fn clear_task_assignee(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<(), String> {
        self.assignment_clears
            .lock()
            .await
            .push(task_id.to_string());
        Ok(())
    }

    async fn create_comment(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
        body_markdown: &str,
    ) -> Result<ClickUpComment, String> {
        self.comments
            .lock()
            .await
            .push((task_id.to_string(), body_markdown.to_string()));
        Ok(ClickUpComment {
            id: "clk-comment-1".to_string(),
            body: body_markdown.to_string(),
            author_id: Some(7),
            author_name: Some("A. User".to_string()),
            created_at: Some("2026-06-20T08:00:00Z".to_string()),
            attachments: Vec::new(),
            replies: Vec::new(),
        })
    }

    async fn set_task_tags(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
        tags: Vec<String>,
    ) -> Result<(), String> {
        self.tag_writes
            .lock()
            .await
            .push((task_id.to_string(), tags));
        Ok(())
    }
}

async fn enabled_linear_service(
    client: Arc<RecordingLinearClient>,
) -> Arc<LinearIntegrationService> {
    let service = Arc::new(LinearIntegrationService::new(
        Arc::new(MemoryLinearIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ));
    service
        .save_settings(Some("lin-token".to_string()))
        .await
        .expect("linear settings should save");
    service
        .validate_and_enable()
        .await
        .expect("linear should validate");
    service
}

fn disabled_linear_service(client: Arc<RecordingLinearClient>) -> Arc<LinearIntegrationService> {
    Arc::new(LinearIntegrationService::new(
        Arc::new(MemoryLinearIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ))
}

async fn enabled_atlassian_service(
    client: Arc<RecordingAtlassianClient>,
) -> Arc<AtlassianIntegrationService> {
    let service = Arc::new(AtlassianIntegrationService::new(
        Arc::new(MemoryAtlassianIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ));
    service
        .save_settings(
            Some(AtlassianAuthMethod::ApiToken),
            Some("jira.test".to_string()),
            Some("agent@example.com".to_string()),
            Some("jira-token".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("atlassian settings should save");
    service
        .validate_and_enable()
        .await
        .expect("atlassian should validate");
    service
}

fn disabled_atlassian_service(
    client: Arc<RecordingAtlassianClient>,
) -> Arc<AtlassianIntegrationService> {
    Arc::new(AtlassianIntegrationService::new(
        Arc::new(MemoryAtlassianIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ))
}

async fn enabled_clickup_service(
    client: Arc<dyn ClickUpApiClient>,
) -> Arc<ClickUpIntegrationService> {
    let service = Arc::new(ClickUpIntegrationService::new(
        Arc::new(MemoryClickUpIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ));
    service
        .save_settings(Some("clk-token".to_string()), Some("team-1".to_string()))
        .await
        .expect("clickup settings should save");
    service
        .validate_and_enable()
        .await
        .expect("clickup should validate");
    service
}

fn disabled_clickup_service(client: Arc<dyn ClickUpApiClient>) -> Arc<ClickUpIntegrationService> {
    Arc::new(ClickUpIntegrationService::new(
        Arc::new(MemoryClickUpIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ))
}

fn external_issue_service() -> Arc<ExternalIssueLinkService> {
    Arc::new(ExternalIssueLinkService::new(Arc::new(
        MemoryExternalIssueLinkRepository::new(),
    )))
}

fn service_with_sink(
    atlassian: Arc<AtlassianIntegrationService>,
    linear: Arc<LinearIntegrationService>,
    external_issues: Arc<ExternalIssueLinkService>,
) -> (TicketingService, Arc<RecordingEventSink>) {
    let sink = Arc::new(RecordingEventSink::default());
    let sink_trait: Arc<dyn TicketingEventSink> = sink.clone();
    (
        TicketingService::new(
            atlassian,
            linear,
            disabled_clickup_service(Arc::new(EmptyClickUpApiClient)),
            external_issues,
        )
        .with_event_sink(sink_trait),
        sink,
    )
}

fn clickup_service_with_sink(
    clickup: Arc<ClickUpIntegrationService>,
    external_issues: Arc<ExternalIssueLinkService>,
) -> (TicketingService, Arc<RecordingEventSink>) {
    let sink = Arc::new(RecordingEventSink::default());
    let sink_trait: Arc<dyn TicketingEventSink> = sink.clone();
    (
        TicketingService::new(
            disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
            disabled_linear_service(Arc::new(RecordingLinearClient::default())),
            clickup,
            external_issues,
        )
        .with_event_sink(sink_trait),
        sink,
    )
}

fn linear_ticket() -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: "linear".to_string(),
        id: "issue-1".to_string(),
        key: Some("LIN-1".to_string()),
        local_project_id: Some("project-1".to_string()),
    }
}

fn jira_ticket() -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: "jira".to_string(),
        id: "10001".to_string(),
        key: Some("JRA-1".to_string()),
        local_project_id: Some("project-1".to_string()),
    }
}

fn clickup_ticket() -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: "clickup".to_string(),
        id: "task-abc".to_string(),
        key: None,
        local_project_id: Some("project-1".to_string()),
    }
}

#[tokio::test]
async fn transition_records_unlinked_linear_operation_and_idempotent_retry() {
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = enabled_linear_service(Arc::clone(&linear_client)).await;
    let external_issues = external_issue_service();
    let (service, sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        Arc::clone(&external_issues),
    );

    let request = TicketTransitionRequest {
        ticket: linear_ticket(),
        to_state_id: "done".to_string(),
        provider_transition_id: None,
        client_operation_id: Some("op-linear-transition".to_string()),
    };
    let result = service
        .transition_ticket_status(request.clone())
        .await
        .expect("transition should succeed");

    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    assert_eq!(result.operation.link_id, None);
    assert!(!result.idempotent);
    assert_eq!(
        *linear_client.updates.lock().await,
        vec![("issue-1".to_string(), "done".to_string())]
    );

    let retry = service
        .transition_ticket_status(request)
        .await
        .expect("idempotent retry should succeed");
    assert!(retry.idempotent);
    assert_eq!(linear_client.updates.lock().await.len(), 1);

    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "issue-1",
            Some("LIN-1"),
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        sink.events()
            .iter()
            .map(|event| event.status)
            .collect::<Vec<_>>(),
        vec![
            ProviderTicketOperationStatus::Pending,
            ProviderTicketOperationStatus::Succeeded
        ]
    );
}

#[tokio::test]
async fn invalid_transition_records_failed_operation_without_provider_update() {
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = enabled_linear_service(Arc::clone(&linear_client)).await;
    let external_issues = external_issue_service();
    let (service, sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        Arc::clone(&external_issues),
    );

    let error = service
        .transition_ticket_status(TicketTransitionRequest {
            ticket: linear_ticket(),
            to_state_id: "missing".to_string(),
            provider_transition_id: None,
            client_operation_id: Some("op-linear-missing-transition".to_string()),
        })
        .await
        .expect_err("missing transition should fail");

    assert!(error.contains("Transition target is not available"));
    assert!(linear_client.updates.lock().await.is_empty());
    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "issue-1",
            Some("LIN-1"),
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Failed);
    assert!(records[0]
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("Transition target is not available"));
    assert_eq!(
        sink.events()
            .iter()
            .map(|event| event.status)
            .collect::<Vec<_>>(),
        vec![
            ProviderTicketOperationStatus::Pending,
            ProviderTicketOperationStatus::Failed
        ]
    );
}

#[tokio::test]
async fn linked_jira_transition_updates_operation_and_sync_record() {
    let atlassian_client = Arc::new(RecordingAtlassianClient::default());
    let atlassian = enabled_atlassian_service(Arc::clone(&atlassian_client)).await;
    let linear = disabled_linear_service(Arc::new(RecordingLinearClient::default()));
    let external_issues = external_issue_service();
    let link = external_issues
        .upsert_link(ExternalIssueLinkUpsert {
            provider: "atlassian".to_string(),
            external_kind: "jira".to_string(),
            external_id: "JRA-1".to_string(),
            external_key: Some("JRA-1".to_string()),
            external_url: Some("https://jira.test/browse/JRA-1".to_string()),
            local_object: ExternalIssueLocalObject::task("task-1"),
            local_project_id: Some("project-1".to_string()),
            local_sha: Some("abc123".to_string()),
            local_state: Some("ready".to_string()),
            idempotency_key: "atlassian:jira:JRA-1:task:task-1".to_string(),
            metadata_json: None,
        })
        .await
        .expect("link should save");
    let (service, _sink) = service_with_sink(atlassian, linear, Arc::clone(&external_issues));

    let result = service
        .transition_ticket_status(TicketTransitionRequest {
            ticket: jira_ticket(),
            to_state_id: "done".to_string(),
            provider_transition_id: Some("31".to_string()),
            client_operation_id: Some("op-jira-transition".to_string()),
        })
        .await
        .expect("jira transition should succeed");

    assert_eq!(result.operation.link_id.as_deref(), Some(link.id.as_str()));
    assert_eq!(
        result.operation.provider_operation_id.as_deref(),
        Some("31")
    );
    assert_eq!(
        *atlassian_client.transitions.lock().await,
        vec![("JRA-1".to_string(), "31".to_string())]
    );
    let sync_records = external_issues
        .list_sync_records_for_link(&link.id)
        .await
        .expect("sync records should load");
    assert_eq!(sync_records.len(), 1);
    assert_eq!(sync_records[0].sync_kind, "ticket_transition");
    assert_eq!(sync_records[0].status, ExternalIssueSyncStatus::Succeeded);
    assert_eq!(sync_records[0].local_sha.as_deref(), Some("abc123"));
}

#[tokio::test]
async fn permission_failure_records_failed_assignment_history() {
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = disabled_linear_service(Arc::clone(&linear_client));
    let external_issues = external_issue_service();
    let (service, _sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        Arc::clone(&external_issues),
    );

    let error = service
        .assign_ticket(TicketAssignRequest {
            ticket: linear_ticket(),
            client_operation_id: Some("op-linear-assign-disabled".to_string()),
        })
        .await
        .expect_err("disabled provider should fail");

    assert_eq!(error, "Linear integration is not enabled");
    assert!(linear_client.assignments.lock().await.is_empty());
    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "issue-1",
            Some("LIN-1"),
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation, ProviderTicketOperationKind::Assign);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Failed);
}

#[tokio::test]
async fn clear_assignee_records_assignment_operation_and_is_idempotent() {
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = enabled_linear_service(Arc::clone(&linear_client)).await;
    let external_issues = external_issue_service();
    let (service, _sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        Arc::clone(&external_issues),
    );

    let request = TicketAssignRequest {
        ticket: linear_ticket(),
        client_operation_id: Some("op-linear-clear-assignee".to_string()),
    };
    let result = service
        .clear_ticket_assignee(request.clone())
        .await
        .expect("clear assignee should succeed");

    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    assert_eq!(result.assignee, None);
    assert_eq!(
        *linear_client.assignment_clears.lock().await,
        vec!["issue-1".to_string()]
    );

    let retry = service
        .clear_ticket_assignee(request)
        .await
        .expect("idempotent clear retry should succeed");
    assert!(retry.idempotent);
    assert_eq!(linear_client.assignment_clears.lock().await.len(), 1);

    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "issue-1",
            Some("LIN-1"),
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation, ProviderTicketOperationKind::Assign);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        records[0].metadata_json.as_deref(),
        Some(r#"{"assignee":null}"#)
    );
}

#[tokio::test]
async fn assign_ticket_records_jira_operation_and_returns_current_user() {
    let atlassian_client = Arc::new(RecordingAtlassianClient::default());
    let atlassian = enabled_atlassian_service(Arc::clone(&atlassian_client)).await;
    let external_issues = external_issue_service();
    let (service, _sink) = service_with_sink(
        atlassian,
        disabled_linear_service(Arc::new(RecordingLinearClient::default())),
        Arc::clone(&external_issues),
    );

    let result = service
        .assign_ticket(TicketAssignRequest {
            ticket: jira_ticket(),
            client_operation_id: Some("op-jira-assign".to_string()),
        })
        .await
        .expect("jira assign should succeed");

    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    assert_eq!(
        result.operation.operation,
        ProviderTicketOperationKind::Assign
    );
    assert!(!result.idempotent);
    assert_eq!(
        *atlassian_client.assignments.lock().await,
        vec!["JRA-1".to_string()]
    );
    // Jira normalizes its external id to the issue key and records operations
    // under the jira provider/kind pair.
    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "jira",
            "jira",
            "JRA-1",
            Some("JRA-1"),
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation, ProviderTicketOperationKind::Assign);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        records[0].metadata_json.as_deref(),
        Some(r#"{"assignee":"current_user"}"#)
    );
}

#[tokio::test]
async fn add_comment_records_unlinked_linear_operation_and_is_idempotent() {
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = enabled_linear_service(Arc::clone(&linear_client)).await;
    let external_issues = external_issue_service();
    let (service, sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        Arc::clone(&external_issues),
    );

    let request = TicketCommentRequest {
        ticket: linear_ticket(),
        body_markdown: "Looks good to me".to_string(),
        client_operation_id: Some("op-linear-comment".to_string()),
    };
    let result = service
        .add_ticket_comment(request.clone())
        .await
        .expect("comment should succeed");

    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    assert_eq!(
        result.operation.operation,
        ProviderTicketOperationKind::Comment
    );
    assert_eq!(result.operation.link_id, None);
    assert!(!result.idempotent);
    let comment = result.comment.expect("comment payload should be present");
    assert_eq!(comment.body_markdown, "Looks good to me");
    assert_eq!(comment.author_name.as_deref(), Some("A. User"));
    assert_eq!(
        *linear_client.comments.lock().await,
        vec![("issue-1".to_string(), "Looks good to me".to_string())]
    );

    let retry = service
        .add_ticket_comment(request)
        .await
        .expect("idempotent comment retry should succeed");
    assert!(retry.idempotent);
    // Idempotent retry must not call the provider a second time.
    assert_eq!(linear_client.comments.lock().await.len(), 1);

    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "issue-1",
            Some("LIN-1"),
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation, ProviderTicketOperationKind::Comment);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        sink.events()
            .iter()
            .map(|event| event.status)
            .collect::<Vec<_>>(),
        vec![
            ProviderTicketOperationStatus::Pending,
            ProviderTicketOperationStatus::Succeeded
        ]
    );
}

#[tokio::test]
async fn add_comment_on_disabled_provider_records_failed_operation() {
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = disabled_linear_service(Arc::clone(&linear_client));
    let external_issues = external_issue_service();
    let (service, _sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        Arc::clone(&external_issues),
    );

    let error = service
        .add_ticket_comment(TicketCommentRequest {
            ticket: linear_ticket(),
            body_markdown: "blocked".to_string(),
            client_operation_id: Some("op-linear-comment-disabled".to_string()),
        })
        .await
        .expect_err("disabled provider should fail to comment");

    assert_eq!(error, "Linear integration is not enabled");
    assert!(linear_client.comments.lock().await.is_empty());
    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "issue-1",
            Some("LIN-1"),
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation, ProviderTicketOperationKind::Comment);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Failed);
}

#[tokio::test]
async fn linked_jira_comment_writes_ticket_comment_sync_record() {
    let atlassian_client = Arc::new(RecordingAtlassianClient::default());
    let atlassian = enabled_atlassian_service(Arc::clone(&atlassian_client)).await;
    let external_issues = external_issue_service();
    let link = external_issues
        .upsert_link(ExternalIssueLinkUpsert {
            provider: "atlassian".to_string(),
            external_kind: "jira".to_string(),
            external_id: "JRA-1".to_string(),
            external_key: Some("JRA-1".to_string()),
            external_url: Some("https://jira.test/browse/JRA-1".to_string()),
            local_object: ExternalIssueLocalObject::task("task-1"),
            local_project_id: Some("project-1".to_string()),
            local_sha: Some("abc123".to_string()),
            local_state: Some("ready".to_string()),
            idempotency_key: "atlassian:jira:JRA-1:task:task-1".to_string(),
            metadata_json: None,
        })
        .await
        .expect("link should save");
    let (service, _sink) = service_with_sink(
        atlassian,
        disabled_linear_service(Arc::new(RecordingLinearClient::default())),
        Arc::clone(&external_issues),
    );

    let result = service
        .add_ticket_comment(TicketCommentRequest {
            ticket: jira_ticket(),
            body_markdown: "Linked comment".to_string(),
            client_operation_id: Some("op-jira-comment".to_string()),
        })
        .await
        .expect("jira comment should succeed");

    assert_eq!(result.operation.link_id.as_deref(), Some(link.id.as_str()));
    assert_eq!(
        *atlassian_client.comments.lock().await,
        vec![("JRA-1".to_string(), "Linked comment".to_string())]
    );
    let sync_records = external_issues
        .list_sync_records_for_link(&link.id)
        .await
        .expect("sync records should load");
    assert_eq!(sync_records.len(), 1);
    assert_eq!(sync_records[0].sync_kind, "ticket_comment");
    assert_eq!(sync_records[0].status, ExternalIssueSyncStatus::Succeeded);
}

#[tokio::test]
async fn set_ticket_labels_forwards_full_array_for_jira_and_is_idempotent() {
    let atlassian_client = Arc::new(RecordingAtlassianClient::default());
    let atlassian = enabled_atlassian_service(Arc::clone(&atlassian_client)).await;
    let external_issues = external_issue_service();
    let (service, sink) = service_with_sink(
        atlassian,
        disabled_linear_service(Arc::new(RecordingLinearClient::default())),
        Arc::clone(&external_issues),
    );

    let request = TicketSetLabelsRequest {
        ticket: jira_ticket(),
        labels: vec!["frontend".to_string(), "bug".to_string()],
        client_operation_id: Some("op-jira-labels".to_string()),
    };
    let result = service
        .set_ticket_labels(request.clone())
        .await
        .expect("label set should succeed");

    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    assert_eq!(
        result.operation.operation,
        ProviderTicketOperationKind::SetLabels
    );
    assert!(!result.idempotent);
    let labels = result.labels.expect("labels payload should be present");
    // Normalized: sorted + deduped.
    assert_eq!(
        labels.labels,
        vec!["bug".to_string(), "frontend".to_string()]
    );
    assert_eq!(
        *atlassian_client.label_writes.lock().await,
        vec![(
            "JRA-1".to_string(),
            vec!["bug".to_string(), "frontend".to_string()]
        )]
    );

    // Second call with the same client_operation_id is idempotent and does not
    // re-invoke the provider.
    let retry = service
        .set_ticket_labels(request)
        .await
        .expect("idempotent label retry should succeed");
    assert!(retry.idempotent);
    assert_eq!(atlassian_client.label_writes.lock().await.len(), 1);

    assert_eq!(
        sink.events()
            .iter()
            .map(|event| event.status)
            .collect::<Vec<_>>(),
        vec![
            ProviderTicketOperationStatus::Pending,
            ProviderTicketOperationStatus::Succeeded
        ]
    );
}

#[tokio::test]
async fn set_ticket_labels_resolves_names_to_ids_for_linear() {
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = enabled_linear_service(Arc::clone(&linear_client)).await;
    let external_issues = external_issue_service();
    let (service, _sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        Arc::clone(&external_issues),
    );

    let result = service
        .set_ticket_labels(TicketSetLabelsRequest {
            ticket: linear_ticket(),
            // Mixed case / whitespace exercises the resolver's normalization.
            labels: vec![" bug ".to_string(), "Feature".to_string()],
            client_operation_id: Some("op-linear-labels".to_string()),
        })
        .await
        .expect("label set should succeed");

    assert_eq!(
        result.operation.operation,
        ProviderTicketOperationKind::SetLabels
    );
    // Resolved to team label ids (issue-1 is the external id for linear_ticket()).
    assert_eq!(
        *linear_client.label_updates.lock().await,
        vec![(
            "issue-1".to_string(),
            vec!["label-bug".to_string(), "label-feature".to_string()]
        )]
    );
}

#[tokio::test]
async fn set_ticket_labels_normalization_produces_stable_idempotency() {
    let atlassian_client = Arc::new(RecordingAtlassianClient::default());
    let atlassian = enabled_atlassian_service(Arc::clone(&atlassian_client)).await;
    let external_issues = external_issue_service();
    let (service, _sink) = service_with_sink(
        atlassian,
        disabled_linear_service(Arc::new(RecordingLinearClient::default())),
        Arc::clone(&external_issues),
    );

    // First call: duplicate + whitespace + reversed order, no explicit op id.
    let first = service
        .set_ticket_labels(TicketSetLabelsRequest {
            ticket: jira_ticket(),
            labels: vec![
                "frontend".to_string(),
                " bug ".to_string(),
                "bug".to_string(),
            ],
            client_operation_id: None,
        })
        .await
        .expect("first label set should succeed");
    assert!(!first.idempotent);

    // Second call: same logical set, different surface form (reordered + extra
    // whitespace), no explicit op id. The derived client_operation_id (hash of
    // the normalized set) must match, so this short-circuits as idempotent.
    let second = service
        .set_ticket_labels(TicketSetLabelsRequest {
            ticket: jira_ticket(),
            labels: vec!["  bug".to_string(), "frontend  ".to_string()],
            client_operation_id: None,
        })
        .await
        .expect("second label set should succeed");
    assert!(second.idempotent);
    assert_eq!(atlassian_client.label_writes.lock().await.len(), 1);
}

#[tokio::test]
async fn set_ticket_labels_on_disabled_provider_records_failed_operation() {
    let atlassian_client = Arc::new(RecordingAtlassianClient::default());
    let atlassian = disabled_atlassian_service(Arc::clone(&atlassian_client));
    let external_issues = external_issue_service();
    let (service, sink) = service_with_sink(
        atlassian,
        disabled_linear_service(Arc::new(RecordingLinearClient::default())),
        Arc::clone(&external_issues),
    );

    let error = service
        .set_ticket_labels(TicketSetLabelsRequest {
            ticket: jira_ticket(),
            labels: vec!["bug".to_string()],
            client_operation_id: Some("op-jira-labels-disabled".to_string()),
        })
        .await
        .expect_err("disabled provider should fail to set labels");

    assert_eq!(error, "Jira integration is not enabled");
    assert!(atlassian_client.label_writes.lock().await.is_empty());
    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "jira",
            "jira",
            "JRA-1",
            Some("JRA-1"),
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation, ProviderTicketOperationKind::SetLabels);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Failed);
    assert_eq!(
        sink.events()
            .iter()
            .map(|event| event.status)
            .collect::<Vec<_>>(),
        vec![
            ProviderTicketOperationStatus::Pending,
            ProviderTicketOperationStatus::Failed
        ]
    );
}

// ── clickup write-backs ─────────────────────────────────────────────────────

#[tokio::test]
async fn clickup_comment_records_operation_and_is_idempotent() {
    let clickup_client = Arc::new(RecordingClickUpClient::default());
    let clickup = enabled_clickup_service(clickup_client.clone()).await;
    let external_issues = external_issue_service();
    let (service, sink) = clickup_service_with_sink(clickup, Arc::clone(&external_issues));

    let request = TicketCommentRequest {
        ticket: clickup_ticket(),
        body_markdown: "ClickUp note".to_string(),
        client_operation_id: Some("op-clickup-comment".to_string()),
    };
    let result = service
        .add_ticket_comment(request.clone())
        .await
        .expect("clickup comment should succeed");

    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    assert_eq!(
        result.operation.operation,
        ProviderTicketOperationKind::Comment
    );
    assert!(!result.idempotent);
    let comment = result.comment.expect("comment payload should be present");
    assert_eq!(comment.body_markdown, "ClickUp note");
    assert_eq!(comment.id.as_deref(), Some("clk-comment-1"));
    assert_eq!(comment.author_name.as_deref(), Some("A. User"));
    assert_eq!(
        *clickup_client.comments.lock().await,
        vec![("task-abc".to_string(), "ClickUp note".to_string())]
    );

    let retry = service
        .add_ticket_comment(request)
        .await
        .expect("idempotent clickup comment retry should succeed");
    assert!(retry.idempotent);
    // Idempotent retry must not call the provider a second time.
    assert_eq!(clickup_client.comments.lock().await.len(), 1);

    // Operations are recorded under the clickup provider/task pair (no key).
    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "clickup",
            "task",
            "task-abc",
            None,
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation, ProviderTicketOperationKind::Comment);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Succeeded);

    // Write-backs emit the same Pending→Succeeded operation events as Linear/Jira
    // (tagged with the clickup provider), which is what drives ticketing cache
    // invalidation in the UI.
    assert_eq!(
        sink.events()
            .iter()
            .map(|event| event.status)
            .collect::<Vec<_>>(),
        vec![
            ProviderTicketOperationStatus::Pending,
            ProviderTicketOperationStatus::Succeeded
        ]
    );
    assert!(sink
        .events()
        .iter()
        .all(|event| event.provider == "clickup"));
}

#[tokio::test]
async fn clickup_transition_routes_through_status_update() {
    let clickup_client = Arc::new(RecordingClickUpClient::default());
    let clickup = enabled_clickup_service(clickup_client.clone()).await;
    let external_issues = external_issue_service();
    let (service, _sink) = clickup_service_with_sink(clickup, Arc::clone(&external_issues));

    // ClickUp transitions are listed from the task's space statuses and applied
    // by status name.
    let options = service
        .list_transitions(&clickup_ticket())
        .await
        .expect("clickup transitions should load");
    let names: Vec<&str> = options.iter().map(|o| o.to_state_id.as_str()).collect();
    assert_eq!(names, vec!["to do", "complete"]);
    assert!(options.iter().all(|o| o.provider_transition_id.is_none()));

    let result = service
        .transition_ticket_status(TicketTransitionRequest {
            ticket: clickup_ticket(),
            to_state_id: "complete".to_string(),
            provider_transition_id: None,
            client_operation_id: Some("op-clickup-transition".to_string()),
        })
        .await
        .expect("clickup transition should succeed");

    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    let transition = result.transition.expect("transition payload should exist");
    assert_eq!(transition.category, "done");
    assert_eq!(
        *clickup_client.status_updates.lock().await,
        vec![("task-abc".to_string(), "complete".to_string())]
    );
}

#[tokio::test]
async fn clickup_assign_records_operation_and_returns_current_user() {
    let clickup_client = Arc::new(RecordingClickUpClient::default());
    let clickup = enabled_clickup_service(clickup_client.clone()).await;
    let external_issues = external_issue_service();
    let (service, _sink) = clickup_service_with_sink(clickup, Arc::clone(&external_issues));

    let result = service
        .assign_ticket(TicketAssignRequest {
            ticket: clickup_ticket(),
            client_operation_id: Some("op-clickup-assign".to_string()),
        })
        .await
        .expect("clickup assign should succeed");

    assert_eq!(
        result.operation.operation,
        ProviderTicketOperationKind::Assign
    );
    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    let assignee = result.assignee.expect("assignee payload should exist");
    assert_eq!(assignee.name, "A. User");
    assert_eq!(assignee.id.as_deref(), Some("7"));
    assert_eq!(
        *clickup_client.assignments.lock().await,
        vec!["task-abc".to_string()]
    );
}

#[tokio::test]
async fn clickup_comment_on_disabled_provider_records_failed_operation() {
    let clickup_client = Arc::new(RecordingClickUpClient::default());
    let clickup = disabled_clickup_service(clickup_client.clone());
    let external_issues = external_issue_service();
    let (service, _sink) = clickup_service_with_sink(clickup, Arc::clone(&external_issues));

    let error = service
        .add_ticket_comment(TicketCommentRequest {
            ticket: clickup_ticket(),
            body_markdown: "blocked".to_string(),
            client_operation_id: Some("op-clickup-comment-disabled".to_string()),
        })
        .await
        .expect_err("disabled clickup provider should fail to comment");

    assert_eq!(error, "ClickUp integration is not enabled");
    assert!(clickup_client.comments.lock().await.is_empty());
    let records = external_issues
        .list_provider_ticket_operations_for_ticket(
            "clickup",
            "task",
            "task-abc",
            None,
            Some("project-1"),
        )
        .await
        .expect("operation history should load");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation, ProviderTicketOperationKind::Comment);
    assert_eq!(records[0].status, ProviderTicketOperationStatus::Failed);
}

#[test]
fn tauri_event_sink_emits_operation_updated_event_to_listeners() {
    use std::sync::mpsc;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Listener;

    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock tauri app should build");
    let handle = app.handle().clone();

    let (tx, rx) = mpsc::channel::<String>();
    handle.listen(TICKETING_OPERATION_EVENT, move |event| {
        let _ = tx.send(event.payload().to_string());
    });

    let sink = TauriTicketingEventSink::new(handle);
    sink.emit_ticketing_operation_event(TicketingOperationEvent {
        provider: "linear".to_string(),
        external_kind: "issue".to_string(),
        external_id: "issue-1".to_string(),
        external_key: Some("LIN-1".to_string()),
        local_project_id: Some("project-1".to_string()),
        operation_id: "operation-1".to_string(),
        operation: ProviderTicketOperationKind::Comment,
        client_operation_id: "client-op-1".to_string(),
        status: ProviderTicketOperationStatus::Succeeded,
        provider_operation_id: Some("comment-1".to_string()),
        error_message: None,
    });

    let payload = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("listener should receive the emitted ticketing operation event");
    assert!(payload.contains("\"provider\":\"linear\""));
    assert!(payload.contains("\"operationId\":\"operation-1\""));
    assert!(payload.contains("\"status\":\"succeeded\""));
    assert!(payload.contains("\"operation\":\"comment\""));
}

// ── normalize_ticket_identity ───────────────────────────────────────────────

#[test]
fn normalize_ticket_identity_rejects_unknown_provider() {
    let ticket = TicketingTicketIdentity {
        provider: "asana".to_string(),
        id: "1".to_string(),
        key: None,
        local_project_id: None,
    };
    let err = normalize_ticket_identity(&ticket).unwrap_err();
    assert!(err.contains("Unknown ticketing provider"), "got: {err}");
}

#[test]
fn normalize_ticket_identity_rejects_blank_id() {
    let ticket = TicketingTicketIdentity {
        provider: "jira".to_string(),
        id: "   ".to_string(),
        key: Some("JRA-1".to_string()),
        local_project_id: None,
    };
    let err = normalize_ticket_identity(&ticket).unwrap_err();
    assert!(err.contains("Ticket id is required"), "got: {err}");
}

#[test]
fn normalize_ticket_identity_jira_uses_key_as_external_id() {
    let ticket = TicketingTicketIdentity {
        provider: " jira ".to_string(),
        id: " 10001 ".to_string(),
        key: Some("  JRA-1  ".to_string()),
        local_project_id: Some("  proj-1  ".to_string()),
    };
    let identity = normalize_ticket_identity(&ticket).unwrap();
    assert_eq!(identity.provider, "jira");
    assert_eq!(identity.external_kind, "jira");
    assert_eq!(identity.external_id, "JRA-1");
    assert_eq!(identity.external_key.as_deref(), Some("JRA-1"));
    assert_eq!(identity.local_project_id.as_deref(), Some("proj-1"));
}

#[test]
fn normalize_ticket_identity_jira_falls_back_to_id_without_key() {
    let ticket = TicketingTicketIdentity {
        provider: "jira".to_string(),
        id: "10001".to_string(),
        key: None,
        local_project_id: None,
    };
    let identity = normalize_ticket_identity(&ticket).unwrap();
    assert_eq!(identity.external_id, "10001");
    assert_eq!(identity.external_key, None);
}

#[test]
fn normalize_ticket_identity_linear_uses_id_not_key() {
    let ticket = TicketingTicketIdentity {
        provider: "linear".to_string(),
        id: "issue-1".to_string(),
        key: Some("LIN-1".to_string()),
        local_project_id: None,
    };
    let identity = normalize_ticket_identity(&ticket).unwrap();
    assert_eq!(identity.external_kind, "issue");
    assert_eq!(identity.external_id, "issue-1");
    assert_eq!(identity.external_key.as_deref(), Some("LIN-1"));
}

#[test]
fn normalize_ticket_identity_clickup_uses_id_and_task_kind() {
    let ticket = TicketingTicketIdentity {
        provider: " clickup ".to_string(),
        id: " task-abc ".to_string(),
        key: None,
        local_project_id: Some(" proj-1 ".to_string()),
    };
    let identity = normalize_ticket_identity(&ticket).unwrap();
    assert_eq!(identity.provider, "clickup");
    assert_eq!(identity.external_kind, "task");
    assert_eq!(identity.external_id, "task-abc");
    assert_eq!(identity.external_key, None);
    assert_eq!(identity.local_project_id.as_deref(), Some("proj-1"));
}

#[test]
fn normalize_ticket_identity_drops_blank_key_and_project() {
    let ticket = TicketingTicketIdentity {
        provider: "linear".to_string(),
        id: "issue-1".to_string(),
        key: Some("   ".to_string()),
        local_project_id: Some(String::new()),
    };
    let identity = normalize_ticket_identity(&ticket).unwrap();
    assert_eq!(identity.external_key, None);
    assert_eq!(identity.local_project_id, None);
}

// ── required_trimmed ────────────────────────────────────────────────────────

#[test]
fn required_trimmed_errors_on_empty_or_whitespace() {
    assert_eq!(required_trimmed("", "needed").unwrap_err(), "needed");
    assert_eq!(required_trimmed("   ", "needed").unwrap_err(), "needed");
}

#[test]
fn required_trimmed_returns_trimmed_value() {
    assert_eq!(required_trimmed("  hi  ", "needed").unwrap(), "hi");
}

// ── normalize_labels ────────────────────────────────────────────────────────

#[test]
fn normalize_labels_trims_dedupes_case_insensitively_and_sorts() {
    let input = vec![
        "  Frontend ".to_string(),
        "bug".to_string(),
        "BUG".to_string(),
        "   ".to_string(),
        "apex".to_string(),
    ];
    // Empty entry dropped; "BUG" deduped (first surface form "bug" kept);
    // sorted case-insensitively.
    assert_eq!(
        normalize_labels(&input),
        vec![
            "apex".to_string(),
            "bug".to_string(),
            "Frontend".to_string()
        ]
    );
}

#[test]
fn normalize_labels_empty_input_yields_empty() {
    assert!(normalize_labels(&[]).is_empty());
    assert!(normalize_labels(&["  ".to_string()]).is_empty());
}

// ── find_transition ─────────────────────────────────────────────────────────

fn transition(
    to_state_id: &str,
    provider_transition_id: Option<&str>,
) -> TicketingTransitionOption {
    TicketingTransitionOption {
        to_state_id: to_state_id.to_string(),
        provider_transition_id: provider_transition_id.map(str::to_string),
        name: format!("To {to_state_id}"),
        category: "todo".to_string(),
        disabled_reason: None,
    }
}

#[test]
fn find_transition_by_provider_transition_id_takes_precedence() {
    let options = vec![
        transition("done", Some("31")),
        transition("in_progress", Some("21")),
    ];
    // Match by provider transition id even when to_state_id differs.
    let found = find_transition(&options, "ignored-state", Some("21")).unwrap();
    assert_eq!(found.to_state_id, "in_progress");
}

#[test]
fn find_transition_by_to_state_id_when_no_provider_id() {
    let options = vec![transition("done", None), transition("in_progress", None)];
    let found = find_transition(&options, "done", None).unwrap();
    assert_eq!(found.to_state_id, "done");
}

#[test]
fn find_transition_blank_provider_id_falls_back_to_state_id() {
    let options = vec![transition("done", Some("31"))];
    // A blank provider id is filtered out, so it matches on to_state_id.
    let found = find_transition(&options, "done", Some("   ")).unwrap();
    assert_eq!(found.to_state_id, "done");
}

#[test]
fn find_transition_not_found_errors() {
    let options = vec![transition("done", Some("31"))];
    let err = find_transition(&options, "nope", None).unwrap_err();
    assert!(err.contains("not available"), "got: {err}");
    let err = find_transition(&options, "done", Some("99")).unwrap_err();
    assert!(err.contains("not available"), "got: {err}");
}

#[test]
fn find_transition_disabled_returns_reason() {
    let mut disabled = transition("done", Some("31"));
    disabled.disabled_reason = Some("Requires approval".to_string());
    let err = find_transition(&[disabled], "done", None).unwrap_err();
    assert_eq!(err, "Requires approval");
}

// ── provider → ticketing mapping ────────────────────────────────────────────

#[test]
fn jira_transition_option_maps_all_fields() {
    let option = jira_transition_option(AtlassianJiraTransition {
        provider_transition_id: "31".to_string(),
        to_state_id: "done".to_string(),
        name: "Done".to_string(),
        category: "done".to_string(),
    });
    assert_eq!(option.to_state_id, "done");
    assert_eq!(option.provider_transition_id.as_deref(), Some("31"));
    assert_eq!(option.name, "Done");
    assert_eq!(option.category, "done");
    assert_eq!(option.disabled_reason, None);
}

#[test]
fn linear_transition_option_has_no_provider_transition_id() {
    let option = linear_transition_option(LinearWorkflowState {
        id: "state-done".to_string(),
        name: "Done".to_string(),
        category: "done".to_string(),
        color: Some("#fff".to_string()),
    });
    assert_eq!(option.to_state_id, "state-done");
    assert_eq!(option.provider_transition_id, None);
    assert_eq!(option.name, "Done");
}

#[test]
fn jira_comment_result_maps_author_and_bodies() {
    let result = jira_comment_result(AtlassianJiraComment {
        id: Some("c1".to_string()),
        author: Some("A. User".to_string()),
        body_markdown: "**hi**".to_string(),
        body_text: "hi".to_string(),
        created_at: Some("2026-06-20T08:00:00Z".to_string()),
        updated_at: None,
    });
    assert_eq!(result.id.as_deref(), Some("c1"));
    assert_eq!(result.author_name.as_deref(), Some("A. User"));
    assert_eq!(result.body_markdown, "**hi**");
    assert_eq!(result.body_text, "hi");
    assert_eq!(result.updated_at, None);
}

#[test]
fn linear_comment_result_duplicates_body_into_text() {
    let result = linear_comment_result(LinearComment {
        id: "c1".to_string(),
        body: "hello".to_string(),
        author_id: Some("u1".to_string()),
        author_name: Some("A. User".to_string()),
        created_at: None,
        updated_at: None,
    });
    assert_eq!(result.id.as_deref(), Some("c1"));
    assert_eq!(result.body_markdown, "hello");
    assert_eq!(result.body_text, "hello");
    assert_eq!(result.author_name.as_deref(), Some("A. User"));
}

#[test]
fn linear_user_to_person_defaults_name_to_me() {
    let person = linear_user_to_ticketing_person(LinearUser {
        id: "u1".to_string(),
        name: None,
    });
    assert_eq!(person.id.as_deref(), Some("u1"));
    assert_eq!(person.name, "Me");

    let named = linear_user_to_ticketing_person(LinearUser {
        id: "u2".to_string(),
        name: Some("Ada".to_string()),
    });
    assert_eq!(named.name, "Ada");
}

#[test]
fn clickup_transition_option_maps_status_name_as_state_id() {
    let option = clickup_transition_option(ClickUpStatus {
        id: Some("status-1".to_string()),
        status: "in progress".to_string(),
        status_type: "custom".to_string(),
        category: "in_progress".to_string(),
        color: None,
        orderindex: Some(1),
    });
    // ClickUp has no separate transition id; the status name is the target id.
    assert_eq!(option.to_state_id, "in progress");
    assert_eq!(option.name, "in progress");
    assert_eq!(option.category, "in_progress");
    assert_eq!(option.provider_transition_id, None);
    assert_eq!(option.disabled_reason, None);
}

#[test]
fn clickup_comment_result_duplicates_body_and_drops_updated_at() {
    let result = clickup_comment_result(ClickUpComment {
        id: "c1".to_string(),
        body: "hello".to_string(),
        author_id: Some(7),
        author_name: Some("A. User".to_string()),
        created_at: Some("2026-06-20T08:00:00Z".to_string()),
        attachments: Vec::new(),
        replies: Vec::new(),
    });
    assert_eq!(result.id.as_deref(), Some("c1"));
    assert_eq!(result.body_markdown, "hello");
    assert_eq!(result.body_text, "hello");
    assert_eq!(result.author_name.as_deref(), Some("A. User"));
    assert_eq!(result.created_at.as_deref(), Some("2026-06-20T08:00:00Z"));
    assert_eq!(result.updated_at, None);
}

#[test]
fn clickup_user_to_person_defaults_name_to_me() {
    let person = clickup_user_to_ticketing_person(ClickUpUser {
        id: 42,
        username: None,
        email: None,
    });
    assert_eq!(person.id.as_deref(), Some("42"));
    assert_eq!(person.name, "Me");

    let named = clickup_user_to_ticketing_person(ClickUpUser {
        id: 7,
        username: Some("Ada".to_string()),
        email: None,
    });
    assert_eq!(named.name, "Ada");
}

// ── metadata + sync mapping ─────────────────────────────────────────────────

#[test]
fn transition_metadata_serializes_ids() {
    let json = transition_metadata("done", Some("31"));
    assert!(json.contains("\"toStateId\":\"done\""));
    assert!(json.contains("\"providerTransitionId\":\"31\""));

    let json_none = transition_metadata("done", None);
    assert!(json_none.contains("\"providerTransitionId\":null"));
}

#[test]
fn success_transition_metadata_includes_name_and_category() {
    let json = success_transition_metadata(&transition("done", Some("31")));
    assert!(json.contains("\"toStateId\":\"done\""));
    assert!(json.contains("\"providerTransitionId\":\"31\""));
    assert!(json.contains("\"category\":\"todo\""));
}

#[test]
fn sync_kind_for_maps_each_operation_kind() {
    assert_eq!(
        sync_kind_for(ProviderTicketOperationKind::Transition),
        "ticket_transition"
    );
    assert_eq!(
        sync_kind_for(ProviderTicketOperationKind::Assign),
        "ticket_assignment"
    );
    assert_eq!(
        sync_kind_for(ProviderTicketOperationKind::Comment),
        "ticket_comment"
    );
    assert_eq!(
        sync_kind_for(ProviderTicketOperationKind::SetLabels),
        "ticket_labels"
    );
}

#[test]
fn sync_status_for_collapses_failure_variants() {
    assert_eq!(
        sync_status_for(ProviderTicketOperationStatus::Pending),
        ExternalIssueSyncStatus::Pending
    );
    assert_eq!(
        sync_status_for(ProviderTicketOperationStatus::Succeeded),
        ExternalIssueSyncStatus::Succeeded
    );
    for failed in [
        ProviderTicketOperationStatus::Failed,
        ProviderTicketOperationStatus::TimedOut,
        ProviderTicketOperationStatus::Canceled,
    ] {
        assert_eq!(sync_status_for(failed), ExternalIssueSyncStatus::Failed);
    }
}

// ── client_operation_id_or_derive + stable_hash ─────────────────────────────

#[test]
fn client_operation_id_prefers_provided_value() {
    let identity = normalize_ticket_identity(&jira_ticket()).unwrap();
    let id = client_operation_id_or_derive(
        Some("  explicit-op  "),
        &identity,
        ProviderTicketOperationKind::Transition,
        "ignored",
    );
    assert_eq!(id, "explicit-op");
}

#[test]
fn client_operation_id_derives_deterministically_when_absent() {
    let identity = normalize_ticket_identity(&jira_ticket()).unwrap();
    let a = client_operation_id_or_derive(
        None,
        &identity,
        ProviderTicketOperationKind::Transition,
        "suffix",
    );
    let b = client_operation_id_or_derive(
        Some("   "),
        &identity,
        ProviderTicketOperationKind::Transition,
        "suffix",
    );
    // Blank provided value is treated as absent → same derived id.
    assert_eq!(a, b);
    assert!(a.starts_with("ticketing:jira:jira:JRA-1:transition:"));

    // Different suffix → different derived id.
    let c = client_operation_id_or_derive(
        None,
        &identity,
        ProviderTicketOperationKind::Transition,
        "other",
    );
    assert_ne!(a, c);
}

#[test]
fn stable_hash_is_deterministic_and_hex() {
    let a = stable_hash("payload");
    let b = stable_hash("payload");
    assert_eq!(a, b);
    assert_eq!(a.len(), 64, "sha256 hex is 64 chars");
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, stable_hash("different"));
}

// ── list_transitions + capability gating ────────────────────────────────────

#[tokio::test]
async fn list_transitions_routes_to_jira_client() {
    let atlassian_client = Arc::new(RecordingAtlassianClient::default());
    let atlassian = enabled_atlassian_service(Arc::clone(&atlassian_client)).await;
    let (service, _sink) = service_with_sink(
        atlassian,
        disabled_linear_service(Arc::new(RecordingLinearClient::default())),
        external_issue_service(),
    );

    let options = service
        .list_transitions(&jira_ticket())
        .await
        .expect("jira transitions should load");
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].to_state_id, "done");
    assert_eq!(options[0].provider_transition_id.as_deref(), Some("31"));
}

#[tokio::test]
async fn list_transitions_routes_to_linear_client() {
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = enabled_linear_service(Arc::clone(&linear_client)).await;
    let (service, _sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        external_issue_service(),
    );

    let options = service
        .list_transitions(&linear_ticket())
        .await
        .expect("linear transitions should load");
    let ids: Vec<&str> = options.iter().map(|o| o.to_state_id.as_str()).collect();
    assert_eq!(ids, vec!["todo", "done"]);
    // Linear workflow states never carry a provider transition id.
    assert!(options.iter().all(|o| o.provider_transition_id.is_none()));
}

#[tokio::test]
async fn list_transitions_on_disabled_provider_errors() {
    let (service, _sink) = service_with_sink(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        disabled_linear_service(Arc::new(RecordingLinearClient::default())),
        external_issue_service(),
    );

    let err = service
        .list_transitions(&linear_ticket())
        .await
        .expect_err("disabled provider must reject list_transitions");
    assert!(!err.is_empty());
}

// ── event emission without a sink ───────────────────────────────────────────

#[tokio::test]
async fn operations_succeed_without_an_event_sink() {
    // Build a service WITHOUT calling with_event_sink, exercising the
    // emit_operation_event no-sink branch end-to-end.
    let linear_client = Arc::new(RecordingLinearClient::default());
    let linear = enabled_linear_service(Arc::clone(&linear_client)).await;
    let service = TicketingService::new(
        disabled_atlassian_service(Arc::new(RecordingAtlassianClient::default())),
        linear,
        disabled_clickup_service(Arc::new(EmptyClickUpApiClient)),
        external_issue_service(),
    );

    let result = service
        .transition_ticket_status(TicketTransitionRequest {
            ticket: linear_ticket(),
            to_state_id: "done".to_string(),
            provider_transition_id: None,
            client_operation_id: Some("op-no-sink".to_string()),
        })
        .await
        .expect("transition should succeed even without an event sink");
    assert_eq!(
        result.operation.status,
        ProviderTicketOperationStatus::Succeeded
    );
    assert_eq!(
        *linear_client.updates.lock().await,
        vec![("issue-1".to_string(), "done".to_string())]
    );
}
