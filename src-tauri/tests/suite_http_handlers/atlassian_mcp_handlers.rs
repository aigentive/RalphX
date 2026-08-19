//! Per-request enforcement for the Atlassian MCP tool endpoints.
//!
//! These prove the backend gate independently of spawn-time tool filtering:
//! tier fail-closed, integration-unavailable, escape-hatch containment, run
//! authority, and NULL persisted role/project.

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use ralphx_lib::application::{
    AppState, AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity,
    AtlassianIntegrationService, AtlassianJiraAttachment, AtlassianResourceContent,
    AtlassianResourceKind, AtlassianResourceSummary, EmptyAtlassianApiClient,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AtlassianMcpAccess, ManualRoleDefault, RoutingRole};
use ralphx_lib::domain::entities::{AgentRun, AgentRunStatus, ChatConversation, IdeationSessionId};
use ralphx_lib::domain::integrations::{
    AtlassianAuthMethod, AtlassianIntegrationSettings, AtlassianIntegrationSettingsRepository,
    IntegrationValidationStatus,
};
use ralphx_lib::domain::services::{ComposerIntegrationReference, SecretStore};
use ralphx_lib::http_server::handlers::atlassian_mcp;
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::memory::{
    MemoryAtlassianIntegrationSettingsRepository, MemorySecretStore,
};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

/// Stub client whose Jira search summaries carry the enriched fields and
/// whose `fetch` returns an attachment with a live download URL, so tests can
/// prove enrichment (1.2) and the redaction boundary (1.4) without a real
/// Atlassian call.
struct EnrichedAtlassianClient;

