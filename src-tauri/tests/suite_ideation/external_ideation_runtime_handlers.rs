use axum::{
    extract::{Path, Query, State},
    Json,
};
use ralphx_lib::application::{
    agent_conversation_workspace::resolve_agent_conversation_workspace_path, AppState,
    InteractiveProcessKey,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use ralphx_lib::domain::entities::{
    ideation::{ChatMessage, IdeationSession, IdeationSessionStatus, SessionOrigin},
    project::{GitMode, Project},
    types::ProjectId,
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, Persona, PersonaId,
    PersonaStatus,
};
use ralphx_lib::domain::services::running_agent_registry::RunningAgentKey;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::HttpServerState;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncBufReadExt, BufReader};

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

fn make_project(id: &str, name: &str) -> Project {
    Project {
        id: ProjectId::from_string(id.to_string()),
        name: name.to_string(),
        working_directory: "/tmp".to_string(),
        git_mode: GitMode::Worktree,
        base_branch: None,
        worktree_parent_directory: None,
        use_feature_branches: true,
        merge_validation_mode: Default::default(),
        merge_strategy: Default::default(),
        detected_analysis: None,
        custom_analysis: None,
        analyzed_at: None,
        github_pr_enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
    }
}

fn unrestricted_scope() -> ProjectScope {
    ProjectScope(None)
}

fn scoped(ids: &[&str]) -> ProjectScope {
    let vec: Vec<ProjectId> = ids
        .iter()
        .map(|s| ProjectId::from_string(s.to_string()))
        .collect();
    ProjectScope(Some(vec))
}

async fn setup_session(
    state: &HttpServerState,
    project_id: &str,
    project_name: &str,
) -> (String, String) {
    let project = make_project(project_id, project_name);
    state.app_state.project_repo.create(project).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());
    let session = IdeationSession::new(pid);
    let created = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    (project_id.to_string(), created.id.as_str().to_string())
}

async fn setup_external_session(
    state: &HttpServerState,
    project_id: &str,
    project_name: &str,
) -> (String, String) {
    let project = make_project(project_id, project_name);
    state.app_state.project_repo.create(project).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());
    let session = IdeationSession::builder()
        .project_id(pid)
        .origin(SessionOrigin::External)
        .api_key_id("test-api-key")
        .build();
    let created = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    (project_id.to_string(), created.id.as_str().to_string())
}

async fn create_message(state: &HttpServerState, msg: ChatMessage) {
    state.app_state.chat_message_repo.create(msg).await.unwrap();
}

async fn create_active_ideation_session(state: &HttpServerState) -> String {
    let session = IdeationSession::new(ProjectId::new());
    let sid = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    sid
}

async fn register_fake_ideation_agent(state: &HttpServerState, session_id: &str) {
    let key = RunningAgentKey::new("ideation", session_id);
    state
        .app_state
        .running_agent_registry
        .register(
            key,
            99999,
            "test-conv".to_string(),
            "test-run".to_string(),
            None,
            None,
        )
        .await;
}

fn fill_queue(state: &HttpServerState, session_id: &str, n: usize) {
    for i in 0..n {
        state.app_state.message_queue.queue(
            ChatContextType::Ideation,
            session_id,
            format!("queued message {i}"),
        );
    }
}

fn external_persona_spawn_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct PersonaCaptureCli {
    _temp_dir: tempfile::TempDir,
    path: std::path::PathBuf,
    ran_marker: std::path::PathBuf,
    persona_marker: std::path::PathBuf,
}

fn make_persona_capture_cli() -> PersonaCaptureCli {
    let temp_dir = tempfile::tempdir().expect("persona capture CLI tempdir");
    let path = temp_dir.path().join("claude");
    let ran_marker = temp_dir.path().join("ran");
    let persona_marker = temp_dir.path().join("persona-injected");
    let stdin_capture_path = temp_dir.path().join("stdin.txt");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nfor arg in \"$@\"; do\n  case \"$arg\" in *'<ralphx_agent_persona>'*) touch '{}' ;; esac\n  if [ -f \"$arg\" ] && grep -q '<ralphx_agent_persona>' \"$arg\"; then touch '{}'; fi\ndone\ncat > '{}'\ngrep -q '<ralphx_agent_persona>' '{}' && touch '{}'\ntouch '{}'\nprintf '%s\\n' '{{\"type\":\"result\",\"session_id\":\"external-persona-capture\",\"is_error\":false,\"result\":\"ok\",\"cost_usd\":0.0}}'\n",
            persona_marker.display(),
            persona_marker.display(),
            stdin_capture_path.display(),
            stdin_capture_path.display(),
            persona_marker.display(),
            ran_marker.display(),
        ),
    )
    .expect("write persona capture CLI");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(&path)
            .expect("persona capture CLI metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("make persona capture CLI executable");
    }

    PersonaCaptureCli {
        _temp_dir: temp_dir,
        path,
        ran_marker,
        persona_marker,
    }
}

