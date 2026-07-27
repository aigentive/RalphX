use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use ralphx_lib::application::chat_service::{
    AppChatService, ChatService, ChatServiceError, SendMessageOptions,
};
use ralphx_lib::application::{AppState, InteractiveProcessKey};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use ralphx_lib::domain::entities::ideation::{SessionPurpose, VerificationStatus};
use ralphx_lib::domain::entities::{
    ChatContextType, ChatMessage, IdeationSession, IdeationSessionBuilder, IdeationSessionId,
    InternalStatus, Project, ProjectId, ProjectSkillSettings, Task, TaskOutcomeSource,
    TaskOutcomeStatus, VerificationGap, VerificationRoundSnapshot, VerificationRunSnapshot,
};
use ralphx_lib::domain::execution::ExecutionSettings;
use ralphx_lib::domain::repositories::{TaskOutcomeListOptions, UpsertTaskOutcomeInput};
use ralphx_lib::domain::services::{new_empty_task_outcome, RunningAgentKey};
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::{
    ChildSessionStatusParams, HttpServerState, SendSessionMessageRequest,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path as StdPath, PathBuf},
    sync::Arc,
};
use tokio::io::{AsyncBufReadExt, BufReader};

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());

    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

/// Helper: spawn a `cat` process to get a live ChildStdin for IPR registration.
/// Caller is responsible for killing the child after the test.
async fn spawn_test_stdin_ideation() -> (
    tokio::process::Child,
    tokio::process::ChildStdin,
    tokio::process::ChildStdout,
) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn cat for ideation IPR test");
    let stdin = child.stdin.take().expect("cat stdin handle");
    let stdout = child.stdout.take().expect("cat stdout handle");
    (child, stdin, stdout)
}

/// Helper: default no-op params for get_child_session_status_handler.
fn no_messages_params() -> ChildSessionStatusParams {
    ChildSessionStatusParams {
        include_messages: None,
        message_limit: None,
    }
}

/// Helper: create and persist an Active ideation session.
async fn create_active_session(state: &HttpServerState) -> IdeationSessionId {
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .build();
    let id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    id
}

async fn create_active_session_in_project(
    state: &HttpServerState,
    project_id: ProjectId,
) -> IdeationSessionId {
    let session = IdeationSessionBuilder::new().project_id(project_id).build();
    let id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    id
}

async fn create_active_session_with_purpose(
    state: &HttpServerState,
    purpose: SessionPurpose,
) -> IdeationSessionId {
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .session_purpose(purpose)
        .build();
    let id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    id
}

fn build_ideation_chat_service(state: &HttpServerState) -> AppChatService {
    state
        .app_state
        .build_chat_service_with_execution_state(Arc::clone(&state.execution_state))
        .with_interactive_process_registry(Arc::clone(
            &state.app_state.interactive_process_registry,
        ))
}

struct FakeClaudeCli {
    _temp_dir: tempfile::TempDir,
    path: PathBuf,
}

fn make_fake_claude_cli() -> FakeClaudeCli {
    let temp_dir = tempfile::Builder::new()
        .prefix("ralphx-fake-claude-")
        .tempdir()
        .expect("create fake Claude CLI temp dir");
    let path = temp_dir.path().join("claude");
    fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write fake Claude CLI");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&path)
            .expect("read fake Claude CLI metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("mark fake Claude CLI executable");
    }

    FakeClaudeCli {
        _temp_dir: temp_dir,
        path,
    }
}

async fn configure_fake_claude_cli(state: &HttpServerState, cli_path: &StdPath) {
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
        .expect("seed fake Claude provider settings");
}