#[async_trait]
impl AtlassianApiClient for EnrichedAtlassianClient {
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
        kind: AtlassianResourceKind,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, String> {
        Ok(vec![AtlassianResourceSummary {
            kind,
            id: "10001".to_string(),
            key: Some("PROJ-1".to_string()),
            title: "Example issue".to_string(),
            url: Some("https://example.atlassian.net/browse/PROJ-1".to_string()),
            excerpt: None,
            status: Some("In Progress".to_string()),
            issue_type: Some("Bug".to_string()),
            assignee: Some("A. Dev".to_string()),
            updated_at: Some("2026-06-20T10:00:00.000+0000".to_string()),
        }])
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
            title: "Leaky attachment issue".to_string(),
            url: None,
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
            attachments: vec![AtlassianJiraAttachment {
                id: Some("a1".to_string()),
                filename: "leak.png".to_string(),
                mime_type: Some("image/png".to_string()),
                size: Some(10),
                author: None,
                content_url: Some(
                    "https://example.atlassian.net/secure/attachment/a1/leak.png".to_string(),
                ),
                thumbnail_url: Some(
                    "https://example.atlassian.net/secure/thumbnail/a1".to_string(),
                ),
                created_at: None,
            }],
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
        _issue_key: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn list_jira_comments(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
        _start_at: usize,
        _max_results: usize,
    ) -> Result<ralphx_lib::application::JiraCommentsPage, String> {
        Ok(ralphx_lib::application::JiraCommentsPage {
            comments: Vec::new(),
            total: 12,
        })
    }

    async fn list_confluence_spaces(
        &self,
        _auth: &AtlassianAuthContext,
        _limit: usize,
    ) -> Result<Vec<ralphx_lib::application::ConfluenceSpaceSummary>, String> {
        Ok(vec![ralphx_lib::application::ConfluenceSpaceSummary {
            id: "10001".to_string(),
            key: "ENG".to_string(),
            name: "Engineering".to_string(),
        }])
    }

    async fn search_jira_users(
        &self,
        _auth: &AtlassianAuthContext,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<ralphx_lib::application::JiraUserSummary>, String> {
        Ok(vec![ralphx_lib::application::JiraUserSummary {
            account_id: "acc-1".to_string(),
            display_name: "Ada Lovelace".to_string(),
        }])
    }

    async fn exchange_oauth_code(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _code: &str,
        _redirect_uri: &str,
    ) -> Result<ralphx_lib::application::AtlassianOAuthTokenResponse, String> {
        Err("not available in this stub".to_string())
    }

    async fn refresh_oauth_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _refresh_token: &str,
    ) -> Result<ralphx_lib::application::AtlassianOAuthTokenResponse, String> {
        Err("not available in this stub".to_string())
    }

    async fn oauth_accessible_resources(
        &self,
        _access_token: &str,
    ) -> Result<Vec<ralphx_lib::application::AtlassianOAuthResource>, String> {
        Ok(Vec::new())
    }
}

// ============================================================================
// Fixture
// ============================================================================

struct Fixture {
    state: HttpServerState,
    conversation_id: String,
    run_id: String,
}

fn router(state: HttpServerState) -> axum::Router {
    axum::Router::new()
        .route(
            "/api/atlassian-mcp/jira/search",
            post(atlassian_mcp::jira::jira_search_issues),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/create",
            post(atlassian_mcp::jira::jira_create_issue),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/comment",
            post(atlassian_mcp::jira::jira_add_comment),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/comments",
            post(atlassian_mcp::jira::jira_list_comments),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/assign",
            post(atlassian_mcp::jira::jira_assign_issue),
        )
        .route(
            "/api/atlassian-mcp/jira/users/search",
            post(atlassian_mcp::jira::jira_search_users),
        )
        .route(
            "/api/atlassian-mcp/jira/issue",
            post(atlassian_mcp::jira::jira_get_issue),
        )
        .route(
            "/api/atlassian-mcp/jira/agile/boards",
            post(atlassian_mcp::agile::jira_list_boards),
        )
        .route(
            "/api/atlassian-mcp/jira/agile/sprints",
            post(atlassian_mcp::agile::jira_list_sprints),
        )
        .route(
            "/api/atlassian-mcp/jira/agile/sprint/issues",
            post(atlassian_mcp::agile::jira_get_sprint_issues),
        )
        .route(
            "/api/atlassian-mcp/confluence/search",
            post(atlassian_mcp::confluence::confluence_search_pages),
        )
        .route(
            "/api/atlassian-mcp/confluence/spaces",
            post(atlassian_mcp::confluence::confluence_list_spaces),
        )
        .route(
            "/api/atlassian-mcp/confluence/page",
            post(atlassian_mcp::confluence::confluence_get_page),
        )
        .route(
            "/api/atlassian-mcp/confluence/page/create",
            post(atlassian_mcp::confluence::confluence_create_page),
        )
        .route(
            "/api/atlassian-mcp/confluence/page/update",
            post(atlassian_mcp::confluence::confluence_update_page),
        )
        .route(
            "/api/atlassian-mcp/request",
            post(atlassian_mcp::raw::atlassian_api_request),
        )
        .with_state(state)
}

/// Seeded into the fixture's secret store so `enabled_settings()` can actually
/// build an auth context; without it every enabled-integration request fails
/// closed at `load_token` with a 400 before reaching the stub client.
const TEST_TOKEN_SECRET_REF: &str = "atlassian-api-token-test";

fn enabled_settings() -> AtlassianIntegrationSettings {
    AtlassianIntegrationSettings {
        enabled: true,
        auth_method: AtlassianAuthMethod::ApiToken,
        site_url: Some("https://example.atlassian.net".to_string()),
        email: Some("dev@example.com".to_string()),
        token_secret_ref: Some(TEST_TOKEN_SECRET_REF.to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        ..AtlassianIntegrationSettings::default()
    }
}

/// Build a fixture with a live caller run carrying the given persisted identity.
async fn fixture_with(
    routing_role: Option<RoutingRole>,
    project_id: Option<&str>,
    settings: Option<AtlassianIntegrationSettings>,
) -> Fixture {
    fixture_with_client(
        routing_role,
        project_id,
        settings,
        Arc::new(EmptyAtlassianApiClient),
    )
    .await
}

/// Same as [`fixture_with`], but lets a test control what the Atlassian
/// client returns (for example an attachment carrying a download URL, to
/// prove the redaction boundary).
async fn fixture_with_client(
    routing_role: Option<RoutingRole>,
    project_id: Option<&str>,
    settings: Option<AtlassianIntegrationSettings>,
    client: Arc<dyn AtlassianApiClient>,
) -> Fixture {
    let mut app_state = AppState::new_test();

    // Replace the integration service so the test controls settings state. The
    // stub client never performs network I/O; these tests assert authorization
    // and handler-to-client pass-through, not real Atlassian responses.
    let settings_repo = Arc::new(MemoryAtlassianIntegrationSettingsRepository::new());
    if let Some(settings) = settings {
        settings_repo
            .upsert(&settings)
            .await
            .expect("settings should persist");
    }
    let secret_store = Arc::new(MemorySecretStore::new());
    secret_store
        .put_secret(TEST_TOKEN_SECRET_REF, "test-api-token")
        .await
        .expect("token should persist");
    app_state.atlassian_integration_service = Arc::new(AtlassianIntegrationService::new(
        settings_repo,
        secret_store,
        client,
    ));

    let conversation = ChatConversation::new_ideation(IdeationSessionId::new());
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");

    let mut run = AgentRun::new(conversation_id);
    run.status = AgentRunStatus::Running;
    run.routing_role = routing_role;
    run.project_id = project_id.map(str::to_string);
    let run_id = run.id.as_str().to_string();
    app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("run should persist");

    Fixture {
        state: HttpServerState {
            app_state: Arc::new(app_state),
            execution_state: Arc::new(ExecutionState::new()),
            delegation_service: Default::default(),
            external_mcp_supervisor: None,
        },
        conversation_id: conversation_id.as_str().to_string(),
        run_id,
    }
}

impl Fixture {
    async fn post(&self, path: &str, body: serde_json::Value) -> StatusCode {
        self.post_with_identity(path, body, Some((&self.conversation_id, &self.run_id)))
            .await
    }

    async fn post_with_identity(
        &self,
        path: &str,
        body: serde_json::Value,
        identity: Option<(&str, &str)>,
    ) -> StatusCode {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some((conversation_id, run_id)) = identity {
            request = request
                .header("x-ralphx-conversation-id", conversation_id)
                .header("x-ralphx-agent-run-id", run_id);
        }
        let response = router(self.state.clone())
            .oneshot(
                request
                    .body(Body::from(body.to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");
        response.status()
    }

    /// Like [`Fixture::post`], but also returns the parsed JSON response body
    /// for tests that assert on response shape, not just status.
    async fn post_json(&self, path: &str, body: serde_json::Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .header("x-ralphx-conversation-id", &self.conversation_id)
            .header("x-ralphx-agent-run-id", &self.run_id)
            .body(Body::from(body.to_string()))
            .expect("request should build");
        let response = router(self.state.clone())
            .oneshot(request)
            .await
            .expect("router should respond");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }
}

fn search_body() -> serde_json::Value {
    serde_json::json!({ "query": "project = PROJ" })
}

fn create_issue_body() -> serde_json::Value {
    serde_json::json!({
        "projectKey": "PROJ",
        "issueType": "Task",
        "summary": "From an agent"
    })
}

// ============================================================================
// Tier fail-closed
// ============================================================================

#[tokio::test]
async fn read_tier_run_is_denied_a_write_endpoint() {
    // WorkspaceReviewer defaults to read.
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceReviewer),
        None,
        Some(enabled_settings()),
    )
    .await;

    assert_eq!(
        fixture
            .post("/api/atlassian-mcp/jira/issue/create", create_issue_body())
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/issue/comment",
                serde_json::json!({ "issueKey": "PROJ-1", "body": "hi" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn read_tier_run_passes_the_authorization_gate_on_a_read_endpoint() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceReviewer),
        None,
        Some(enabled_settings()),
    )
    .await;

    // The stub client cannot serve the call, but authorization must not be the
    // reason: a 403 here would mean the read tier was refused a read endpoint.
    assert_ne!(
        fixture
            .post("/api/atlassian-mcp/jira/search", search_body())
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn agile_tools_pass_the_authorization_gate_at_the_read_tier() {
    // WorkspaceReviewer defaults to read; a stub-client failure here would
    // still not be a 403, so `!= FORBIDDEN` isolates authorization.
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceReviewer),
        None,
        Some(enabled_settings()),
    )
    .await;

    assert_ne!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/agile/boards",
                serde_json::json!({})
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_ne!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/agile/sprints",
                serde_json::json!({ "boardId": "41" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_ne!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/agile/sprint/issues",
                serde_json::json!({ "sprintId": "91" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn agile_tools_are_denied_below_the_read_tier() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceReviewer),
        Some("project-1"),
        Some(enabled_settings()),
    )
    .await;

    let mut value = ManualRoleDefault::from_legacy(
        &ralphx_lib::domain::agents::standard_agent_lane_defaults()
            .values()
            .next()
            .cloned()
            .expect("a lane default exists"),
    );
    value.atlassian_access = Some(AtlassianMcpAccess::None);
    fixture
        .state
        .app_state
        .manual_role_default_repo
        .upsert_for_project("project-1", RoutingRole::WorkspaceReviewer, &value)
        .await
        .expect("override should persist");

    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/agile/boards",
                serde_json::json!({})
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/agile/sprints",
                serde_json::json!({ "boardId": "41" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/agile/sprint/issues",
                serde_json::json!({ "sprintId": "91" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_disabled_integration_is_reported_separately_from_a_denied_role() {
    let write_role_but_integration_off =
        fixture_with(Some(RoutingRole::WorkspaceEdit), None, None).await;

    assert_eq!(
        write_role_but_integration_off
            .post("/api/atlassian-mcp/jira/search", search_body())
            .await,
        StatusCode::FAILED_DEPENDENCY
    );
}

#[tokio::test]
async fn an_enabled_but_unvalidated_integration_denies_every_endpoint() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(AtlassianIntegrationSettings {
            validation_status: IntegrationValidationStatus::Invalid,
            ..enabled_settings()
        }),
    )
    .await;

    assert_eq!(
        fixture
            .post("/api/atlassian-mcp/jira/search", search_body())
            .await,
        StatusCode::FAILED_DEPENDENCY
    );
}

// ============================================================================
// Run authority
// ============================================================================

#[tokio::test]
async fn a_run_without_a_persisted_routing_role_is_denied() {
    // Pre-migration runs read back NULL and must fail closed.
    let fixture = fixture_with(None, Some("project-1"), Some(enabled_settings())).await;

    assert_eq!(
        fixture
            .post("/api/atlassian-mcp/jira/search", search_body())
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn missing_transport_identity_is_rejected_before_authorization() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    assert_eq!(
        fixture
            .post_with_identity("/api/atlassian-mcp/jira/search", search_body(), None)
            .await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn an_unknown_caller_run_is_rejected() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    assert_eq!(
        fixture
            .post_with_identity(
                "/api/atlassian-mcp/jira/search",
                search_body(),
                Some((&fixture.conversation_id, "run-that-does-not-exist"))
            )
            .await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_terminal_caller_run_loses_authority() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;
    let run_id = ralphx_lib::domain::entities::AgentRunId::from_string(fixture.run_id.clone());
    fixture
        .state
        .app_state
        .agent_run_repo
        .complete(&run_id)
        .await
        .expect("run should complete");

    assert_eq!(
        fixture
            .post("/api/atlassian-mcp/jira/search", search_body())
            .await,
        StatusCode::UNAUTHORIZED
    );
}

// ============================================================================
// Role overrides
// ============================================================================

#[tokio::test]
async fn a_project_override_of_none_denies_a_role_that_would_otherwise_be_granted() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        Some("project-1"),
        Some(enabled_settings()),
    )
    .await;

    let mut value = ManualRoleDefault::from_legacy(
        &ralphx_lib::domain::agents::standard_agent_lane_defaults()
            .values()
            .next()
            .cloned()
            .expect("a lane default exists"),
    );
    value.atlassian_access = Some(AtlassianMcpAccess::None);
    fixture
        .state
        .app_state
        .manual_role_default_repo
        .upsert_for_project("project-1", RoutingRole::WorkspaceEdit, &value)
        .await
        .expect("override should persist");

    assert_eq!(
        fixture
            .post("/api/atlassian-mcp/jira/search", search_body())
            .await,
        StatusCode::FORBIDDEN
    );
}

// ============================================================================
// Escape-hatch containment
// ============================================================================

#[tokio::test]
async fn the_escape_hatch_rejects_unsafe_paths() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    for path in [
        "https://evil.example.com/rest/api/3/issue",
        "//evil.example.com/rest/api/3/issue",
        "/rest/api/../../secrets",
        "/plugins/servlet/admin",
        "rest/api/3/issue",
    ] {
        assert_eq!(
            fixture
                .post(
                    "/api/atlassian-mcp/request",
                    serde_json::json!({ "method": "GET", "product": "jira", "path": path })
                )
                .await,
            StatusCode::BAD_REQUEST,
            "{path} must be rejected"
        );
    }
}

#[tokio::test]
async fn the_escape_hatch_gates_mutating_methods_on_the_write_tier() {
    let read_only = fixture_with(
        Some(RoutingRole::WorkspaceReviewer),
        None,
        Some(enabled_settings()),
    )
    .await;

    // GET is allowed at the read tier: whatever happens next, it is not a 403.
    assert_ne!(
        read_only
            .post(
                "/api/atlassian-mcp/request",
                serde_json::json!({
                    "method": "GET",
                    "product": "jira",
                    "path": "/rest/agile/1.0/board/5/sprint"
                })
            )
            .await,
        StatusCode::FORBIDDEN
    );

    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        assert_eq!(
            read_only
                .post(
                    "/api/atlassian-mcp/request",
                    serde_json::json!({
                        "method": method,
                        "product": "jira",
                        "path": "/rest/api/3/issue"
                    })
                )
                .await,
            StatusCode::FORBIDDEN,
            "{method} must require the write tier"
        );
    }
}

#[tokio::test]
async fn the_escape_hatch_rejects_unsupported_methods_and_products() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/request",
                serde_json::json!({ "method": "TRACE", "product": "jira", "path": "/rest/api/3/x" })
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/request",
                serde_json::json!({ "method": "GET", "product": "bitbucket", "path": "/rest/api/3/x" })
            )
            .await,
        StatusCode::BAD_REQUEST
    );
}

// ============================================================================
// Request validation
// ============================================================================

#[tokio::test]
async fn blank_required_fields_are_rejected_before_any_atlassian_call() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/confluence/page",
                serde_json::json!({ "pageId": "   " })
            )
            .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/issue/create",
                serde_json::json!({ "projectKey": "  ", "issueType": "Task", "summary": "x" })
            )
            .await,
        StatusCode::BAD_REQUEST
    );
}

// ============================================================================
// Phase 1: raw JQL/CQL pass-through, enriched summaries, attachment redaction
// ============================================================================

#[tokio::test]
async fn a_jql_true_search_request_is_accepted_and_reaches_the_raw_search_path() {
    // EmptyAtlassianApiClient does not override search_raw, so the default
    // trait implementation answers with a status-less "not available" error,
    // mapped through Api (BAD_GATEWAY) rather than InvalidRequest (BAD_REQUEST).
    // A BAD_REQUEST here would mean the jql flag never reached the raw path.
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    let status = fixture
        .post(
            "/api/atlassian-mcp/jira/search",
            serde_json::json!({ "query": "project = ENG AND status = \"In Progress\"", "jql": true }),
        )
        .await;

    assert_ne!(status, StatusCode::BAD_REQUEST);
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn a_cql_true_search_request_is_accepted_and_reaches_the_raw_search_path() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    let status = fixture
        .post(
            "/api/atlassian-mcp/confluence/search",
            serde_json::json!({ "query": "type = page AND text ~ \"runbook\"", "cql": true }),
        )
        .await;

    assert_ne!(status, StatusCode::BAD_REQUEST);
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn a_default_mode_search_request_is_unaffected_by_the_raw_mode_addition() {
    // Regression: omitting jql/cql must keep hitting the existing smart-mode
    // path. EmptyAtlassianApiClient's smart-mode `search()` answers
    // `Ok(Vec::new())`, so 200 here proves the raw-mode addition did not
    // change default-mode routing.
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    let status = fixture
        .post("/api/atlassian-mcp/jira/search", search_body())
        .await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn jira_search_summaries_carry_status_type_and_assignee() {
    let fixture = fixture_with_client(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
        Arc::new(EnrichedAtlassianClient),
    )
    .await;

    let (status, body) = fixture
        .post_json("/api/atlassian-mcp/jira/search", search_body())
        .await;

    assert_eq!(status, StatusCode::OK);
    let issue = &body["issues"][0];
    assert_eq!(issue["status"], "In Progress");
    assert_eq!(issue["issueType"], "Bug");
    assert_eq!(issue["assignee"], "A. Dev");
    assert_eq!(issue["updatedAt"], "2026-06-20T10:00:00.000+0000");
}

#[tokio::test]
async fn jira_get_issue_response_has_no_content_or_thumbnail_url() {
    // Regression for the pre-existing leak: AtlassianResourceContent keeps
    // content_url/thumbnail_url for the Tauri/UI path, but the MCP response
    // boundary must strip them before any read-tier agent can see them.
    let fixture = fixture_with_client(
        Some(RoutingRole::WorkspaceReviewer),
        None,
        Some(enabled_settings()),
        Arc::new(EnrichedAtlassianClient),
    )
    .await;

    let (status, body) = fixture
        .post_json(
            "/api/atlassian-mcp/jira/issue",
            serde_json::json!({ "issueKey": "PROJ-1" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    let attachments = body["issue"]["attachments"]
        .as_array()
        .expect("attachments array");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0]["filename"], "leak.png");
    assert!(attachments[0].get("contentUrl").is_none());
    assert!(attachments[0].get("thumbnailUrl").is_none());
}

// ============================================================================
// Phase 4.1b: Confluence bodyStorage/bodyMarkdown exactly-one-of validation
// ============================================================================

fn confluence_create_body(fields: serde_json::Value) -> serde_json::Value {
    let mut base = serde_json::json!({ "spaceId": "789", "title": "Runbook" });
    if let (Some(base_map), Some(fields_map)) = (base.as_object_mut(), fields.as_object()) {
        for (key, value) in fields_map {
            base_map.insert(key.clone(), value.clone());
        }
    }
    base
}

#[tokio::test]
async fn confluence_create_page_accepts_exactly_one_of_body_storage_or_body_markdown() {
    // EmptyAtlassianApiClient does not override confluence_create_page, so a
    // validated request reaches the default trait impl's status-less "not
    // available" error (BAD_GATEWAY) rather than InvalidRequest (BAD_REQUEST).
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    let only_storage = fixture
        .post(
            "/api/atlassian-mcp/confluence/page/create",
            confluence_create_body(serde_json::json!({ "bodyStorage": "<p>hi</p>" })),
        )
        .await;
    assert_eq!(
        only_storage,
        StatusCode::BAD_GATEWAY,
        "storage only should pass validation"
    );

    let only_markdown = fixture
        .post(
            "/api/atlassian-mcp/confluence/page/create",
            confluence_create_body(serde_json::json!({ "bodyMarkdown": "# hi" })),
        )
        .await;
    assert_eq!(
        only_markdown,
        StatusCode::BAD_GATEWAY,
        "markdown only should pass validation"
    );

    let neither = fixture
        .post(
            "/api/atlassian-mcp/confluence/page/create",
            confluence_create_body(serde_json::json!({})),
        )
        .await;
    assert_eq!(neither, StatusCode::BAD_REQUEST, "neither must be rejected");

    let both = fixture
        .post(
            "/api/atlassian-mcp/confluence/page/create",
            confluence_create_body(
                serde_json::json!({ "bodyStorage": "<p>hi</p>", "bodyMarkdown": "# hi" }),
            ),
        )
        .await;
    assert_eq!(both, StatusCode::BAD_REQUEST, "both must be rejected");
}

#[tokio::test]
async fn confluence_update_page_rejects_both_body_fields_but_allows_neither() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
    )
    .await;

    let both = fixture
        .post(
            "/api/atlassian-mcp/confluence/page/update",
            serde_json::json!({
                "pageId": "12345",
                "bodyStorage": "<p>hi</p>",
                "bodyMarkdown": "# hi"
            }),
        )
        .await;
    assert_eq!(both, StatusCode::BAD_REQUEST, "both must be rejected");

    // Title-only update (neither body field supplied) is a pre-existing,
    // still-supported shape: the empty-patch guard is satisfied by title
    // alone, and omitting both body fields means "leave the body unchanged".
    let title_only = fixture
        .post(
            "/api/atlassian-mcp/confluence/page/update",
            serde_json::json!({ "pageId": "12345", "title": "Renamed" }),
        )
        .await;
    assert_eq!(
        title_only,
        StatusCode::BAD_GATEWAY,
        "title-only update should pass validation and reach the stub client"
    );
}

// ============================================================================
// Phase 4.2 — discovery tools
// ============================================================================

#[tokio::test]
async fn discovery_tools_pass_the_authorization_gate_at_the_read_tier() {
    // WorkspaceReviewer defaults to read; a stub-client failure here would
    // still not be a 403, so `!= FORBIDDEN` isolates authorization.
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceReviewer),
        None,
        Some(enabled_settings()),
    )
    .await;