async fn configure_persona_capture_cli(state: &HttpServerState, cli_path: &std::path::Path) {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.is_default = true;
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(cli_path.to_string_lossy().into_owned());
    state
        .app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("seed capture CLI provider settings");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn external_mcp_send_has_no_persona_block() {
    let _spawn_lock = external_persona_spawn_lock().lock().expect("lock poisoned");
    let _persona_flag = crate::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "1");
    let _spawn_permission =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let state = setup_test_state().await;
    let capture_cli = make_persona_capture_cli();
    configure_persona_capture_cli(&state, &capture_cli.path).await;
    let (project_id, session_id) =
        setup_external_session(&state, "external-persona-project", "External persona send").await;

    let persona = Persona {
        id: PersonaId::from("external-handler-persona"),
        artifact_id: None,

        project_id: None,
        slug: "external-handler-persona".to_string(),
        name: "External Handler Persona".to_string(),
        description: "must not enter external MCP sends".to_string(),
        content: "<ralphx_agent_persona>external handler persona body</ralphx_agent_persona>"
            .to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: "external-handler-persona-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state
        .app_state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed active persona");
    let mut bound_conversation = ChatConversation::new_project(ProjectId::from_string(project_id));
    bound_conversation.persona_id = Some(persona.id.to_string());
    state
        .app_state
        .chat_conversation_repo
        .create(bound_conversation)
        .await
        .expect("seed same-project bound conversation");

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id,
            message: "external handler persona absence".to_string(),
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "external message should spawn through the handler"
    );

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !capture_cli.ran_marker.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("capture CLI should run");
    assert!(
        !capture_cli.persona_marker.exists(),
        "the external handler's final spawn command must not contain a persona block"
    );
}

#[tokio::test]
async fn test_ideation_message_invalid_session_returns_404() {
    let state = setup_test_state().await;

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: "nonexistent-session-id".to_string(),
            message: "hello".to_string(),
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_ideation_message_session_not_active_returns_400() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-msg-inactive", "Inactive Session").await;

    let session_id = IdeationSessionId::from_string(session_id_str.clone());
    state
        .app_state
        .ideation_session_repo
        .update_status(&session_id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "hello".to_string(),
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_ideation_message_scope_violation_returns_403() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-msg-scope-a", "Scope A Session").await;

    let result = ideation_message_http(
        State(state),
        scoped(&["proj-msg-scope-b"]),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "hello".to_string(),
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_ideation_message_linked_cleaned_agent_workspace_returns_non_resumable_before_persisting_message(
) {
    let state = setup_test_state().await;
    let project = make_project("proj-msg-linked-cleaned", "Linked Cleaned Agent Workspace");
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session = IdeationSession::builder()
        .project_id(project.id.clone())
        .source_context_type("agent_conversation")
        .source_context_id("conversation-linked-cleaned")
        .spawn_reason("agent_plan_mode")
        .build();
    let session = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let conversation_id = ChatConversationId::from_string("conversation-linked-cleaned");
    let expected_worktree = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("expected workspace path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/linked-cleaned".to_string(),
        expected_worktree.to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    workspace.publication_pr_number = Some(571);
    workspace.publication_pr_status = Some("merged".to_string());
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let result = ideation_message_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session.id.as_str().to_string(),
            message: "continue the old cleaned workspace".to_string(),
        }),
    )
    .await;

    assert!(result.is_err(), "cleaned workspace must not be spawned");
    let (status, body) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert_eq!(body.0["error"], "agent_workspace_non_resumable");
    assert_eq!(body.0["reason"], "cleaned_after_terminal");
    assert_eq!(
        body.0["next_action"], "start_fresh_agent_conversation",
        "response should direct the caller away from stale continuation"
    );

    let messages = state
        .app_state
        .chat_message_repo
        .get_by_session(&session.id)
        .await
        .unwrap();
    assert!(
        messages.is_empty(),
        "preflight must reject before persisting a continuation message"
    );
}

#[tokio::test]
async fn test_ideation_message_queued_when_agent_running() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-msg-queued", "Queued Session").await;

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            12345,
            "fake-conv-id".to_string(),
            "fake-run-id".to_string(),
            None,
            None,
        )
        .await;

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "hello".to_string(),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "expected Ok response, got: {:?}",
        result.err()
    );
    let response = result.unwrap().0;
    assert_eq!(response.status, "queued");
}

