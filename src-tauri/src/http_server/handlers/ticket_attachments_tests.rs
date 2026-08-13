use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use super::super::types::HttpServerState;
use super::ticket_attachments::{
    fetch_response, fetch_response_with_inline_text, fetch_ticket_attachment_http,
    list_ticket_attachments_http, provider_item, safe_attachment_id, safe_file_name,
    AttachmentFetchTarget, TicketAttachmentFetchRequest, TicketAttachmentListRequest,
    TICKET_ATTACHMENT_CANONICAL_GRANTEES,
};
use crate::application::ticket_attachment::{
    build_ticket_attachment_content_location, TicketAttachmentDescriptor,
    TicketAttachmentFetchResult, TicketAttachmentProvider, TicketAttachmentSourceHandle,
};
use crate::application::{AppState, AtlassianIntegrationService, EmptyAtlassianApiClient};
use crate::domain::agents::{AtlassianMcpAccess, ManualRoleDefault, RoutingRole};
use crate::domain::entities::{AgentRun, AgentRunId, AgentRunStatus, ChatConversation, IdeationSessionId};
use crate::domain::integrations::{
    AtlassianAuthMethod, AtlassianIntegrationSettings, AtlassianIntegrationSettingsRepository,
    IntegrationValidationStatus,
};
use crate::infrastructure::memory::{MemoryAtlassianIntegrationSettingsRepository, MemorySecretStore};

/// Fixture carrying a trusted, live caller run bound to a conversation, with
/// the identity headers `authorize()` and the ticket-attachment caller check
/// both read.
struct Fixture {
    state: HttpServerState,
    conversation_id: String,
    run_id: String,
}

fn enabled_jira_settings() -> AtlassianIntegrationSettings {
    AtlassianIntegrationSettings {
        enabled: true,
        auth_method: AtlassianAuthMethod::ApiToken,
        site_url: Some("https://example.atlassian.net".to_string()),
        email: Some("dev@example.com".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        ..AtlassianIntegrationSettings::default()
    }
}

/// Build a fixture with a live caller run carrying the given persisted
/// identity, mirroring `suite_http_handlers/atlassian_mcp_handlers.rs`.
async fn fixture_with(
    routing_role: Option<RoutingRole>,
    settings: Option<AtlassianIntegrationSettings>,
) -> Fixture {
    fixture_with_agent_name(routing_role, settings, None).await
}

/// Same as [`fixture_with`], but also sets the caller run's `agent_name` —
/// the identity the Linear/ClickUp canonical-grant check reads.
async fn fixture_with_agent_name(
    routing_role: Option<RoutingRole>,
    settings: Option<AtlassianIntegrationSettings>,
    agent_name: Option<&str>,
) -> Fixture {
    let mut app_state = AppState::new_test();

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
    run.agent_name = agent_name.map(str::to_string);
    let run_id = run.id.as_str().to_string();
    app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("run should persist");

    Fixture {
        state: HttpServerState::new_test(Arc::new(app_state)),
        conversation_id: conversation_id.as_str().to_string(),
        run_id,
    }
}

impl Fixture {
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-ralphx-conversation-id",
            self.conversation_id.parse().expect("valid header value"),
        );
        headers.insert(
            "x-ralphx-agent-run-id",
            self.run_id.parse().expect("valid header value"),
        );
        headers
    }

    async fn deny_atlassian_access_for(&self, role: RoutingRole) {
        let mut value = ManualRoleDefault::from_legacy(
            &crate::domain::agents::standard_agent_lane_defaults()
                .values()
                .next()
                .cloned()
                .expect("a lane default exists"),
        );
        value.atlassian_access = Some(AtlassianMcpAccess::None);
        self.state
            .app_state
            .manual_role_default_repo
            .upsert_global(role, &value)
            .await
            .expect("override should persist");
    }
}

fn list_request(provider: TicketAttachmentProvider) -> TicketAttachmentListRequest {
    TicketAttachmentListRequest {
        provider,
        ticket_id: "TICKET-1".to_string(),
    }
}

fn fetch_request(provider: TicketAttachmentProvider) -> TicketAttachmentFetchRequest {
    TicketAttachmentFetchRequest {
        provider,
        ticket_id: "TICKET-1".to_string(),
        content_pointer: "ta_0000000000000000000000".to_string(),
    }
}

