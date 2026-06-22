use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::*;
use crate::application::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianCredential,
    AtlassianIntegrationService, AtlassianResourceContent, AtlassianResourceKind,
    AtlassianResourceSummary, LinearApiClient, LinearAuthContext, LinearIntegrationService,
    LinearIssueContent, LinearIssueSummary, LinearLabel, LinearUser,
};
use crate::domain::integrations::{
    AtlassianAuthMethod, ExternalIssueLinkUpsert, ExternalIssueLocalObject,
    ProviderTicketOperationStatus,
};
use crate::domain::services::ComposerIntegrationReference;
use crate::infrastructure::memory::{
    MemoryAtlassianIntegrationSettingsRepository, MemoryExternalIssueLinkRepository,
    MemoryLinearIntegrationSettingsRepository, MemorySecretStore,
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
    async fn validate(
        &self,
        auth: &AtlassianAuthContext,
    ) -> Result<AtlassianConnectivity, String> {
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
        TicketingService::new(atlassian, linear, external_issues).with_event_sink(sink_trait),
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

    assert_eq!(result.operation.status, ProviderTicketOperationStatus::Succeeded);
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

    assert_eq!(result.operation.status, ProviderTicketOperationStatus::Succeeded);
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

    assert_eq!(result.operation.status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(result.operation.operation, ProviderTicketOperationKind::Assign);
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

    assert_eq!(result.operation.status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(result.operation.operation, ProviderTicketOperationKind::Comment);
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

    assert_eq!(result.operation.status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        result.operation.operation,
        ProviderTicketOperationKind::SetLabels
    );
    assert!(!result.idempotent);
    let labels = result.labels.expect("labels payload should be present");
    // Normalized: sorted + deduped.
    assert_eq!(labels.labels, vec!["bug".to_string(), "frontend".to_string()]);
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