#[tokio::test]
async fn test_ideation_message_sent_when_interactive_process_registered() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_external_session(&state, "proj-msg-sent", "Sent Session").await;
    let message = "hello from test";

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("failed to spawn cat for test");
    let stdin = child.stdin.take().expect("no stdin on cat process");
    let stdout = child.stdout.take().expect("no stdout on cat process");

    let ipr_key = InteractiveProcessKey::new("ideation", &session_id_str);
    state
        .app_state
        .interactive_process_registry
        .register(ipr_key, stdin)
        .await;

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str.clone(),
            message: message.to_string(),
        }),
    )
    .await;

    let mut written = String::new();
    let mut reader = BufReader::new(stdout);
    reader
        .read_line(&mut written)
        .await
        .expect("read cat stdout");
    let _ = child.kill().await;

    assert!(
        result.is_ok(),
        "expected Ok response, got: {:?}",
        result.err()
    );
    let response = result.unwrap().0;
    assert_eq!(response.status, "sent");
    let payload: serde_json::Value = serde_json::from_str(written.trim_end()).expect("valid JSON");
    assert_eq!(payload["type"], "user");
    assert_eq!(payload["message"]["role"], "user");
    let content = payload["message"]["content"]
        .as_str()
        .expect("content string");
    assert!(
        content.contains(&format!("<context_id>{session_id_str}</context_id>")),
        "content must include ideation context wrapper: {content}"
    );
    assert!(
        content.contains(&format!("<user_message>{message}</user_message>")),
        "content must include wrapped user message: {content}"
    );
}

#[tokio::test]
async fn test_ideation_message_unread_returns_409() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_external_session(&state, "proj-rbw-unread", "RBW Unread").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    state
        .app_state
        .chat_message_repo
        .create(ChatMessage::user_in_session(
            session_id.clone(),
            "user message",
        ))
        .await
        .unwrap();
    state
        .app_state
        .chat_message_repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "agent response",
        ))
        .await
        .unwrap();

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "follow-up without reading".to_string(),
        }),
    )
    .await;

    assert!(result.is_err());
    let (status, body) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert_eq!(body.0["error"], "unread_messages");
    assert_eq!(body.0["next_action"], "fetch_messages");
    // count_unread_messages counts both User + Orchestrator roles (2 messages created above)
    assert_eq!(body.0["unread_count"], 2i64);
}

#[tokio::test]
async fn test_ideation_message_external_initial_message_allowed() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_external_session(&state, "proj-rbw-initial", "RBW Initial").await;

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            99999,
            "conv".to_string(),
            "run".to_string(),
            None,
            None,
        )
        .await;

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "initial message".to_string(),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "expected Ok for initial message, got: {:?}",
        result.err()
    );
    let response = result.unwrap().0;
    assert_eq!(response.status, "queued");
}

#[tokio::test]
async fn test_ideation_message_post_read_allowed() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_external_session(&state, "proj-rbw-postread", "RBW Post-read").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    let agent_msg = ChatMessage::orchestrator_in_session(session_id.clone(), "agent response");
    let created_agent_msg = state
        .app_state
        .chat_message_repo
        .create(agent_msg)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .update_external_last_read_message_id(&session_id, created_agent_msg.id.as_str())
        .await
        .unwrap();

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            99998,
            "conv2".to_string(),
            "run2".to_string(),
            None,
            None,
        )
        .await;

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "follow-up after reading".to_string(),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "expected Ok after reading, got: {:?}",
        result.err()
    );
    let response = result.unwrap().0;
    assert_eq!(response.status, "queued");
}