#[tokio::test]
async fn test_get_child_session_status_likely_generating() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let key = RunningAgentKey::new("session", &sid_str);
    state
        .app_state
        .running_agent_registry
        .register(
            key.clone(),
            99999,
            "test-conv".to_string(),
            "test-run".to_string(),
            None,
            None,
        )
        .await;
    state
        .app_state
        .running_agent_registry
        .update_heartbeat(&key, "test-run", chrono::Utc::now())
        .await
        .expect("matching agent run must accept heartbeat");

    let result =
        get_child_session_status_handler(State(state), Path(sid_str), Query(no_messages_params()))
            .await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    let resp = result.unwrap().0;
    assert!(resp.agent_state.is_running, "agent must be running");
    assert_eq!(
        resp.agent_state.estimated_status, "likely_generating",
        "recent heartbeat must yield likely_generating"
    );
}

#[tokio::test]
async fn test_get_child_session_status_likely_waiting() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let key = RunningAgentKey::new("ideation", &sid_str);
    state
        .app_state
        .running_agent_registry
        .register(
            key.clone(),
            99998,
            "test-conv-2".to_string(),
            "test-run-2".to_string(),
            None,
            None,
        )
        .await;
    let stale = chrono::Utc::now() - chrono::Duration::seconds(1000);
    state
        .app_state
        .running_agent_registry
        .update_heartbeat(&key, "test-run-2", stale)
        .await
        .expect("matching agent run must accept heartbeat");

    let result =
        get_child_session_status_handler(State(state), Path(sid_str), Query(no_messages_params()))
            .await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    let resp = result.unwrap().0;
    assert!(resp.agent_state.is_running, "agent must be running");
    assert_eq!(
        resp.agent_state.estimated_status, "likely_waiting",
        "stale heartbeat (1000s) must yield likely_waiting"
    );
}

#[tokio::test]
async fn test_get_child_session_status_idle() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let result =
        get_child_session_status_handler(State(state), Path(sid_str), Query(no_messages_params()))
            .await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    let resp = result.unwrap().0;
    assert!(!resp.agent_state.is_running, "agent must not be running");
    assert_eq!(resp.agent_state.estimated_status, "idle");
    assert!(resp.agent_state.pid.is_none());
    assert!(resp.agent_state.last_active_at.is_none());
}

#[tokio::test]
async fn test_get_child_session_status_include_messages_truncated() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let long_content = "A".repeat(700);
    let msg = ChatMessage::user_in_session(session_id.clone(), long_content.clone());
    state.app_state.chat_message_repo.create(msg).await.unwrap();

    let params = ChildSessionStatusParams {
        include_messages: Some(true),
        message_limit: Some(5),
    };

    let result = get_child_session_status_handler(State(state), Path(sid_str), Query(params)).await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    let resp = result.unwrap().0;
    let messages = resp.recent_messages.expect("messages must be returned");
    assert_eq!(messages.len(), 1, "one message created");
    assert_eq!(
        messages[0].content.chars().count(),
        500,
        "content must be truncated to 500 chars"
    );
    assert_eq!(messages[0].role, "user");
}

#[tokio::test]
async fn test_get_child_session_status_not_found_returns_404() {
    let state = setup_test_state().await;

    let result = get_child_session_status_handler(
        State(state),
        Path("non-existent-session-id".to_string()),
        Query(no_messages_params()),
    )
    .await;

    assert!(result.is_err(), "expected Err for missing session");
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "must return 404 for missing session"
    );
}

#[tokio::test]
async fn test_get_child_session_status_message_limit_clamped_to_50() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    for i in 0..60 {
        let msg = ChatMessage::user_in_session(session_id.clone(), format!("Message {}", i));
        state.app_state.chat_message_repo.create(msg).await.unwrap();
    }

    let params = ChildSessionStatusParams {
        include_messages: Some(true),
        message_limit: Some(10000),
    };

    let result = get_child_session_status_handler(State(state), Path(sid_str), Query(params)).await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    let messages = result
        .unwrap()
        .0
        .recent_messages
        .expect("messages must be returned");
    assert!(
        messages.len() <= 50,
        "message_limit=10000 must be clamped to 50, got {}",
        messages.len()
    );
}

