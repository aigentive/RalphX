//! Per-request enforcement for the Atlassian MCP tool endpoints.
//!
//! These prove the backend gate independently of spawn-time tool filtering:
//! tier fail-closed, integration-unavailable, escape-hatch containment, run
//! authority, and NULL persisted role/project.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use ralphx_lib::application::{AppState, AtlassianIntegrationService, EmptyAtlassianApiClient};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AtlassianMcpAccess, ManualRoleDefault, RoutingRole};
use ralphx_lib::domain::entities::{AgentRun, AgentRunStatus, ChatConversation, IdeationSessionId};
use ralphx_lib::domain::integrations::{
    AtlassianAuthMethod, AtlassianIntegrationSettings, AtlassianIntegrationSettingsRepository,
    IntegrationValidationStatus,
};
use ralphx_lib::http_server::handlers::atlassian_mcp;
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::memory::{
    MemoryAtlassianIntegrationSettingsRepository, MemorySecretStore,
};
use std::sync::Arc;
use tower::ServiceExt;

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
            "/api/atlassian-mcp/confluence/page",
            post(atlassian_mcp::confluence::confluence_get_page),
        )
        .route(
            "/api/atlassian-mcp/request",
            post(atlassian_mcp::raw::atlassian_api_request),
        )
        .with_state(state)
}

fn enabled_settings() -> AtlassianIntegrationSettings {
    AtlassianIntegrationSettings {
        enabled: true,
        auth_method: AtlassianAuthMethod::ApiToken,
        site_url: Some("https://example.atlassian.net".to_string()),
        email: Some("dev@example.com".to_string()),
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
    let mut app_state = AppState::new_test();

    // Replace the integration service so the test controls settings state. The
    // stub client never performs network I/O; these tests assert authorization,
    // not Atlassian responses.
    let settings_repo = Arc::new(MemoryAtlassianIntegrationSettingsRepository::new());
    if let Some(settings) = settings {
        settings_repo
            .upsert(&settings)
            .await
            .expect("settings should persist");
    }
    app_state.atlassian_integration_service = Arc::new(AtlassianIntegrationService::new(
        settings_repo,
        Arc::new(MemorySecretStore::new()),
        Arc::new(EmptyAtlassianApiClient),
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