#[tokio::test]
async fn test_ideation_message_internal_session_is_now_guarded() {
    // After removing the origin gate, Internal sessions are also subject to the unread guard.
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-rbw-internal", "RBW Internal").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    // Create an unread orchestrator message — cursor is NULL so it counts as unread
    state
        .app_state
        .chat_message_repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "agent response",
        ))
        .await
        .unwrap();

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key.clone(),
            99997,
            "conv3".to_string(),
            "run3".to_string(),
            None,
            None,
        )
        .await;

    // First attempt: should be blocked with 409 because there's an unread message
    let result = ideation_message_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str.clone(),
            message: "internal message".to_string(),
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "internal session should now be guarded when there are unread messages"
    );
    let (status, _) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::CONFLICT);

    // Read messages to advance cursor
    let read_result = get_ideation_messages_http(
        State(state.clone()),
        unrestricted_scope(),
        Path(session_id_str.clone()),
        Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;
    assert!(read_result.is_ok());

    // Second attempt: should succeed now that cursor is up to date
    let result2 = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "internal message after read".to_string(),
        }),
    )
    .await;

    assert!(
        result2.is_ok(),
        "message should be allowed after reading, got: {:?}",
        result2.err()
    );
    let response = result2.unwrap().0;
    assert_eq!(response.status, "queued");
}

#[tokio::test]
async fn test_get_ideation_messages_updates_external_read_cursor() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_external_session(&state, "proj-cursor-update", "Cursor Update").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    let msg = ChatMessage::orchestrator_in_session(session_id.clone(), "agent response");
    let created_msg = state.app_state.chat_message_repo.create(msg).await.unwrap();
    let expected_msg_id = created_msg.id.to_string();

    let result = get_ideation_messages_http(
        State(state.clone()),
        unrestricted_scope(),
        Path(session_id_str),
        Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.messages.len(), 1);

    let updated_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_session.external_last_read_message_id.as_deref(),
        Some(expected_msg_id.as_str()),
        "cursor should be updated to the latest message ID"
    );
}

#[tokio::test]
async fn test_get_ideation_messages_internal_cursor_is_updated() {
    // After removing the origin gate, cursor is updated for Internal sessions too.
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-cursor-internal", "Cursor Internal").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    let msg = ChatMessage::orchestrator_in_session(session_id.clone(), "agent response");
    let created_msg = state.app_state.chat_message_repo.create(msg).await.unwrap();
    let expected_msg_id = created_msg.id.to_string();

    let result = get_ideation_messages_http(
        State(state.clone()),
        unrestricted_scope(),
        Path(session_id_str),
        Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());

    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        session.external_last_read_message_id.as_deref(),
        Some(expected_msg_id.as_str()),
        "internal session cursor must now be updated"
    );
}

#[tokio::test]
async fn test_count_unread_assistant_messages_sql_query() {
    use ralphx_lib::domain::repositories::ChatMessageRepository;
    use ralphx_lib::infrastructure::sqlite::{
        open_connection, run_migrations, SqliteChatMessageRepository,
    };

    let conn = open_connection(&std::path::PathBuf::from(":memory:")).unwrap();
    run_migrations(&conn).unwrap();
    conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();
    let repo = SqliteChatMessageRepository::new(conn);

    let session_id = IdeationSessionId::from_string("sql-test-session".to_string());

    let count = repo
        .count_unread_assistant_messages(session_id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(count, 0, "empty session: 0 unread");

    repo.create(ChatMessage::user_in_session(session_id.clone(), "user msg"))
        .await
        .unwrap();
    let count = repo
        .count_unread_assistant_messages(session_id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(count, 0, "only user message: 0 unread");

    let m1 = repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "agent reply 1",
        ))
        .await
        .unwrap();

    let count = repo
        .count_unread_assistant_messages(session_id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(count, 1, "one orchestrator message: 1 unread");

    let count = repo
        .count_unread_assistant_messages(session_id.as_str(), Some(m1.id.as_str()))
        .await
        .unwrap();
    assert_eq!(count, 0, "cursor at m1: 0 unread");

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let m2 = repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "agent reply 2",
        ))
        .await
        .unwrap();

    let count = repo
        .count_unread_assistant_messages(session_id.as_str(), Some(m1.id.as_str()))
        .await
        .unwrap();
    assert_eq!(count, 1, "cursor at m1, m2 newer: 1 unread");

    let count = repo
        .count_unread_assistant_messages(session_id.as_str(), Some(m2.id.as_str()))
        .await
        .unwrap();
    assert_eq!(count, 0, "cursor at m2: 0 unread");
}

