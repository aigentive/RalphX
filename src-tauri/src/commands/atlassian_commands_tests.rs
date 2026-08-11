use chrono::{DateTime, Utc};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

use super::*;
use crate::domain::entities::{
    AgentConversationJiraRefreshStatus, ChatConversation, ChatMessageId,
};

fn assigned_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-17T12:00:00Z")
        .expect("timestamp")
        .with_timezone(&Utc)
}

fn link() -> AgentConversationJiraIssueLink {
    let mut link = AgentConversationJiraIssueLink::new(
        ChatConversationId::new(),
        ProjectId::from_string("project-1".to_string()),
        "RX-42".to_string(),
        assigned_at(),
    )
    .with_reference_metadata(
        Some("10042".to_string()),
        Some("Fix Jira tab".to_string()),
        Some("https://jira.test/browse/RX-42".to_string()),
    )
    .with_assignment_source(Some(ChatMessageId::from_string("msg-1")), true);
    link.status = Some("In Progress".to_string());
    link.assignee = Some("Ada Lovelace".to_string());
    link.reporter = Some("Grace Hopper".to_string());
    link.updated_at_remote = Some("2026-06-17T11:00:00.000+0000".to_string());
    link.description_markdown = Some("## Details\nShip it".to_string());
    link.description_text = Some("Details\nShip it".to_string());
    link.acceptance_criteria_markdown = Some("- Visible Jira tab".to_string());
    link.acceptance_criteria_text = Some("Visible Jira tab".to_string());
    link.comments_json = serde_json::to_string(&vec![AtlassianJiraComment {
        id: Some("c1".to_string()),
        author: Some("Reviewer".to_string()),
        body_markdown: "Looks good".to_string(),
        body_text: "Looks good".to_string(),
        created_at: Some("2026-06-17T12:01:00.000+0000".to_string()),
        updated_at: None,
    }])
    .expect("comments json");
    link.attachments_json = serde_json::to_string(&vec![AtlassianJiraAttachment {
        id: Some("a1".to_string()),
        filename: "design.png".to_string(),
        mime_type: Some("image/png".to_string()),
        size: Some(42),
        author: Some("Designer".to_string()),
        content_url: Some("https://jira.test/attachment/a1".to_string()),
        thumbnail_url: None,
        created_at: Some("2026-06-17T12:02:00.000+0000".to_string()),
    }])
    .expect("attachments json");
    link.last_refreshed_at = Some(assigned_at());
    link.refresh_status = AgentConversationJiraRefreshStatus::Loaded;
    link
}

#[test]
fn jira_issue_response_serializes_rich_cached_fields() {
    let response = AgentConversationJiraIssueLinkResponse::from(link());

    assert_eq!(response.issue_key, "RX-42");
    assert_eq!(response.issue_id.as_deref(), Some("10042"));
    assert_eq!(response.title.as_deref(), Some("Fix Jira tab"));
    assert_eq!(response.status.as_deref(), Some("In Progress"));
    assert_eq!(response.assignee.as_deref(), Some("Ada Lovelace"));
    assert_eq!(response.reporter.as_deref(), Some("Grace Hopper"));
    assert_eq!(
        response.updated_at_remote.as_deref(),
        Some("2026-06-17T11:00:00.000+0000")
    );
    assert_eq!(
        response.description_markdown.as_deref(),
        Some("## Details\nShip it")
    );
    assert_eq!(
        response.acceptance_criteria_markdown.as_deref(),
        Some("- Visible Jira tab")
    );
    assert_eq!(response.comments.len(), 1);
    assert_eq!(response.comments[0].author.as_deref(), Some("Reviewer"));
    assert_eq!(response.attachments.len(), 1);
    assert_eq!(response.attachments[0].filename, "design.png");
    assert_eq!(response.refresh_status, "loaded");
    assert!(response.manually_assigned);
    assert_eq!(response.assigned_from_message_id.as_deref(), Some("msg-1"));
}

#[test]
fn parse_and_trim_helpers_reject_invalid_ids_and_blank_values() {
    assert!(parse_conversation_id("not-a-uuid").is_err());
    assert!(parse_conversation_id("11111111-1111-1111-1111-111111111111").is_ok());
    assert_eq!(
        non_empty(Some("  RX-42  ".to_string())),
        Some("RX-42".to_string())
    );
    assert_eq!(non_empty(Some("   ".to_string())), None);
    assert_eq!(non_empty(None), None);
}