#[tokio::test]
async fn list_ticket_attachments_fails_closed_without_leaking_provider_details() {
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), Some(enabled_jira_settings())).await;
    let request = list_request(TicketAttachmentProvider::Jira);

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(request),
    )
    .await
    .expect_err("stub provider client should fail closed");
    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error response body should be readable");
    let body = std::str::from_utf8(&body).expect("error response should be utf8");
    assert!(body.contains("ticket_attachment_provider_failed"));
    assert!(!body.contains("http://"));
    assert!(!body.contains("https://"));
    assert!(!body.to_ascii_lowercase().contains("token"));
}

#[tokio::test]
async fn fetch_ticket_attachment_rejects_direct_download_pointer_before_provider_lookup() {
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), Some(enabled_jira_settings())).await;
    let request = TicketAttachmentFetchRequest {
        provider: TicketAttachmentProvider::Jira,
        ticket_id: "JIRA-123".to_string(),
        content_pointer: "https://example.test/download?token=secret".to_string(),
    };

    let error = fetch_ticket_attachment_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(request),
    )
    .await
    .expect_err("direct download URL should not be accepted as a pointer");
    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error response body should be readable");
    let body = std::str::from_utf8(&body).expect("error response should be utf8");
    assert!(body.contains("invalid_ticket_attachment_request"));
    assert!(!body.contains("https://example.test"));
    assert!(!body.contains("token=secret"));
}

// ============================================================================
// Backend authorization (STEP A) — denied paths
// ============================================================================