#[tokio::test]
async fn test_get_child_session_status_heartbeat_at_exact_threshold_is_likely_waiting() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let key = RunningAgentKey::new("session", &sid_str);
    state
        .app_state
        .running_agent_registry
        .register(
            key.clone(),
            99997,
            "test-conv-3".to_string(),
            "test-run-3".to_string(),
            None,
            None,
        )
        .await;

    let default_threshold_secs: i64 = 10;
    let at_boundary = chrono::Utc::now() - chrono::Duration::seconds(default_threshold_secs);
    state
        .app_state
        .running_agent_registry
        .update_heartbeat(&key, "test-run-3", at_boundary)
        .await
        .expect("matching agent run must accept heartbeat");

    let result =
        get_child_session_status_handler(State(state), Path(sid_str), Query(no_messages_params()))
            .await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    let resp = result.unwrap().0;
    assert_eq!(
        resp.agent_state.estimated_status, "likely_waiting",
        "heartbeat at exact threshold boundary must yield likely_waiting (elapsed >= threshold)"
    );
}

#[tokio::test]
async fn test_get_child_session_status_native_verification_snapshot_populated() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .verification_generation(2)
        .build();
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id_obj,
            &VerificationRunSnapshot {
                generation: 2,
                status: VerificationStatus::Reviewing,
                in_progress: true,
                current_round: 2,
                max_rounds: 5,
                best_round_index: Some(2),
                convergence_reason: None,
                current_gaps: vec![],
                rounds: vec![
                    VerificationRoundSnapshot {
                        round: 1,
                        gap_score: 7,
                        fingerprints: vec!["fp-1".to_string()],
                        gaps: vec![],
                        parse_failed: false,
                    },
                    VerificationRoundSnapshot {
                        round: 2,
                        gap_score: 3,
                        fingerprints: vec!["fp-2".to_string()],
                        gaps: vec![],
                        parse_failed: false,
                    },
                ],
            },
        )
        .await
        .unwrap();

    let result = get_child_session_status_handler(
        State(state),
        Path(session_id),
        Query(no_messages_params()),
    )
    .await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    let resp = result.unwrap().0;
    let verification = resp
        .verification
        .expect("verification must be populated for non-Unverified status");
    assert_eq!(verification.status, "reviewing");
    assert_eq!(verification.generation, 2);
    assert_eq!(
        verification.current_round,
        Some(2),
        "current_round=2 from native snapshot"
    );
    assert_eq!(
        verification.gap_score,
        Some(3),
        "gap_score must come from last round (index 1, score=3)"
    );
}

#[tokio::test]
async fn test_get_child_session_status_prefers_native_verification_snapshot() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .verification_generation(2)
        .build();
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id_obj,
            &VerificationRunSnapshot {
                generation: 2,
                status: VerificationStatus::Reviewing,
                in_progress: true,
                current_round: 2,
                max_rounds: 5,
                best_round_index: Some(2),
                convergence_reason: None,
                current_gaps: vec![VerificationGap {
                    severity: "high".to_string(),
                    category: "completeness".to_string(),
                    description: "Missing registration".to_string(),
                    why_it_matters: Some("Migration never executes".to_string()),
                    source: Some("completeness".to_string()),
                }],
                rounds: vec![
                    VerificationRoundSnapshot {
                        round: 1,
                        gap_score: 7,
                        fingerprints: vec!["fp-1".to_string()],
                        gaps: vec![],
                        parse_failed: false,
                    },
                    VerificationRoundSnapshot {
                        round: 2,
                        gap_score: 3,
                        fingerprints: vec!["fp-2".to_string()],
                        gaps: vec![],
                        parse_failed: false,
                    },
                ],
            },
        )
        .await
        .unwrap();

    let result = get_child_session_status_handler(
        State(state),
        Path(session_id),
        Query(no_messages_params()),
    )
    .await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    let resp = result.unwrap().0;
    let verification = resp
        .verification
        .expect("verification must be populated for non-Unverified status");
    assert_eq!(verification.status, "reviewing");
    assert_eq!(verification.generation, 2);
    assert_eq!(verification.current_round, Some(2));
    assert_eq!(verification.gap_score, Some(3));
}