#[tokio::test]
async fn test_start_ideation_returns_next_action_poll_status() {
    let state = setup_test_state().await;

    let project_id = "proj-na-start";
    let p = make_project(project_id, "Next Action Start Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.next_action, "poll_status");
    assert!(response.hint.is_some());
}

#[tokio::test]
async fn test_status_idle_agent_next_action_send_message() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-na-idle", "Idle Agent Status Session").await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "idle");
    assert_eq!(response.next_action, "send_message");
    assert_eq!(response.queued_message_count, 0);
    assert_eq!(response.unread_message_count, 0);
}

#[tokio::test]
async fn test_status_generating_agent_next_action_wait() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(
        &state,
        "proj-na-generating",
        "Generating Agent Status Session",
    )
    .await;

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            0,
            "conv-id-na-gen".to_string(),
            "run-id-na-gen".to_string(),
            None,
            None,
        )
        .await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "generating");
    assert_eq!(response.next_action, "wait");
    let hint = response.hint.as_deref().unwrap_or("");
    assert!(
        hint.contains("5-10s"),
        "generating hint must contain '5-10s', got: {hint}"
    );
}

#[tokio::test]
async fn test_status_waiting_with_unread_next_action_fetch_messages() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-na-unread", "Waiting With Unread Session").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            0,
            "conv-id-na-unread".to_string(),
            "run-id-na-unread".to_string(),
            None,
            None,
        )
        .await;

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("failed to spawn cat for test");
    let stdin = child.stdin.take().expect("no stdin on cat process");

    let ipr_key = InteractiveProcessKey::new("ideation", &session_id_str);
    state
        .app_state
        .interactive_process_registry
        .register(ipr_key, stdin)
        .await;

    create_message(
        &state,
        ChatMessage::orchestrator_in_session(session_id, "Agent has responded with a plan."),
    )
    .await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    drop(child);

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "waiting_for_input");
    assert_eq!(response.unread_message_count, 1);
    assert_eq!(response.next_action, "fetch_messages");
    let hint = response.hint.as_deref().unwrap_or("");
    assert!(
        hint.contains("Fetch messages"),
        "hint must mention fetching messages, got: {hint}"
    );
}

#[tokio::test]
async fn test_status_waiting_no_unread_next_action_send_message() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-na-no-unread", "Waiting No Unread Session").await;

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            0,
            "conv-id-na-no-unread".to_string(),
            "run-id-na-no-unread".to_string(),
            None,
            None,
        )
        .await;

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("failed to spawn cat for test");
    let stdin = child.stdin.take().expect("no stdin on cat process");

    let ipr_key = InteractiveProcessKey::new("ideation", &session_id_str);
    state
        .app_state
        .interactive_process_registry
        .register(ipr_key, stdin)
        .await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    drop(child);

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "waiting_for_input");
    assert_eq!(response.unread_message_count, 0);
    assert_eq!(response.next_action, "send_message");
    let hint = response.hint.as_deref().unwrap_or("");
    assert!(
        hint.contains("ready for input"),
        "hint must mention ready for input, got: {hint}"
    );
}

#[tokio::test]
async fn test_get_ideation_messages_next_action_idle() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-na-msg-idle", "Messages Next Action Idle").await;

    let result = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "idle");
    assert_eq!(response.next_action, "send_message");
}

#[tokio::test]
async fn test_get_ideation_messages_next_action_generating() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-na-msg-gen", "Messages Next Action Generating").await;

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            0,
            "conv-id-na-msg-gen".to_string(),
            "run-id-na-msg-gen".to_string(),
            None,
            None,
        )
        .await;

    let result = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "generating");
    assert_eq!(response.next_action, "wait");
}

#[tokio::test]
async fn test_status_external_activity_phase_none_for_internal_session() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-na-phase", "Internal Session Phase Test").await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(
        response.external_activity_phase.is_none(),
        "internal session must have external_activity_phase: None, got: {:?}",
        response.external_activity_phase
    );
}