#[tokio::test]
async fn list_ticket_attachments_jira_rejects_missing_caller_identity() {
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), Some(enabled_jira_settings())).await;

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        HeaderMap::new(),
        axum::Json(list_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("missing identity headers should be rejected");

    assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn fetch_ticket_attachment_jira_rejects_missing_caller_identity() {
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), Some(enabled_jira_settings())).await;

    let error = fetch_ticket_attachment_http(
        State(fixture.state.clone()),
        HeaderMap::new(),
        axum::Json(fetch_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("missing identity headers should be rejected");

    assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_ticket_attachments_jira_rejects_terminal_caller_run() {
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), Some(enabled_jira_settings())).await;
    let run_id = AgentRunId::from_string(fixture.run_id.clone());
    fixture
        .state
        .app_state
        .agent_run_repo
        .complete(&run_id)
        .await
        .expect("run should complete");

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(list_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("a terminal run should lose caller authority");

    assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_ticket_attachments_jira_rejects_null_routing_role() {
    // Pre-migration runs read back NULL and must fail closed.
    let fixture = fixture_with(None, Some(enabled_jira_settings())).await;

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(list_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("a run without a persisted routing role should be denied");

    assert_eq!(error.into_response().status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn fetch_ticket_attachment_jira_rejects_terminal_caller_run() {
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), Some(enabled_jira_settings())).await;
    let run_id = AgentRunId::from_string(fixture.run_id.clone());
    fixture
        .state
        .app_state
        .agent_run_repo
        .complete(&run_id)
        .await
        .expect("run should complete");

    let error = fetch_ticket_attachment_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(fetch_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("a terminal run should lose caller authority");

    assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn fetch_ticket_attachment_jira_rejects_null_routing_role() {
    // Pre-migration runs read back NULL and must fail closed.
    let fixture = fixture_with(None, Some(enabled_jira_settings())).await;

    let error = fetch_ticket_attachment_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(fetch_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("a run without a persisted routing role should be denied");

    assert_eq!(error.into_response().status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_ticket_attachments_jira_rejects_none_tier_role() {
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), Some(enabled_jira_settings())).await;
    fixture
        .deny_atlassian_access_for(RoutingRole::WorkspaceEdit)
        .await;

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(list_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("a none-tier role should be denied");

    assert_eq!(error.into_response().status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn fetch_ticket_attachment_jira_rejects_none_tier_role() {
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), Some(enabled_jira_settings())).await;
    fixture
        .deny_atlassian_access_for(RoutingRole::WorkspaceEdit)
        .await;

    let error = fetch_ticket_attachment_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(fetch_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("a none-tier role should be denied");

    assert_eq!(error.into_response().status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_ticket_attachments_linear_rejects_missing_caller_identity() {
    // Linear is not part of the Atlassian tier system, but the backend must
    // never fall through open: it still requires a trusted, live caller run.
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), None).await;

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        HeaderMap::new(),
        axum::Json(list_request(TicketAttachmentProvider::Linear)),
    )
    .await
    .expect_err("missing identity headers should be rejected even for Linear");

    assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_ticket_attachments_linear_allows_trusted_canonical_grantee_without_atlassian_tier() {
    // No Atlassian integration is configured and the role has no Atlassian
    // access at all, yet a trusted caller run bound to a canonical grantee
    // (worker/coder) must still reach the Linear provider call: Linear
    // authorization is the canonical MCP grant, not the Atlassian tier gate.
    let fixture = fixture_with_agent_name(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some("ralphx-execution-worker"),
    )
    .await;

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(list_request(TicketAttachmentProvider::Linear)),
    )
    .await
    .expect_err("the memory Linear integration has nothing to return");

    let status = error.into_response().status();
    assert_ne!(status, StatusCode::UNAUTHORIZED);
    assert_ne!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_ticket_attachments_linear_rejects_non_grantee_read_tier_agent() {
    // A Read-tier role that is not one of the two canonical grantees (for
    // example a workspace chat agent) must not reach Linear attachments,
    // even with a live, correctly bound caller run.
    let fixture = fixture_with_agent_name(
        Some(RoutingRole::WorkspaceEdit),
        None,
        Some("ralphx-workspace-chat"),
    )
    .await;

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(list_request(TicketAttachmentProvider::Linear)),
    )
    .await
    .expect_err("a non-grantee agent should be rejected for Linear");

    assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn fetch_ticket_attachment_jira_ignores_grantee_list_and_gates_only_by_tier() {
    // The same non-grantee agent_name that Linear/ClickUp reject must still
    // reach the Jira provider call: Jira authorization stays purely
    // tier-driven, independent of the canonical grantee list.
    let fixture = fixture_with_agent_name(
        Some(RoutingRole::WorkspaceEdit),
        Some(enabled_jira_settings()),
        Some("ralphx-workspace-chat"),
    )
    .await;

    let error = fetch_ticket_attachment_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(fetch_request(TicketAttachmentProvider::Jira)),
    )
    .await
    .expect_err("the stub provider client should fail closed, not the authorization gate");

    assert_eq!(
        error.into_response().status(),
        StatusCode::BAD_GATEWAY,
        "jira authorization must pass for a Read-tier caller regardless of agent_name"
    );
}

#[tokio::test]
async fn worker_and_coder_runs_pass_authorization_on_both_jira_and_linear() {
    for grantee in TICKET_ATTACHMENT_CANONICAL_GRANTEES {
        let jira_fixture = fixture_with_agent_name(
            Some(RoutingRole::WorkspaceEdit),
            Some(enabled_jira_settings()),
            Some(grantee),
        )
        .await;
        let jira_error = fetch_ticket_attachment_http(
            State(jira_fixture.state.clone()),
            jira_fixture.headers(),
            axum::Json(fetch_request(TicketAttachmentProvider::Jira)),
        )
        .await
        .expect_err("stub provider client should fail closed after authorization passes");
        assert_eq!(
            jira_error.into_response().status(),
            StatusCode::BAD_GATEWAY,
            "grantee {grantee} should pass Jira authorization"
        );

        let linear_fixture =
            fixture_with_agent_name(Some(RoutingRole::WorkspaceEdit), None, Some(grantee)).await;
        let linear_error = list_ticket_attachments_http(
            State(linear_fixture.state.clone()),
            linear_fixture.headers(),
            axum::Json(list_request(TicketAttachmentProvider::Linear)),
        )
        .await
        .expect_err("the memory Linear integration has nothing to return");
        let linear_status = linear_error.into_response().status();
        assert_ne!(
            linear_status,
            StatusCode::UNAUTHORIZED,
            "grantee {grantee} should pass Linear authorization"
        );
        assert_ne!(linear_status, StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn list_ticket_attachments_linear_rejects_run_with_no_agent_name() {
    // A live, correctly bound caller run with `agent_name: None` (for
    // example a pre-persistence run) must fail closed rather than being
    // treated as trusted by default.
    let fixture = fixture_with(Some(RoutingRole::WorkspaceEdit), None).await;

    let error = list_ticket_attachments_http(
        State(fixture.state.clone()),
        fixture.headers(),
        axum::Json(list_request(TicketAttachmentProvider::Linear)),
    )
    .await
    .expect_err("a run with no agent_name should be rejected");

    assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// contentPath / contentText exposure (STEP B)
// ============================================================================

#[test]
fn fetch_response_exposes_materialized_content_path_when_location_is_some() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Jira, "JIRA-1", "attachment-1")
            .expect("source handle");
    let location = build_ticket_attachment_content_location(temp.path(), &source, "evidence.txt")
        .expect("location");
    let descriptor = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Jira,
        "JIRA-1",
        "attachment-1",
        "evidence.txt",
        Some("text/plain"),
        Some(11),
        None,
    )
    .expect("descriptor");
    let expected_path = location.path().display().to_string();

    let response = fetch_response(TicketAttachmentFetchResult {
        descriptor,
        location: Some(location),
    });

    assert_eq!(response.content.content_path.as_deref(), Some(expected_path.as_str()));
    assert!(response.content.content_text.is_none());
}

#[tokio::test]
async fn fetch_response_with_inline_text_includes_small_text_preview() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Jira, "JIRA-1", "attachment-1")
            .expect("source handle");
    let location = build_ticket_attachment_content_location(temp.path(), &source, "evidence.txt")
        .expect("location");
    tokio::fs::create_dir_all(location.path().parent().expect("parent dir"))
        .await
        .expect("create parent dir");
    tokio::fs::write(location.path(), b"steps to reproduce")
        .await
        .expect("write fixture content");
    let descriptor = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Jira,
        "JIRA-1",
        "attachment-1",
        "evidence.txt",
        Some("text/plain"),
        Some(19),
        None,
    )
    .expect("descriptor");

    let response = fetch_response_with_inline_text(TicketAttachmentFetchResult {
        descriptor,
        location: Some(location),
    })
    .await;

    assert_eq!(
        response.content.content_text.as_deref(),
        Some("steps to reproduce")
    );
    assert!(response.content.content_path.is_some());
}

#[tokio::test]
async fn fetch_response_with_inline_text_skips_non_text_media_types() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Jira, "JIRA-1", "attachment-2")
            .expect("source handle");
    let location = build_ticket_attachment_content_location(temp.path(), &source, "evidence.png")
        .expect("location");
    tokio::fs::create_dir_all(location.path().parent().expect("parent dir"))
        .await
        .expect("create parent dir");
    tokio::fs::write(location.path(), b"not actually png bytes")
        .await
        .expect("write fixture content");
    let descriptor = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Jira,
        "JIRA-1",
        "attachment-2",
        "evidence.png",
        Some("image/png"),
        Some(23),
        None,
    )
    .expect("descriptor");

    let response = fetch_response_with_inline_text(TicketAttachmentFetchResult {
        descriptor,
        location: Some(location),
    })
    .await;

    assert!(response.content.content_text.is_none());
    assert!(response.content.content_path.is_some());
}

#[tokio::test]
async fn fetch_response_with_inline_text_skips_missing_media_type() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Jira, "JIRA-1", "attachment-3")
            .expect("source handle");
    let location = build_ticket_attachment_content_location(temp.path(), &source, "evidence")
        .expect("location");
    tokio::fs::create_dir_all(location.path().parent().expect("parent dir"))
        .await
        .expect("create parent dir");
    tokio::fs::write(location.path(), b"unlabeled bytes")
        .await
        .expect("write fixture content");
    let descriptor = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Jira,
        "JIRA-1",
        "attachment-3",
        "evidence",
        None,
        Some(16),
        None,
    )
    .expect("descriptor");

    let response = fetch_response_with_inline_text(TicketAttachmentFetchResult {
        descriptor,
        location: Some(location),
    })
    .await;

    assert!(response.content.content_text.is_none());
}