#[tokio::test]
async fn resolve_assignment_project_id_uses_project_conversation_when_explicit_missing() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-1".to_string());
    let conversation = ChatConversation::new_project(project_id.clone());
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create conversation");

    let resolved = resolve_assignment_project_id(&state, &conversation_id, None)
        .await
        .expect("project id");

    assert_eq!(resolved, project_id);
}

#[tokio::test]
async fn refresh_jira_issue_link_records_error_when_integration_disabled() {
    let state = AppState::new_test();

    let refreshed = crate::application::agent_conversation_jira_issue::refresh_jira_issue_link(
        &state.agent_conversation_jira_issue_repo,
        state.atlassian_integration_service.as_ref(),
        link(),
    )
    .await
    .expect("refresh stores error");

    assert_eq!(
        refreshed.refresh_status,
        AgentConversationJiraRefreshStatus::Error
    );
    assert_eq!(
        refreshed.refresh_error.as_deref(),
        Some("Atlassian integration is not enabled")
    );
    assert!(state
        .agent_conversation_jira_issue_repo
        .get_by_conversation_id(&refreshed.conversation_id)
        .await
        .expect("load refreshed link")
        .is_some());
}

#[tokio::test]
async fn jira_assignment_commands_return_validation_errors_for_empty_or_missing_assignments() {
    let app = mock_builder()
        .manage(AppState::new_test())
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let conversation_id = ChatConversationId::new();

    let empty_issue_error = assign_agent_conversation_jira_issue(
        AssignAgentConversationJiraIssueInput {
            conversation_id: conversation_id.as_str(),
            project_id: Some("project-1".to_string()),
            issue_key: "   ".to_string(),
            issue_id: None,
            title: None,
            issue_url: None,
            refresh: Some(false),
        },
        app.state::<AppState>(),
    )
    .await
    .unwrap_err();
    assert_eq!(empty_issue_error, "Jira issue key is required");

    let missing_assignment_error = refresh_agent_conversation_jira_issue(
        RefreshAgentConversationJiraIssueInput {
            conversation_id: conversation_id.as_str(),
        },
        app.state::<AppState>(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        missing_assignment_error,
        "No Jira issue is assigned to this conversation"
    );

    let missing_assign_to_me_error = assign_agent_conversation_jira_issue_to_me(
        AssignAgentConversationJiraIssueToMeInput {
            conversation_id: conversation_id.as_str(),
        },
        app.state::<AppState>(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        missing_assign_to_me_error,
        "No Jira issue is assigned to this conversation"
    );
}

#[tokio::test]
async fn jira_assignment_commands_round_trip_and_refresh_cached_issue() {
    let app_state = AppState::new_test();
    app_state
        .atlassian_integration_service
        .save_settings(
            Some(AtlassianAuthMethod::ApiToken),
            Some("https://example.atlassian.net".to_string()),
            Some("dev@example.com".to_string()),
            Some("token".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("save Atlassian settings");
    app_state
        .atlassian_integration_service
        .validate_and_enable()
        .await
        .expect("enable Atlassian");

    let project_id = ProjectId::from_string("project-1".to_string());
    let conversation = ChatConversation::new_project(project_id.clone());
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create conversation");
    let app = mock_builder()
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app");

    let assigned = assign_agent_conversation_jira_issue(
        AssignAgentConversationJiraIssueInput {
            conversation_id: conversation_id.as_str(),
            project_id: None,
            issue_key: " rx-42 ".to_string(),
            issue_id: Some(" 10042 ".to_string()),
            title: Some(" Fix Jira tab ".to_string()),
            issue_url: Some(" https://jira.test/browse/RX-42 ".to_string()),
            refresh: Some(false),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("assign Jira issue")
    .issue
    .expect("assigned response");

    assert_eq!(assigned.project_id, project_id.as_str());
    assert_eq!(assigned.issue_key, "RX-42");
    assert_eq!(assigned.issue_id.as_deref(), Some("10042"));
    assert_eq!(assigned.title.as_deref(), Some("Fix Jira tab"));
    assert_eq!(assigned.refresh_status, "not_loaded");
    assert!(assigned.manually_assigned);

    let assigned_to_me = assign_agent_conversation_jira_issue_to_me(
        AssignAgentConversationJiraIssueToMeInput {
            conversation_id: conversation_id.as_str(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("assign Jira issue to current user")
    .issue
    .expect("assigned-to-me response");
    assert_eq!(assigned_to_me.issue_key, "RX-42");
    assert_eq!(assigned_to_me.refresh_status, "loaded");
    assert!(assigned_to_me.last_refreshed_at.is_some());

    let loaded = get_agent_conversation_jira_issue(
        GetAgentConversationJiraIssueInput {
            conversation_id: conversation_id.as_str(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("get Jira issue")
    .issue
    .expect("loaded response");
    assert_eq!(loaded.issue_key, "RX-42");

    let refreshed = refresh_agent_conversation_jira_issue(
        RefreshAgentConversationJiraIssueInput {
            conversation_id: conversation_id.as_str(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("refresh Jira issue")
    .issue
    .expect("refreshed response");
    assert_eq!(refreshed.refresh_status, "loaded");
    assert_eq!(refreshed.title.as_deref(), Some("Fix Jira tab"));
    assert!(refreshed.last_refreshed_at.is_some());

    let cleared = clear_agent_conversation_jira_issue(
        ClearAgentConversationJiraIssueInput {
            conversation_id: conversation_id.as_str(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("clear Jira issue");
    assert!(cleared.issue.is_none());
}

#[tokio::test]
async fn resolve_atlassian_resource_urls_returns_resources_for_owned_urls() {
    let app_state = AppState::new_test();
    app_state
        .atlassian_integration_service
        .save_settings(
            Some(AtlassianAuthMethod::ApiToken),
            Some("https://example.atlassian.net".to_string()),
            Some("dev@example.com".to_string()),
            Some("token".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("save Atlassian settings");
    app_state
        .atlassian_integration_service
        .validate_and_enable()
        .await
        .expect("enable Atlassian");
    let app = mock_builder()
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app");

    let response = resolve_atlassian_resource_urls(
        ResolveAtlassianResourceUrlsInput {
            urls: vec![
                "https://example.atlassian.net/browse/rx-42".to_string(),
                "https://other.atlassian.net/browse/RX-99".to_string(),
            ],
        },
        app.state::<AppState>(),
    )
    .await
    .expect("resolve URLs");

    assert_eq!(response.results.len(), 2);
    let resource = response.results[0].resource.as_ref().expect("resource");
    assert_eq!(resource.kind, AtlassianResourceKind::Jira);
    assert_eq!(resource.id, "RX-42");
    assert_eq!(resource.key.as_deref(), Some("RX-42"));
    assert_eq!(
        resource.url.as_deref(),
        Some("https://example.atlassian.net/browse/RX-42")
    );
    assert!(response.results[1].resource.is_none());
}

#[tokio::test]
async fn resolve_atlassian_resource_urls_rejects_oauth_when_feature_disabled() {
    let app_state = AppState::new_test();
    app_state
        .atlassian_integration_service
        .save_settings(
            Some(AtlassianAuthMethod::OAuth),
            Some("https://example.atlassian.net".to_string()),
            None,
            None,
            Some("client-id".to_string()),
            Some("client-secret".to_string()),
            Some("http://127.0.0.1:8765/atlassian/oauth/callback".to_string()),
        )
        .await
        .expect("save OAuth settings");
    let app = mock_builder()
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app");

    let error = resolve_atlassian_resource_urls(
        ResolveAtlassianResourceUrlsInput {
            urls: vec!["https://example.atlassian.net/browse/RX-42".to_string()],
        },
        app.state::<AppState>(),
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        "Atlassian OAuth setup is disabled. Use API token setup for now."
    );
}

#[tokio::test]
async fn disconnect_atlassian_integration_resets_saved_connection() {
    let app_state = AppState::new_test();
    app_state
        .atlassian_integration_service
        .save_settings(
            Some(AtlassianAuthMethod::ApiToken),
            Some("https://example.atlassian.net".to_string()),
            Some("dev@example.com".to_string()),
            Some("token".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("save Atlassian settings");
    app_state
        .atlassian_integration_service
        .validate_and_enable()
        .await
        .expect("enable Atlassian");
    let app = mock_builder()
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app");

    let before = get_atlassian_integration_settings(app.state::<AppState>())
        .await
        .expect("settings load");
    assert!(before.enabled);
    assert!(before.has_api_token);

    let cleared = disconnect_atlassian_integration(app.state::<AppState>())
        .await
        .expect("disconnect Atlassian");

    assert!(!cleared.enabled);
    assert!(!cleared.has_api_token);
    assert!(!cleared.has_oauth_client_secret);
    assert!(!cleared.has_oauth_token);
    assert_eq!(cleared.validation_status, "not_configured");
    assert!(!cleared.jira_available);
    assert!(!cleared.confluence_available);
    assert_eq!(cleared.auth_method, "api_token");
}