#[tokio::test]
async fn test_ideation_message_queue_cap_returns_429_when_full() {
    let state = setup_test_state().await;
    let session_id = create_active_ideation_session(&state).await;

    register_fake_ideation_agent(&state, &session_id).await;

    let cap = 10_usize;
    fill_queue(&state, &session_id, cap);

    assert_eq!(
        state
            .app_state
            .message_queue
            .count_for_context("ideation", &session_id),
        cap
    );

    let req = IdeationMessageRequest {
        session_id: session_id.clone(),
        message: "this should be rejected".to_string(),
    };
    let result = ideation_message_http(State(state), unrestricted_scope(), Json(req)).await;

    assert!(result.is_err(), "expected 429 error when queue is full");
    let (status, Json(body)) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], "queue_full");
    assert_eq!(body["next_action"], "poll_status");
    assert_eq!(body["queued_count"], cap);
    assert!(
        body["hint"].as_str().unwrap().contains("queue is full"),
        "hint should mention queue is full"
    );
}

#[tokio::test]
async fn test_ideation_message_queue_below_cap_accepts_message() {
    let state = setup_test_state().await;
    let session_id = create_active_ideation_session(&state).await;

    register_fake_ideation_agent(&state, &session_id).await;

    let cap = 10_usize;
    fill_queue(&state, &session_id, cap - 1);

    let req = IdeationMessageRequest {
        session_id: session_id.clone(),
        message: "this should be accepted".to_string(),
    };
    let result = ideation_message_http(State(state), unrestricted_scope(), Json(req)).await;

    assert!(result.is_ok(), "expected success when queue is below cap");
    let response = result.unwrap().0;
    assert_eq!(response.status, "queued");
    assert_eq!(response.session_id, session_id);
}

#[tokio::test]
async fn test_ideation_message_no_running_agent_bypasses_queue_cap() {
    let state = setup_test_state().await;
    let session_id = create_active_ideation_session(&state).await;

    fill_queue(&state, &session_id, 20);

    let req = IdeationMessageRequest {
        session_id: session_id.clone(),
        message: "spawned path message".to_string(),
    };
    let result = ideation_message_http(State(state), unrestricted_scope(), Json(req)).await;

    match result {
        Ok(resp) => assert_ne!(resp.status, "429"),
        Err((status, _)) => assert_ne!(
            status,
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "no-agent path must never return 429 from queue cap guard"
        ),
    }
}

#[tokio::test]
async fn test_ideation_message_persists_pending_prompt_when_execution_paused() {
    let state = setup_test_state().await;
    let session_id = create_active_ideation_session(&state).await;
    let message = "Investigate the font scale regression";
    state.execution_state.pause();

    let req = IdeationMessageRequest {
        session_id: session_id.clone(),
        message: message.to_string(),
    };
    let result = ideation_message_http(State(state.clone()), unrestricted_scope(), Json(req)).await;

    assert!(
        result.is_ok(),
        "paused idle ideation message must be accepted durably"
    );
    let response = result.unwrap().0;
    assert_eq!(response.status, "queued");
    assert_eq!(response.session_id, session_id);
    assert_eq!(response.queued_as_pending, Some(true));
    assert_eq!(response.next_action, "wait_for_resume");
    assert_eq!(
        state
            .app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, &response.session_id)
            .len(),
        0,
        "paused idle message must not be stored only in the volatile queue"
    );

    let stored = state
        .app_state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(response.session_id))
        .await
        .unwrap()
        .expect("session should exist");
    assert_eq!(stored.pending_initial_prompt.as_deref(), Some(message));
}

// --- Internal-origin session guard tests ---

#[tokio::test]
async fn test_ideation_message_internal_session_unread_user_message_returns_409() {
    // Proof obligation #1: External agent messaging an Internal session with unread user messages
    // → 409 CONFLICT. User messages from the UI must block the external agent.
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-int-user-msg", "Internal User Msg").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    // Simulate a user message from the UI
    state
        .app_state
        .chat_message_repo
        .create(ChatMessage::user_in_session(
            session_id.clone(),
            "hello from UI",
        ))
        .await
        .unwrap();

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "agent reply".to_string(),
        }),
    )
    .await;

    assert!(result.is_err());
    let (status, body) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    let obj = body.0.as_object().unwrap();
    assert_eq!(obj["error"], "unread_messages");
    assert_eq!(obj["unread_count"], 1);
}