#[test]
fn provider_item_normalizes_all_providers_to_safe_metadata_only() {
    let unsafe_attachment_id = "https://example.test/attachment?token=secret";
    let unsafe_file_name = "Bearer secret";

    for provider in [
        TicketAttachmentProvider::Jira,
        TicketAttachmentProvider::Linear,
        TicketAttachmentProvider::ClickUp,
    ] {
        let item = provider_item(
            provider,
            "TICKET-1",
            safe_attachment_id(Some(unsafe_attachment_id), 7),
            safe_file_name(unsafe_file_name, 7),
            Some("text/plain"),
            Some(512),
            Some("2026-07-14T00:00:00Z".to_string()),
            Some(
                match provider {
                    TicketAttachmentProvider::Jira => {
                        "https://example.atlassian.net/secure/attachment/7/evidence.txt"
                    }
                    TicketAttachmentProvider::Linear => "https://uploads.linear.app/evidence.txt",
                    TicketAttachmentProvider::ClickUp => {
                        "https://attachments.clickup.com/evidence.txt"
                    }
                }
                .to_string(),
            ),
        )
        .expect("unsafe provider metadata should fall back to safe descriptor values");

        assert_eq!(item.descriptor.provider, provider);
        assert_eq!(item.descriptor.file_name, "attachment-7");
        assert!(item.descriptor.content_pointer.id().starts_with("ta_"));
        assert!(!item.descriptor.id.contains("https://"));
        assert!(!item.descriptor.id.contains("token=secret"));
        assert!(!item.descriptor.file_name.contains("Bearer"));
        assert!(item.content_fetch_supported);
        assert!(item.source.fetch_url().is_some());
    }
}