#[tokio::test]
async fn test_get_child_session_status_without_native_snapshot_returns_empty_verification_detail() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = get_child_session_status_handler(
        State(state),
        Path(session_id),
        Query(no_messages_params()),
    )
    .await;

    assert!(
        result.is_ok(),
        "missing native snapshot must not cause 500: {:?}",
        result.err()
    );
    let resp = result.unwrap().0;
    let verification = resp
        .verification
        .expect("VerificationInfo present for non-Unverified status");
    assert_eq!(verification.status, "reviewing");
    assert!(
        verification.gap_score.is_none(),
        "without a native snapshot gap_score must be None"
    );
    assert!(
        verification.current_round.is_none(),
        "without a native snapshot current_round must be None"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_interactive_session_key_sent() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();
    let message = "Hello agent";

    let (mut child, stdin, stdout) = spawn_test_stdin_ideation().await;
    let ipr_key = InteractiveProcessKey::new("session", &sid_str);
    state
        .app_state
        .interactive_process_registry
        .register(ipr_key, stdin)
        .await;

    let result = send_ideation_session_message_handler(
        State(state),
        Path(sid_str.clone()),
        Json(SendSessionMessageRequest {
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

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    assert_eq!(result.unwrap().0.delivery_status, "sent");
    let payload: serde_json::Value = serde_json::from_str(written.trim_end()).expect("valid JSON");
    assert_eq!(payload["type"], "user");
    assert_eq!(payload["message"]["role"], "user");
    let content = payload["message"]["content"]
        .as_str()
        .expect("content string");
    assert!(
        content.contains(&format!("<context_id>{sid_str}</context_id>")),
        "content must include ideation context wrapper: {content}"
    );
    assert!(
        content.contains(&format!("<user_message>{message}</user_message>")),
        "content must include wrapped user message: {content}"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_interactive_ideation_key_sent() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();
    let message = "Nudge from orchestrator";

    let (mut child, stdin, stdout) = spawn_test_stdin_ideation().await;
    let ipr_key = InteractiveProcessKey::new("ideation", &sid_str);
    state
        .app_state
        .interactive_process_registry
        .register(ipr_key, stdin)
        .await;

    let result = send_ideation_session_message_handler(
        State(state),
        Path(sid_str.clone()),
        Json(SendSessionMessageRequest {
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

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    assert_eq!(result.unwrap().0.delivery_status, "sent");
    let payload: serde_json::Value = serde_json::from_str(written.trim_end()).expect("valid JSON");
    assert_eq!(payload["type"], "user");
    assert_eq!(payload["message"]["role"], "user");
    let content = payload["message"]["content"]
        .as_str()
        .expect("content string");
    assert!(
        content.contains(&format!("<context_id>{sid_str}</context_id>")),
        "content must include ideation context wrapper: {content}"
    );
    assert!(
        content.contains(&format!("<user_message>{message}</user_message>")),
        "content must include wrapped user message: {content}"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_running_session_key_queued() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let agent_key = RunningAgentKey::new("session", &sid_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            88888,
            "test-conv-q".to_string(),
            "test-run-q".to_string(),
            None,
            None,
        )
        .await;

    let result = send_ideation_session_message_handler(
        State(state),
        Path(sid_str),
        Json(SendSessionMessageRequest {
            message: "Queue this message".to_string(),
        }),
    )
    .await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    assert_eq!(
        result.unwrap().0.delivery_status,
        "queued",
        "running agent without IPR → message must be queued"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_running_ideation_key_queued() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let agent_key = RunningAgentKey::new("ideation", &sid_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            77777,
            "test-conv-iq".to_string(),
            "test-run-iq".to_string(),
            None,
            None,
        )
        .await;

    let result = send_ideation_session_message_handler(
        State(state),
        Path(sid_str),
        Json(SendSessionMessageRequest {
            message: "Queue via ideation key".to_string(),
        }),
    )
    .await;

    assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    assert_eq!(
        result.unwrap().0.delivery_status,
        "queued",
        "running agent under ideation key without IPR → message must be queued"
    );
}

#[tokio::test]
async fn test_chat_service_blocks_new_ideation_spawn_when_global_ideation_cap_reached() {
    let state = setup_test_state().await;
    let occupied_session_id = create_active_session(&state).await;
    let target_session_id = create_active_session(&state).await;

    state.execution_state.set_global_max_concurrent(5);
    state.execution_state.set_global_ideation_max(1);

    let occupied_key = RunningAgentKey::new("ideation", occupied_session_id.as_str());
    state
        .app_state
        .running_agent_registry
        .register(
            occupied_key,
            66666,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;

    let chat_service = build_ideation_chat_service(&state);
    let result = chat_service
        .send_message(
            ChatContextType::Ideation,
            target_session_id.as_str(),
            "Start ideation",
            SendMessageOptions::default(),
        )
        .await;

    let queued = result.expect("ideation cap should queue the prompt");
    assert!(
        queued.was_queued && queued.queued_as_pending,
        "unexpected queued result: {queued:?}"
    );

    let target_key = RunningAgentKey::new("ideation", target_session_id.as_str());
    assert!(
        !state
            .app_state
            .running_agent_registry
            .is_running(&target_key)
            .await,
        "failed admission must not leave a registered running-agent slot behind"
    );
}

#[tokio::test]
async fn test_verification_child_session_counts_against_ideation_cap() {
    let state = setup_test_state().await;
    let verification_child_id =
        create_active_session_with_purpose(&state, SessionPurpose::Verification).await;
    let target_session_id = create_active_session(&state).await;

    state.execution_state.set_global_max_concurrent(5);
    state.execution_state.set_global_ideation_max(1);

    let occupied_key = RunningAgentKey::new("ideation", verification_child_id.as_str());
    state
        .app_state
        .running_agent_registry
        .register(
            occupied_key,
            55555,
            "verification-conv".to_string(),
            "verification-run".to_string(),
            None,
            None,
        )
        .await;

    let chat_service = build_ideation_chat_service(&state);
    let result = chat_service
        .send_message(
            ChatContextType::Ideation,
            target_session_id.as_str(),
            "Start ideation after verification child",
            SendMessageOptions::default(),
        )
        .await;

    let queued = result.expect("verification child should force queueing");
    assert!(
        queued.was_queued && queued.queued_as_pending,
        "unexpected queued result: {queued:?}"
    );
}

#[tokio::test]
async fn test_project_ideation_cap_blocks_same_project_spawn() {
    let state = setup_test_state().await;
    let project = Project::new("Project Cap".to_string(), "/tmp/project-cap".to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    state
        .app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let occupied_session_id = create_active_session_in_project(&state, project.id.clone()).await;
    let target_session_id = create_active_session_in_project(&state, project.id.clone()).await;

    state.execution_state.set_global_max_concurrent(5);
    state.execution_state.set_global_ideation_max(5);

    state
        .app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied_session_id.as_str()),
            44444,
            "project-cap-conv".to_string(),
            "project-cap-run".to_string(),
            None,
            None,
        )
        .await;

    let chat_service = build_ideation_chat_service(&state);
    let result = chat_service
        .send_message(
            ChatContextType::Ideation,
            target_session_id.as_str(),
            "Start same-project ideation",
            SendMessageOptions::default(),
        )
        .await;

    let queued = result.expect("project ideation cap should queue the prompt");
    assert!(
        queued.was_queued && queued.queued_as_pending,
        "unexpected queued result: {queued:?}"
    );
}

#[tokio::test]
async fn test_borrowing_stays_blocked_when_ready_execution_waits() {
    let state = setup_test_state().await;
    let project = Project::new("Borrow Block".to_string(), "/tmp/borrow-block".to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let occupied_session_id = create_active_session_in_project(&state, project.id.clone()).await;
    let target_session_id = create_active_session_in_project(&state, project.id.clone()).await;

    let mut ready_task = Task::new(project.id.clone(), "Ready execution".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    state.app_state.task_repo.create(ready_task).await.unwrap();

    state.execution_state.set_global_max_concurrent(5);
    state.execution_state.set_global_ideation_max(1);
    state
        .execution_state
        .set_allow_ideation_borrow_idle_execution(true);

    state
        .app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied_session_id.as_str()),
            33333,
            "borrow-block-conv".to_string(),
            "borrow-block-run".to_string(),
            None,
            None,
        )
        .await;

    let chat_service = build_ideation_chat_service(&state);
    let result = chat_service
        .send_message(
            ChatContextType::Ideation,
            target_session_id.as_str(),
            "Start ideation while execution waits",
            SendMessageOptions::default(),
        )
        .await;

    let queued = result.expect("ready execution work should queue ideation");
    assert!(
        queued.was_queued && queued.queued_as_pending,
        "unexpected queued result: {queued:?}"
    );
}

#[tokio::test]
async fn test_chat_service_spawn_blocked_in_test_mode() {
    let state = setup_test_state().await;
    let fake_cli = make_fake_claude_cli();
    configure_fake_claude_cli(&state, &fake_cli.path).await;
    let project = Project::new(
        "Spawn Blocked".to_string(),
        fake_cli
            .path
            .parent()
            .expect("fake CLI must have a parent directory")
            .to_string_lossy()
            .to_string(),
    );
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("project must be persisted for lane resolution");
    let session_id = create_active_session_in_project(&state, project.id).await;

    let chat_service = build_ideation_chat_service(&state).with_cli_path(fake_cli.path.clone());
    let result = chat_service
        .send_message(
            ChatContextType::Ideation,
            session_id.as_str(),
            "Spawn me an agent",
            SendMessageOptions::default(),
        )
        .await;

    let err = result.expect_err("test mode must fail closed on real Claude spawn");
    assert!(
        matches!(err, ChatServiceError::SpawnFailed(ref msg) if msg.contains("disabled in tests")),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn test_chat_service_persists_idle_ideation_message_when_execution_paused() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;

    state.execution_state.pause();

    let chat_service = build_ideation_chat_service(&state);
    let result = chat_service
        .send_message(
            ChatContextType::Ideation,
            session_id.as_str(),
            "Queue during pause",
            SendMessageOptions::default(),
        )
        .await
        .expect("paused ideation send should be deferred durably");

    assert!(
        result.was_queued && result.queued_as_pending,
        "paused idle ideation send must persist as pending rather than volatile queue"
    );
    assert_eq!(
        state
            .app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, session_id.as_str())
            .len(),
        0,
        "idle ideation prompt must not enter the volatile in-memory queue while paused"
    );

    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("session must exist");
    assert_eq!(
        session.pending_initial_prompt.as_deref(),
        Some("Queue during pause"),
        "paused idle ideation prompt must survive restart via pending_initial_prompt"
    );

    let key = RunningAgentKey::new("ideation", session_id.as_str());
    assert!(
        !state
            .app_state
            .running_agent_registry
            .is_running(&key)
            .await,
        "paused ideation send must not register a running agent"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_agent_idle_spawn_blocked_in_test_mode() {
    let state = setup_test_state().await;
    let fake_cli = make_fake_claude_cli();
    configure_fake_claude_cli(&state, &fake_cli.path).await;

    let mut session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .build();
    session.status = ralphx_lib::domain::entities::ideation::IdeationSessionStatus::Active;
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = send_ideation_session_message_handler(
        State(state),
        Path(session_id),
        Json(SendSessionMessageRequest {
            message: "Spawn me an agent".to_string(),
        }),
    )
    .await;

    assert!(result.is_err(), "test mode must block real Claude spawn");
    let (status, _body) = result.unwrap_err();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_send_ideation_session_message_archived_session_returns_422() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .status(ralphx_lib::domain::entities::ideation::IdeationSessionStatus::Archived)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = send_ideation_session_message_handler(
        State(state),
        Path(session_id),
        Json(SendSessionMessageRequest {
            message: "Hello".to_string(),
        }),
    )
    .await;

    assert!(result.is_err(), "Archived session must be rejected");
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Archived session → 422"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_accepted_session_returns_422() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .status(ralphx_lib::domain::entities::ideation::IdeationSessionStatus::Accepted)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = send_ideation_session_message_handler(
        State(state),
        Path(session_id),
        Json(SendSessionMessageRequest {
            message: "Hello".to_string(),
        }),
    )
    .await;

    assert!(result.is_err(), "Accepted session must be rejected");
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Accepted session → 422"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_empty_message_returns_422() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let result = send_ideation_session_message_handler(
        State(state),
        Path(sid_str),
        Json(SendSessionMessageRequest {
            message: String::new(),
        }),
    )
    .await;

    assert!(result.is_err(), "empty message must be rejected");
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "empty message → 422"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_too_long_returns_422() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let huge_message = "X".repeat(10_001);

    let result = send_ideation_session_message_handler(
        State(state),
        Path(sid_str),
        Json(SendSessionMessageRequest {
            message: huge_message,
        }),
    )
    .await;

    assert!(result.is_err(), "message >10000 chars must be rejected");
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "too-long message → 422"
    );
}

#[tokio::test]
async fn test_send_ideation_session_message_send_error_returns_500_in_test_mode() {
    let state = setup_test_state().await;
    let session_id = create_active_session(&state).await;
    let sid_str = session_id.as_str().to_string();

    let result = send_ideation_session_message_handler(
        State(state),
        Path(sid_str),
        Json(SendSessionMessageRequest {
            message: "Trigger spawn failure".to_string(),
        }),
    )
    .await;

    assert!(result.is_err(), "test mode must block real Claude spawn");
    let (status, _body) = result.unwrap_err();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

fn recurrence_key_for_gap(description: &str) -> String {
    let tokens = description
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let canonical = tokens.into_iter().collect::<Vec<_>>().join("\n");
    format!("token-set-v1:{:x}", Sha256::digest(canonical.as_bytes()))
}

async fn seed_recurrence_corpus(
    state: &HttpServerState,
    project_id: &ProjectId,
    key: &str,
    sessions: &[&str],
) {
    for (index, session) in sessions.iter().enumerate() {
        let mut evidence = new_empty_task_outcome(
            project_id.clone(),
            if index % 2 == 0 {
                TaskOutcomeSource::Review
            } else {
                TaskOutcomeSource::MergeValidation
            },
            "fixture",
            format!("corpus-{index}"),
        );
        evidence.status = TaskOutcomeStatus::Failed;
        evidence.evidence_json = serde_json::json!({
            "recurrence_key": key,
            "recurrence_session": session,
        });
        state
            .app_state
            .task_outcome_repo
            .upsert(UpsertTaskOutcomeInput { outcome: evidence })
            .await
            .unwrap();
    }
}

async fn post_missing_import_gap(state: &HttpServerState, session_id: &str, round: u32) {
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.to_string()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(round),
            gaps: Some(vec![VerificationGapRequest {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: "Missing regression tests for the import path".to_string(),
                why_it_matters: Some("The same bug can reappear silently".to_string()),
                source: Some("layer2".to_string()),
            }]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "round {round} verification update should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_post_verification_status_records_recurring_gap_outcome() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    let session_id_str = session_id.as_str().to_string();

    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    let mut settings = ProjectSkillSettings::default_for_project(project_id.clone());
    settings.verification_corpus_gate = 2;
    state
        .app_state
        .project_skill_settings_repo
        .upsert(settings)
        .await
        .unwrap();
    let key = recurrence_key_for_gap("Missing regression tests for the import path");
    seed_recurrence_corpus(
        &state,
        &project_id,
        &key,
        &["trusted-session-1", "trusted-session-2"],
    )
    .await;

    for round in [1, 2] {
        post_missing_import_gap(&state, &session_id_str, round).await;
    }

    let outcomes = state
        .app_state
        .task_outcome_repo
        .list_by_project(&project_id, TaskOutcomeListOptions::default())
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 3);
    let outcome = outcomes
        .iter()
        .find(|outcome| outcome.source == TaskOutcomeSource::Verification)
        .expect("recurring verification outcome");
    assert_eq!(outcome.source.as_str(), "verification");
    assert_eq!(outcome.source_ref_kind, "gap_recurrence");
    assert_eq!(
        outcome.outcome_class.as_ref().map(|class| class.as_str()),
        Some("verification_gap_recurring")
    );
    assert_eq!(outcome.status, TaskOutcomeStatus::Eligible);
    assert_eq!(
        outcome.verification_id.as_deref(),
        Some(session_id_str.as_str())
    );
    assert_eq!(
        outcome.evidence_json["eligible_observations"].as_u64(),
        Some(2)
    );
    assert_eq!(outcome.evidence_json["distinct_sessions"].as_u64(), Some(2));
    assert_eq!(outcome.evidence_json["recurrence_key"], key);

    let batch = state
        .app_state
        .project_skill_evidence_batch_repo
        .get_by_outcome_id(&project_id, &outcome.id)
        .await
        .unwrap()
        .expect("recurrence evidence batch");
    assert_eq!(batch.items.len(), 1);
    assert!(batch.items[0]
        .digest
        .starts_with(&format!("recurrence_key={key}\n")));
}

#[tokio::test]
async fn test_post_verification_status_requires_enabled_gate_and_two_distinct_sessions() {
    let state = setup_test_state().await;
    let key = recurrence_key_for_gap("Missing regression tests for the import path");

    let disabled_project = ProjectId::new();
    let disabled_session = IdeationSession::new(disabled_project.clone());
    let disabled_session_id = disabled_session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(disabled_session)
        .await
        .unwrap();
    seed_recurrence_corpus(
        &state,
        &disabled_project,
        &key,
        &["disabled-session-1", "disabled-session-2"],
    )
    .await;
    post_missing_import_gap(&state, &disabled_session_id, 1).await;
    let disabled_outcomes = state
        .app_state
        .task_outcome_repo
        .list_by_project(&disabled_project, TaskOutcomeListOptions::default())
        .await
        .unwrap();
    assert!(!disabled_outcomes
        .iter()
        .any(|outcome| outcome.source == TaskOutcomeSource::Verification));

    let single_session_project = ProjectId::new();
    let single_session = IdeationSession::new(single_session_project.clone());
    let single_session_id = single_session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(single_session)
        .await
        .unwrap();
    let mut settings = ProjectSkillSettings::default_for_project(single_session_project.clone());
    settings.verification_corpus_gate = 1;
    state
        .app_state
        .project_skill_settings_repo
        .upsert(settings)
        .await
        .unwrap();
    seed_recurrence_corpus(
        &state,
        &single_session_project,
        &key,
        &["same-session", "same-session"],
    )
    .await;
    post_missing_import_gap(&state, &single_session_id, 1).await;
    let single_session_outcomes = state
        .app_state
        .task_outcome_repo
        .list_by_project(&single_session_project, TaskOutcomeListOptions::default())
        .await
        .unwrap();
    assert!(!single_session_outcomes
        .iter()
        .any(|outcome| outcome.source == TaskOutcomeSource::Verification));
}