#[tokio::test]
async fn test_ideation_message_internal_empty_session_allows_send() {
    // Proof obligation #6: Empty session (no messages) → allowed regardless of origin.
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-int-empty", "Internal Empty").await;

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "first message".to_string(),
        }),
    )
    .await;

    // Empty session has 0 unread messages, so guard should pass.
    // The send may succeed or fail for other reasons (agent not running etc.) but must NOT be 409.
    match result {
        Ok(_) => {}
        Err((status, _)) => assert_ne!(
            status,
            axum::http::StatusCode::CONFLICT,
            "empty session must not return 409"
        ),
    }
}

#[tokio::test]
async fn test_ideation_message_system_role_does_not_trigger_guard() {
    // Proof obligation / deadlock prevention: System-role messages (invisible to external agents
    // via GET /messages) must NOT count as unread. If they did, agents could never unblock.
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-int-system", "Internal System").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    // Create a system message (invisible to external agents)
    state
        .app_state
        .chat_message_repo
        .create(ChatMessage::system_in_session(
            session_id.clone(),
            "system context injection",
        ))
        .await
        .unwrap();

    let result = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "agent message after system injection".to_string(),
        }),
    )
    .await;

    // System message must NOT trigger the 409 guard.
    match result {
        Ok(_) => {}
        Err((status, _)) => assert_ne!(
            status,
            axum::http::StatusCode::CONFLICT,
            "system-role message must not count as unread"
        ),
    }
}

#[tokio::test]
async fn test_pagination_offset_advances_cursor_to_page_boundary_only() {
    // Proof obligation #7/#8: Reading with offset>0 → cursor only at page boundary.
    // Reading with offset=0 → cursor at session's latest visible message.
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-int-pagination", "Internal Pagination").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    // Create 3 messages with distinct timestamps to ensure ordering
    let msg1 = state
        .app_state
        .chat_message_repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "msg 1 oldest",
        ))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let _msg2 = state
        .app_state
        .chat_message_repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "msg 2 middle",
        ))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let msg3 = state
        .app_state
        .chat_message_repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "msg 3 newest",
        ))
        .await
        .unwrap();

    // Read with offset=1 (skip newest 1, i.e. skip msg3). Gets [msg1, msg2].
    // Cursor advances to msg2 (last of the page). msg3 is still after cursor.
    let _ = get_ideation_messages_http(
        State(state.clone()),
        unrestricted_scope(),
        Path(session_id_str.clone()),
        Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 1,
        }),
    )
    .await
    .unwrap();

    // Verify cursor is not at msg3 yet (it's at msg2, so msg3 is still "unread")
    let session_after_partial_read = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        session_after_partial_read
            .external_last_read_message_id
            .as_deref(),
        Some(msg3.id.as_str()),
        "cursor must not be at msg3 after reading with offset=1"
    );
    // After partial read, msg3 is still unread → guard triggers
    let blocked = ideation_message_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str.clone(),
            message: "should be blocked".to_string(),
        }),
    )
    .await;
    assert!(
        blocked.is_err(),
        "guard should still trigger after offset=1 read, msg3 still unread"
    );
    let (status, _) = blocked.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::CONFLICT);

    // Now read with offset=0 (gets all messages newest-first: msg3, msg2, msg1 → reversed: msg1,msg2,msg3)
    // Cursor advances to msg3 (the last in the response = newest visible).
    let _ = get_ideation_messages_http(
        State(state.clone()),
        unrestricted_scope(),
        Path(session_id_str.clone()),
        Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await
    .unwrap();

    // Cursor should now be at msg3 → 0 unread → send allowed
    let session_after_full_read = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        session_after_full_read
            .external_last_read_message_id
            .as_deref(),
        Some(msg3.id.as_str()),
        "cursor must advance to msg3 (newest) after offset=0 read"
    );

    // Now send is allowed
    let allowed = ideation_message_http(
        State(state),
        unrestricted_scope(),
        Json(IdeationMessageRequest {
            session_id: session_id_str,
            message: "should be allowed now".to_string(),
        }),
    )
    .await;
    match allowed {
        Ok(_) => {}
        Err((status, _)) => assert_ne!(
            status,
            axum::http::StatusCode::CONFLICT,
            "send must be allowed after reading all messages with offset=0"
        ),
    }
    let _ = msg1; // suppress unused warning
}