#[test]
fn provider_item_keeps_unsafe_fetch_urls_unsupported_and_private() {
    for fetch_url in [
        None,
        Some("http://example.atlassian.net/attachment.txt".to_string()),
        Some("https://127.0.0.1/attachment.txt".to_string()),
        Some("https://evil.example/attachment.txt".to_string()),
        Some("https://user@example.atlassian.net/attachment.txt".to_string()),
    ] {
        let item = provider_item(
            TicketAttachmentProvider::Jira,
            "JIRA-1",
            "attachment-1".to_string(),
            "evidence.txt".to_string(),
            Some("text/plain"),
            Some(512),
            None,
            fetch_url,
        )
        .expect("safe descriptor should still list metadata");

        assert!(!item.content_fetch_supported);
        assert!(item.source.fetch_url().is_none());
        assert!(item.descriptor.content_pointer.id().starts_with("ta_"));
    }
}

#[test]
fn provider_item_drops_unsafe_optional_metadata_before_public_descriptor() {
    let item = provider_item(
        TicketAttachmentProvider::Linear,
        "LIN-1",
        "attachment-1".to_string(),
        "evidence.txt".to_string(),
        Some("https://example.test/content-type?token=secret"),
        Some(512),
        Some("Authorization: Bearer provider-secret".to_string()),
        Some("https://uploads.linear.app/evidence.txt".to_string()),
    )
    .expect("safe required metadata should still produce a descriptor");
    let serialized = serde_json::to_string(&item.descriptor).expect("descriptor serializes");

    assert_eq!(item.descriptor.media_type, None);
    assert_eq!(item.descriptor.created_at, None);
    assert!(!serialized.contains("https://example.test"));
    assert!(!serialized.contains("token=secret"));
    assert!(!serialized.contains("Authorization"));
    assert!(!serialized.contains("Bearer"));
    assert!(item.content_fetch_supported);
    assert!(item.source.fetch_url().is_some());
}

#[test]
fn attachment_fetch_target_allows_only_provider_https_hosts() {
    let cases = [
        (
            TicketAttachmentProvider::Jira,
            "https://example.atlassian.net/secure/attachment/1/evidence.txt",
        ),
        (
            TicketAttachmentProvider::Jira,
            "https://api.atlassian.com/ex/jira/cloud/secure/attachment/1/evidence.txt",
        ),
        (
            TicketAttachmentProvider::Linear,
            "https://uploads.linear.app/evidence.txt",
        ),
        (
            TicketAttachmentProvider::ClickUp,
            "https://attachments.clickup.com/evidence.txt",
        ),
    ];

    for (provider, url) in cases {
        let target = AttachmentFetchTarget::new(provider, url)
            .expect("provider attachment URL should be accepted");
        assert!(!target.host().is_empty());
    }

    for url in [
        "http://uploads.linear.app/evidence.txt",
        "https://localhost/evidence.txt",
        "https://127.0.0.1/evidence.txt",
        "https://example.test/evidence.txt",
        "https://user@uploads.linear.app/evidence.txt",
    ] {
        let result = AttachmentFetchTarget::new(TicketAttachmentProvider::Linear, url);
        assert!(result.is_err(), "{url:?} should be rejected");
    }
}

#[test]
fn fetch_response_returns_only_safe_untrusted_content_reference() {
    let descriptor = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Linear,
        "LIN-1",
        "att-1",
        "evidence.txt",
        Some("text/plain"),
        Some(128),
        None,
    )
    .expect("descriptor");
    let response = fetch_response(TicketAttachmentFetchResult {
        descriptor,
        location: None,
    });
    let serialized = serde_json::to_string(&response).expect("fetch response serializes");

    assert_eq!(response.content.kind, "ticket_attachment_content");
    assert_eq!(response.content.trust, "untrusted_external_content");
    assert!(response.content.available);
    assert!(response.content.id.starts_with("ta_"));
    assert!(!serialized.contains("location"));
    assert!(!serialized.contains("path"));
    assert!(!serialized.contains("source"));
    assert!(!serialized.contains("http://"));
    assert!(!serialized.contains("https://"));
    assert!(!serialized.to_ascii_lowercase().contains("token"));
}
