use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tauri::Manager;

use super::granola_commands::{
    assign_agent_conversation_granola_note, clear_agent_conversation_granola_note,
    get_agent_conversation_granola_note, get_granola_integration_settings, get_granola_note_detail,
    list_granola_notes, refresh_agent_conversation_granola_note, save_granola_integration_settings,
    validate_granola_integration_settings, AssignAgentConversationGranolaNoteInput,
    ClearAgentConversationGranolaNoteInput, GetAgentConversationGranolaNoteInput,
    GetGranolaNoteDetailInput, GranolaIntegrationSettingsResponse, ListGranolaNotesInput,
    RefreshAgentConversationGranolaNoteInput, SaveGranolaIntegrationSettingsInput,
};
use crate::application::{
    AppState, GranolaApiClient, GranolaApiError, GranolaAuthContext, GranolaIntegrationService,
    GranolaNoteDetail, GranolaNoteListPage, GranolaNoteSummary, GranolaTranscriptEntry,
};
use crate::domain::entities::{
    AgentConversationGranolaNoteLink, AgentConversationJiraIssueLink, AgentConversationWorkspace,
    AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::integrations::{GranolaIntegrationSettings, IntegrationValidationStatus};
use crate::infrastructure::memory::{
    MemoryGranolaIntegrationSettingsRepository, MemorySecretStore,
};

#[derive(Default)]
struct TestGranolaClient {
    list_requests: Mutex<Vec<(usize, Option<String>)>>,
    detail_requests: Mutex<Vec<(String, bool)>>,
}

impl TestGranolaClient {
    fn list_requests(&self) -> Vec<(usize, Option<String>)> {
        self.list_requests.lock().expect("list requests").clone()
    }

    fn detail_requests(&self) -> Vec<(String, bool)> {
        self.detail_requests
            .lock()
            .expect("detail requests")
            .clone()
    }
}

#[async_trait]
impl GranolaApiClient for TestGranolaClient {
    async fn validate(&self, _auth: &GranolaAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_notes(
        &self,
        _auth: &GranolaAuthContext,
        page_size: usize,
        cursor: Option<&str>,
    ) -> Result<GranolaNoteListPage, GranolaApiError> {
        self.list_requests
            .lock()
            .expect("list requests")
            .push((page_size, cursor.map(ToOwned::to_owned)));
        Ok(GranolaNoteListPage {
            notes: vec![GranolaNoteSummary {
                id: "not_1234567890ABCD".to_string(),
                title: Some("Planning sync".to_string()),
                url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
                summary: Some("Discussed the launch plan".to_string()),
                created_at: Some("2026-06-20T12:00:00Z".to_string()),
                updated_at: Some("2026-06-20T13:00:00Z".to_string()),
            }],
            has_more: true,
            cursor: Some("next-cursor".to_string()),
        })
    }

    async fn fetch_note_detail(
        &self,
        _auth: &GranolaAuthContext,
        note_id: &str,
        include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        self.detail_requests
            .lock()
            .expect("detail requests")
            .push((note_id.to_string(), include_transcript));
        Ok(GranolaNoteDetail {
            id: note_id.to_string(),
            title: Some("Planning sync".to_string()),
            url: Some(format!("https://granola.ai/notes/{note_id}")),
            summary: Some("Fresh summary from Granola".to_string()),
            transcript: include_transcript.then(|| {
                vec![GranolaTranscriptEntry {
                    speaker: Some("Alex".to_string()),
                    text: "Transcript line".to_string(),
                    start_ms: Some(100),
                    end_ms: Some(250),
                }]
            }),
        })
    }
}

fn test_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

fn test_app_with_granola_client(
    client: Arc<dyn GranolaApiClient>,
) -> tauri::App<tauri::test::MockRuntime> {
    let mut state = AppState::new_test();
    state.granola_integration_service = Arc::new(GranolaIntegrationService::new(
        Arc::new(MemoryGranolaIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        client,
    ));
    tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

async fn enable_granola(app: &tauri::App<tauri::test::MockRuntime>) {
    save_granola_integration_settings(
        SaveGranolaIntegrationSettingsInput {
            api_token: Some("grn_test_token".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save Granola token");
    validate_granola_integration_settings(app.state::<AppState>())
        .await
        .expect("validate Granola token");
}

#[test]
fn integration_settings_response_reports_presence_without_leaking_token_ref() {
    let settings = GranolaIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        last_error: Some("previous error".to_string()),
        ..Default::default()
    };

    let response = GranolaIntegrationSettingsResponse::from(settings);

    assert!(response.enabled);
    assert!(response.has_api_token);
    assert_eq!(response.validation_status, "valid");
    assert_eq!(response.last_error.as_deref(), Some("previous error"));

    // The keychain reference (and therefore the token) must never serialize out.
    let json = serde_json::to_string(&response).expect("response serializes");
    assert!(
        !json.contains("integrations/granola/default/api-token"),
        "response leaked the secret ref: {json}"
    );
    assert!(
        !json.contains("tokenSecretRef"),
        "response leaked the ref field: {json}"
    );
}

#[test]
fn integration_settings_response_handles_unconfigured_settings() {
    let response = GranolaIntegrationSettingsResponse::from(GranolaIntegrationSettings::default());

    assert!(!response.enabled);
    assert!(!response.has_api_token);
    assert_eq!(response.validation_status, "not_configured");
    assert!(response.last_error.is_none());
}

#[tokio::test]
async fn get_granola_integration_settings_returns_default_state() {
    let app = test_app();

    let settings = get_granola_integration_settings(app.state::<AppState>())
        .await
        .expect("default settings should load");

    assert!(!settings.enabled);
    assert!(!settings.has_api_token);
    assert_eq!(settings.validation_status, "not_configured");
}

#[tokio::test]
async fn save_granola_integration_settings_hides_token_and_returns_pending() {
    let app = test_app();

    let saved = save_granola_integration_settings(
        SaveGranolaIntegrationSettingsInput {
            api_token: Some("grn_secret_token".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save should succeed");

    assert!(saved.has_api_token);
    assert!(
        !saved.enabled,
        "saving returns a pending, not-enabled state"
    );
    assert_eq!(saved.validation_status, "pending");

    // The raw token must never appear in the command response.
    let json = serde_json::to_string(&saved).expect("response serializes");
    assert!(
        !json.contains("grn_secret_token"),
        "save response leaked the token: {json}"
    );
}

#[tokio::test]
async fn validate_granola_integration_settings_reports_missing_token() {
    let app = test_app();

    let error = validate_granola_integration_settings(app.state::<AppState>())
        .await
        .expect_err("validation without a token should fail");

    assert!(
        error.contains("Granola API token is required"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn save_then_validate_enables_then_blank_token_resets() {
    let app = test_app();

    save_granola_integration_settings(
        SaveGranolaIntegrationSettingsInput {
            api_token: Some("grn_token".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save Granola settings");

    let validated = validate_granola_integration_settings(app.state::<AppState>())
        .await
        .expect("validate enables the integration");
    assert!(validated.enabled);
    assert_eq!(validated.validation_status, "valid");
    assert!(validated.last_error.is_none());
    assert!(validated.last_validated_at.is_some());

    // A blank token disconnects: secret cleared, not-configured, no token.
    let cleared = save_granola_integration_settings(
        SaveGranolaIntegrationSettingsInput {
            api_token: Some(String::new()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("blank token resets Granola settings");
    assert!(!cleared.enabled);
    assert!(!cleared.has_api_token);
    assert_eq!(cleared.validation_status, "not_configured");
}

#[tokio::test]
async fn agent_conversation_granola_note_commands_assign_get_and_clear_without_refresh() {
    let app = test_app();
    let conversation_id = "123e4567-e89b-12d3-a456-426614174000".to_string();

    let assigned = assign_agent_conversation_granola_note(
        AssignAgentConversationGranolaNoteInput {
            conversation_id: conversation_id.clone(),
            project_id: Some("project-1".to_string()),
            note_id: "not_1234567890ABCD".to_string(),
            title: Some("Planning sync".to_string()),
            note_url: Some("https://granola.ai/notes/not_1234567890ABCD".to_string()),
            summary: Some("Discussed the plan".to_string()),
            include_transcript: Some(true),
            refresh: Some(false),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("assign granola note");
    let assigned_note = assigned.note.expect("assigned note");
    assert_eq!(assigned_note.conversation_id, conversation_id);
    assert_eq!(assigned_note.project_id, "project-1");
    assert_eq!(assigned_note.note_id, "not_1234567890ABCD");
    assert_eq!(assigned_note.title.as_deref(), Some("Planning sync"));
    assert_eq!(
        assigned_note.summary_markdown.as_deref(),
        Some("Discussed the plan")
    );
    assert_eq!(assigned_note.refresh_status, "not_loaded");
    assert!(assigned_note.manually_assigned);

    let loaded = get_agent_conversation_granola_note(
        GetAgentConversationGranolaNoteInput {
            conversation_id: conversation_id.clone(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("get assigned note");
    assert_eq!(
        loaded.note.expect("loaded note").note_id,
        "not_1234567890ABCD"
    );

    let cleared = clear_agent_conversation_granola_note(
        ClearAgentConversationGranolaNoteInput { conversation_id },
        app.state::<AppState>(),
    )
    .await
    .expect("clear assigned note");
    assert!(cleared.note.is_none());
}

#[tokio::test]
async fn granola_note_commands_list_and_detail_return_api_data() {
    let client = Arc::new(TestGranolaClient::default());
    let app = test_app_with_granola_client(client.clone());
    enable_granola(&app).await;

    let listed = list_granola_notes(
        ListGranolaNotesInput {
            page_size: Some(99),
            cursor: Some("cursor/value".to_string()),
            project_id: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("list Granola notes");

    assert_eq!(listed.notes.len(), 1);
    assert_eq!(listed.notes[0].id, "not_1234567890ABCD");
    assert_eq!(listed.notes[0].title.as_deref(), Some("Planning sync"));
    assert!(listed.has_more);
    assert_eq!(listed.cursor.as_deref(), Some("next-cursor"));
    assert_eq!(
        client.list_requests(),
        vec![(30, Some("cursor/value".to_string()))]
    );

    let detail = get_granola_note_detail(
        GetGranolaNoteDetailInput {
            note_id: " not_1234567890ABCD ".to_string(),
            include_transcript: Some(true),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("get Granola note detail");

    assert_eq!(detail.id, "not_1234567890ABCD");
    assert_eq!(
        detail.summary.as_deref(),
        Some("Fresh summary from Granola")
    );
    assert_eq!(detail.transcript.len(), 1);
    assert_eq!(detail.transcript[0].speaker.as_deref(), Some("Alex"));
    assert_eq!(detail.transcript[0].text, "Transcript line");
    assert_eq!(
        client.detail_requests(),
        vec![("not_1234567890ABCD".to_string(), true)]
    );
}

#[tokio::test]
async fn granola_note_list_includes_project_conversation_ticket_and_pr_associations() {
    let client = Arc::new(TestGranolaClient::default());
    let app = test_app_with_granola_client(client);
    enable_granola(&app).await;
    let project_id = ProjectId::from_string("project-with-granola-links".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.set_title("Launch checklist agent");
    let conversation = app
        .state::<AppState>()
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create project conversation");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Current branch (main)".to_string()),
        None,
        "feature/launch-checklist".to_string(),
        "/tmp/ralphx-launch-checklist".to_string(),
    );
    workspace.publication_pr_number = Some(516);
    workspace.publication_pr_url =
        Some("https://github.com/aigentive/ralphx.app/pull/516".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    app.state::<AppState>()
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("create workspace");
    app.state::<AppState>()
        .agent_conversation_granola_note_repo
        .upsert(AgentConversationGranolaNoteLink::new(
            conversation.id,
            project_id.clone(),
            "not_1234567890ABCD".to_string(),
            chrono::Utc::now(),
        ))
        .await
        .expect("link Granola note");
    app.state::<AppState>()
        .agent_conversation_jira_issue_repo
        .upsert({
            let mut link = AgentConversationJiraIssueLink::new(
                conversation.id,
                project_id.clone(),
                "RX-77".to_string(),
                chrono::Utc::now(),
            );
            link.title = Some("Launch checklist ticket".to_string());
            link.issue_url = Some("https://example.atlassian.net/browse/RX-77".to_string());
            link
        })
        .await
        .expect("link Jira ticket");

    let listed = list_granola_notes(
        ListGranolaNotesInput {
            page_size: Some(30),
            cursor: None,
            project_id: Some(project_id.as_str().to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("list Granola notes");

    let note = listed.notes.first().expect("Granola note");
    assert_eq!(note.id, "not_1234567890ABCD");
    assert_eq!(note.rx_conversation_count, 1);
    assert_eq!(
        note.rx_conversations[0].conversation_id,
        conversation.id.as_str()
    );
    assert_eq!(
        note.rx_conversations[0].title.as_deref(),
        Some("Launch checklist agent")
    );
    assert_eq!(note.ticket_count, 1);
    assert_eq!(note.ticket_links[0].provider, "jira");
    assert_eq!(note.ticket_links[0].label, "RX-77");
    assert_eq!(note.pr_count, 1);
    assert_eq!(note.pull_requests[0].number, 516);
    assert_eq!(note.pull_requests[0].status.as_deref(), Some("open"));
}

#[tokio::test]
async fn agent_conversation_granola_note_commands_refresh_assigned_note_from_service() {
    let client = Arc::new(TestGranolaClient::default());
    let app = test_app_with_granola_client(client.clone());
    enable_granola(&app).await;
    let conversation_id = "123e4567-e89b-12d3-a456-426614174111".to_string();

    assign_agent_conversation_granola_note(
        AssignAgentConversationGranolaNoteInput {
            conversation_id: conversation_id.clone(),
            project_id: Some("project-1".to_string()),
            note_id: "not_1234567890ABCD".to_string(),
            title: Some("Planning sync".to_string()),
            note_url: None,
            summary: None,
            include_transcript: Some(true),
            refresh: Some(false),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("assign Granola note");

    let refreshed = refresh_agent_conversation_granola_note(
        RefreshAgentConversationGranolaNoteInput {
            conversation_id: conversation_id.clone(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("refresh Granola note");
    let note = refreshed.note.expect("refreshed note");

    assert_eq!(note.conversation_id, conversation_id);
    assert_eq!(
        note.summary_markdown.as_deref(),
        Some("Fresh summary from Granola")
    );
    assert_eq!(note.refresh_status, "loaded");
    assert_eq!(note.transcript.len(), 1);
    assert_eq!(
        client.detail_requests(),
        vec![("not_1234567890ABCD".to_string(), true)]
    );
}

#[tokio::test]
async fn agent_conversation_granola_note_assignment_can_resolve_project_conversation() {
    let app = test_app();
    let project_id = ProjectId::from_string("project-from-conversation".to_string());
    let conversation = app
        .state::<AppState>()
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("create project conversation");

    let assigned = assign_agent_conversation_granola_note(
        AssignAgentConversationGranolaNoteInput {
            conversation_id: conversation.id.as_str().to_string(),
            project_id: None,
            note_id: "not_1234567890ABCD".to_string(),
            title: Some("  Planning sync  ".to_string()),
            note_url: Some("  https://granola.ai/notes/not_1234567890ABCD  ".to_string()),
            summary: Some("  Discussed the plan  ".to_string()),
            include_transcript: Some(false),
            refresh: Some(false),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("assign Granola note");
    let note = assigned.note.expect("assigned note");

    assert_eq!(note.project_id, project_id.as_str());
    assert_eq!(note.title.as_deref(), Some("Planning sync"));
    assert_eq!(
        note.note_url.as_deref(),
        Some("https://granola.ai/notes/not_1234567890ABCD")
    );
    assert_eq!(note.summary_markdown.as_deref(), Some("Discussed the plan"));
    assert!(!note.include_transcript);
}

#[tokio::test]
async fn agent_conversation_granola_note_commands_validate_bad_inputs() {
    let app = test_app();
    let conversation_id = "123e4567-e89b-12d3-a456-426614174222".to_string();

    let invalid_id = assign_agent_conversation_granola_note(
        AssignAgentConversationGranolaNoteInput {
            conversation_id: conversation_id.clone(),
            project_id: Some("project-1".to_string()),
            note_id: "granola-note".to_string(),
            title: None,
            note_url: None,
            summary: None,
            include_transcript: None,
            refresh: Some(false),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("invalid note id should fail");
    assert!(invalid_id.contains("Granola note id is invalid"));

    let missing_assignment = refresh_agent_conversation_granola_note(
        RefreshAgentConversationGranolaNoteInput {
            conversation_id: conversation_id.clone(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("refresh without assignment should fail");
    assert!(missing_assignment.contains("No Granola note is assigned"));

    let invalid_conversation = get_agent_conversation_granola_note(
        GetAgentConversationGranolaNoteInput {
            conversation_id: "not-a-uuid".to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("invalid conversation id should fail");
    assert_eq!(invalid_conversation, "Invalid conversationId");
}