    assert_ne!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/issue/comments",
                serde_json::json!({ "issueKey": "PROJ-1" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_ne!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/users/search",
                serde_json::json!({ "query": "ada" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_ne!(
        fixture
            .post(
                "/api/atlassian-mcp/confluence/spaces",
                serde_json::json!({})
            )
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn discovery_tools_are_denied_below_the_read_tier() {
    let fixture = fixture_with(
        Some(RoutingRole::WorkspaceReviewer),
        Some("project-1"),
        Some(enabled_settings()),
    )
    .await;

    let mut value = ManualRoleDefault::from_legacy(
        &ralphx_lib::domain::agents::standard_agent_lane_defaults()
            .values()
            .next()
            .cloned()
            .expect("a lane default exists"),
    );
    value.atlassian_access = Some(AtlassianMcpAccess::None);
    fixture
        .state
        .app_state
        .manual_role_default_repo
        .upsert_for_project("project-1", RoutingRole::WorkspaceReviewer, &value)
        .await
        .expect("override should persist");

    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/issue/comments",
                serde_json::json!({ "issueKey": "PROJ-1" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/jira/users/search",
                serde_json::json!({ "query": "ada" })
            )
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture
            .post(
                "/api/atlassian-mcp/confluence/spaces",
                serde_json::json!({})
            )
            .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn jira_list_comments_returns_the_bodies_and_true_total() {
    let fixture = fixture_with_client(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
        Arc::new(EnrichedAtlassianClient),
    )
    .await;

    let (status, body) = fixture
        .post_json(
            "/api/atlassian-mcp/jira/issue/comments",
            serde_json::json!({ "issueKey": "PROJ-1" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 12);
    assert!(body["comments"].as_array().is_some());
}

#[tokio::test]
async fn confluence_list_spaces_returns_id_key_name() {
    let fixture = fixture_with_client(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
        Arc::new(EnrichedAtlassianClient),
    )
    .await;

    let (status, body) = fixture
        .post_json(
            "/api/atlassian-mcp/confluence/spaces",
            serde_json::json!({}),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    let spaces = body["spaces"].as_array().expect("spaces array");
    assert_eq!(spaces.len(), 1);
    assert_eq!(spaces[0]["id"], "10001");
    assert_eq!(spaces[0]["key"], "ENG");
    assert_eq!(spaces[0]["name"], "Engineering");
}

#[tokio::test]
async fn jira_search_users_returns_account_id_and_display_name() {
    let fixture = fixture_with_client(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
        Arc::new(EnrichedAtlassianClient),
    )
    .await;

    let (status, body) = fixture
        .post_json(
            "/api/atlassian-mcp/jira/users/search",
            serde_json::json!({ "query": "ada" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    let users = body["users"].as_array().expect("users array");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0]["accountId"], "acc-1");
    assert_eq!(users[0]["displayName"], "Ada Lovelace");
}

/// Records which underlying assign path `jira_assign_issue` actually calls,
/// to prove the accountId > assignToMe > clear precedence end-to-end through
/// the handler instead of just unit-testing the handler's own match arms.
struct RecordingAssignClient {
    calls: std::sync::Mutex<Vec<String>>,
}

impl RecordingAssignClient {
    fn new() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl AtlassianApiClient for RecordingAssignClient {
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
        Err(format!("not implemented for {}", reference.id))
    }

    async fn assign_jira_issue_to_current_user(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .expect("calls")
            .push("assign_to_me".to_string());
        Ok(())
    }

    async fn clear_jira_issue_assignee(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
    ) -> Result<(), String> {
        self.calls.lock().expect("calls").push("clear".to_string());
        Ok(())
    }

    async fn assign_jira_issue_to_account(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
        account_id: &str,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("account:{account_id}"));
        Ok(())
    }

    async fn exchange_oauth_code(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _code: &str,
        _redirect_uri: &str,
    ) -> Result<ralphx_lib::application::AtlassianOAuthTokenResponse, String> {
        Err("not available in this stub".to_string())
    }

    async fn refresh_oauth_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _refresh_token: &str,
    ) -> Result<ralphx_lib::application::AtlassianOAuthTokenResponse, String> {
        Err("not available in this stub".to_string())
    }

    async fn oauth_accessible_resources(
        &self,
        _access_token: &str,
    ) -> Result<Vec<ralphx_lib::application::AtlassianOAuthResource>, String> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn jira_assign_issue_prefers_account_id_over_assign_to_me() {
    let client = Arc::new(RecordingAssignClient::new());
    let fixture = fixture_with_client(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
        client.clone(),
    )
    .await;

    let status = fixture
        .post(
            "/api/atlassian-mcp/jira/issue/assign",
            serde_json::json!({
                "issueKey": "PROJ-1",
                "accountId": "account-9",
                "assignToMe": true
            }),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        client.calls.lock().expect("calls").as_slice(),
        ["account:account-9"]
    );
}

#[tokio::test]
async fn jira_assign_issue_falls_back_to_assign_to_me_without_account_id() {
    let client = Arc::new(RecordingAssignClient::new());
    let fixture = fixture_with_client(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
        client.clone(),
    )
    .await;

    let status = fixture
        .post(
            "/api/atlassian-mcp/jira/issue/assign",
            serde_json::json!({ "issueKey": "PROJ-1", "assignToMe": true }),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        client.calls.lock().expect("calls").as_slice(),
        ["assign_to_me"]
    );
}

#[tokio::test]
async fn jira_assign_issue_clears_when_neither_account_id_nor_assign_to_me_is_set() {
    let client = Arc::new(RecordingAssignClient::new());
    let fixture = fixture_with_client(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some(enabled_settings()),
        client.clone(),
    )
    .await;

    let status = fixture
        .post(
            "/api/atlassian-mcp/jira/issue/assign",
            serde_json::json!({ "issueKey": "PROJ-1" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(client.calls.lock().expect("calls").as_slice(), ["clear"]);
}
