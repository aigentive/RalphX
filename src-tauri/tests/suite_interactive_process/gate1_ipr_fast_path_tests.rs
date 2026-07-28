// Gate 1 IPR Fast-Path Integration Tests
//
// Tests the Gate 1 interactive process fast-path logic in send_message (mod.rs).
// When an interactive process is registered in IPR for a context, send_message
// should write to the existing process's stdin, reuse the existing conversation,
// and return the same conversation_id — NOT spawn a new process.
//
// These tests exercise the production AppChatService::send_message path with a
// live stdin observer, alongside focused component-contract coverage.

use chrono::Utc;
use ralphx_events::RecordingEventSink;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

use ralphx_lib::application::chat_service::{ChatService, ChatServiceError, SendMessageOptions};
use ralphx_lib::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata, InteractiveProcessRegistry,
};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::ProviderSessionRef;
use ralphx_lib::domain::entities::{
    AgentRun, ChatContextType, ChatConversation, Persona, PersonaId, PersonaStatus, Project,
    ProjectId, TaskId,
};
use ralphx_lib::domain::repositories::ChatConversationRepository;
use ralphx_lib::domain::services::running_agent_registry::{
    MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry,
};
use ralphx_lib::domain::services::QueueKey;
use ralphx_lib::infrastructure::memory::MemoryChatConversationRepository;

use crate::support::erroring_persona_repository::ErroringPersonaRepository;
use crate::support::failing_chat_message_repository::{
    FailingChatMessageRepository, CHAT_MESSAGE_CREATE_FAILURE,
};

fn active_persona(id: &str, content: &str, content_hash: &str) -> Persona {
    Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: None,
        slug: id.to_string(),
        name: id.to_string(),
        description: "Gate-1 test persona".to_string(),
        content: content.to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: content_hash.to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

async fn seed_project_context(state: &AppState, context_id: &str) -> tempfile::TempDir {
    let project_dir = tempfile::tempdir().expect("temp project dir");
    let mut project = Project::new(
        format!("Gate-1 test project {context_id}"),
        project_dir.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string(context_id.to_string());
    state
        .project_repo
        .create(project)
        .await
        .expect("seed project context");
    project_dir
}

fn write_capturing_claude_cli(temp: &Path, capture: &Path) -> PathBuf {
    let cli_path = temp.join("capturing-claude");
    fs::write(
        &cli_path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nfor arg in \"$@\"; do\n  [ -f \"$arg\" ] && cat \"$arg\" >> '{}'\ndone\nsleep 1\n",
            capture.display(),
            capture.display(),
        ),
    )
    .expect("write capturing Claude CLI");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&cli_path)
            .expect("capturing Claude CLI metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&cli_path, permissions).expect("mark capturing Claude CLI executable");
    }
    cli_path
}

fn runtime_plugin_dir_for_gate1_persona_test() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../plugins/app")
        .canonicalize()
        .expect("runtime plugins/app directory should resolve to a canonical path")
}

fn configure_runtime_plugin_dirs_for_gate1_persona_test() -> (
    ralphx_lib::infrastructure::agents::claude::RuntimePluginDirsOverrideGuard,
    tempfile::TempDir,
) {
    let generated_plugin_root = tempfile::tempdir().expect("generated plugin tempdir");
    let generated_plugin_dir = generated_plugin_root.path().join("generated/claude-plugin");
    fs::create_dir_all(&generated_plugin_dir).expect("create generated plugin dir");
    let runtime_plugin_guard =
        ralphx_lib::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            runtime_plugin_dir_for_gate1_persona_test(),
            generated_plugin_dir,
        );

    (runtime_plugin_guard, generated_plugin_root)
}

async fn setup_live_project_continuation(
    state: &AppState,
    context_id: &str,
    completed_runtime: Option<(&str, &str)>,
) -> (
    tempfile::TempDir,
    ralphx_lib::domain::entities::ChatConversationId,
    String,
    InteractiveProcessKey,
    tokio::process::Child,
) {
    let project_dir = seed_project_context(state, context_id).await;
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    if completed_runtime.is_some() {
        conversation.set_provider_session_ref(ProviderSessionRef {
            harness: ralphx_lib::domain::agents::AgentHarnessKind::Claude,
            provider_session_id: "gate1-provider-session".to_string(),
        });
    }
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist agent conversation");

    if let Some((logical_model, effective_model_id)) = completed_runtime {
        let mut completed_run = AgentRun::new(conversation_id);
        completed_run.complete();
        completed_run.harness = Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude);
        completed_run.provider_session_id = Some("gate1-provider-session".to_string());
        completed_run.logical_model = Some(logical_model.to_string());
        completed_run.effective_model_id = Some(effective_model_id.to_string());
        state
            .agent_run_repo
            .create(completed_run)
            .await
            .expect("persist completed continuation runtime");
    }

    let run = AgentRun::new(conversation_id);
    let run_id = run.id.as_str().to_string();
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("persist live run");

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn Claude stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", conversation_id.as_str());
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("observer stdin"),
            InteractiveProcessMetadata {
                agent_run_id: Some(run_id.clone()),
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                ..Default::default()
            },
        )
        .await;
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", conversation_id.as_str()),
            0,
            conversation_id.as_str().to_string(),
            run_id.clone(),
            None,
            None,
        )
        .await;

    (project_dir, conversation_id, run_id, interactive_key, child)
}

async fn seed_live_run_owner(
    state: &AppState,
    context_id: &str,
    conversation_id: ralphx_lib::domain::entities::ChatConversationId,
) -> String {
    let run = AgentRun::new(conversation_id);
    let run_id = run.id.as_str().to_string();
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("persist live run owner");
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", context_id),
            0,
            conversation_id.as_str().to_string(),
            run_id.clone(),
            None,
            None,
        )
        .await;
    run_id
}

// ============================================================================
// Test 1: Gate 1 HIT — IPR has entry, writes to stdin, reuses existing conversation
// ============================================================================

#[tokio::test]
async fn gate1_persona_resolution_failure_blocks_before_stdin_write() {
    let state = AppState::new_test();
    let context_id = "project-gate1-persona-fail-closed";
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some("persona-gate1-error".to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist bound Gate-1 conversation");

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let stdin = child.stdin.take().expect("cat stdin");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register(interactive_key.clone(), stdin)
        .await;

    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::new(ErroringPersonaRepository))
        .with_persona_feature_enabled(true);
    let error = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "must not reach stdin",
            SendMessageOptions::default(),
        )
        .await
        .expect_err("Gate-1 persona resolution failure must block the stdin write");

    assert!(
        error.to_string().contains("persona repository exploded"),
        "Gate-1 must return the typed persona failure: {error}"
    );
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await,
        "fail-closed resolution must leave the live IPR entry intact before any write"
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed_stdin = Vec::new();
    child
        .stdout
        .take()
        .expect("cat stdout")
        .read_to_end(&mut observed_stdin)
        .await
        .expect("read stdin observer output");
    let _ = child.wait().await;
    assert!(
        observed_stdin.is_empty(),
        "persona failure must happen before Gate-1 writes the stream-json payload"
    );
}

#[tokio::test]
async fn gate1_stdin_reuse_resolves_persona_and_compares_before_stdin_write() {
    let state = AppState::new_test();
    let context_id = "project-gate1-persona-compare";
    let _project_dir = seed_project_context(&state, context_id).await;
    let persona = active_persona("persona-gate1-compare", "persona body", "new-hash");
    state.persona_repo.create(persona.clone()).await.unwrap();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("cat stdin"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                provider_session_id: Some("preserved-session".to_string()),
                persona_id: Some(persona.id.to_string()),
                persona_content_hash: Some("old-hash".to_string()),
            },
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true)
        .with_cli_path(std::env::temp_dir().join("missing-gate1-persona-cli"));

    let error = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "must not reach stale stdin",
            SendMessageOptions::default(),
        )
        .await
        .expect_err("persona mismatch must bypass stdin and reach fresh-spawn validation");

    assert!(
        error.to_string().contains("missing-gate1-persona-cli"),
        "a metadata match would have written stdin instead of attempting fresh spawn: {error}"
    );
    assert!(
        !state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await,
        "the stale persona-bound IPR entry must be removed before fresh spawn"
    );
    let mut observed_stdin = Vec::new();
    child
        .stdout
        .take()
        .expect("cat stdout")
        .read_to_end(&mut observed_stdin)
        .await
        .expect("read stdin observer output");
    let _ = child.wait().await;
    assert!(
        observed_stdin.is_empty(),
        "persona mismatch must compare before any Gate-1 stdin write"
    );
}

#[tokio::test]
async fn edit_while_idle_respawns_with_new_persona_and_preserved_provider_session() {
    // provider_resume_mode_for_session requires an on-disk session artifact,
    // otherwise the respawn takes Recovery mode and omits --resume.
    let provider_home = tempfile::tempdir().expect("provider state home");
    let session_dir = provider_home.path().join(".claude/projects/gate1");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    std::fs::write(session_dir.join("preserved-provider-session.jsonl"), "{}\n")
        .expect("session artifact");
    let _provider_home_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_PROVIDER_STATE_HOME_OVERRIDE",
        provider_home.path().as_os_str(),
    );
    let _allow_spawn =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (_runtime_plugin_guard, _generated_plugin_root) =
        configure_runtime_plugin_dirs_for_gate1_persona_test();
    let state = AppState::new_test();
    let context_id = "project-gate1-persona-edit";
    let _project_dir = seed_project_context(&state, context_id).await;
    let persona = active_persona("persona-gate1-edit", "old persona body", "old-hash");
    state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed persona");
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    conversation.set_provider_session_ref(ralphx_lib::domain::agents::ProviderSessionRef {
        harness: ralphx_lib::domain::agents::AgentHarnessKind::Claude,
        provider_session_id: "preserved-provider-session".to_string(),
    });
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist bound conversation");
    let mut completed_run = AgentRun::new(conversation_id);
    completed_run.complete();
    completed_run.harness = Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude);
    completed_run.provider_session_id = Some("preserved-provider-session".to_string());
    completed_run.logical_model = Some("sonnet".to_string());
    completed_run.effective_model_id = Some("sonnet".to_string());
    state
        .agent_run_repo
        .create(completed_run)
        .await
        .expect("seed completed runtime for preserved provider session");

    let mut old_process = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn old interactive process");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            old_process.stdin.take().expect("old process stdin"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                provider_session_id: Some("preserved-provider-session".to_string()),
                persona_id: Some(persona.id.to_string()),
                persona_content_hash: Some("old-hash".to_string()),
            },
        )
        .await;
    state
        .persona_repo
        .set_status(&persona.id, PersonaStatus::Archived)
        .await
        .expect("release the active slug for fixture replacement");
    let mut updated_persona = persona.clone();
    updated_persona.content = "new persona body".to_string();
    updated_persona.content_hash = "new-hash".to_string();
    updated_persona.version += 1;
    state
        .persona_repo
        .create(updated_persona)
        .await
        .expect("simulate update_persona content hash bump");

    let temp = tempfile::tempdir().expect("test tempdir");
    let capture = temp.path().join("claude-arguments-and-prompt");
    let cli_path = write_capturing_claude_cli(temp.path(), &capture);
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        cli_path.to_str().expect("UTF-8 CLI path"),
    );
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true)
        .with_cli_path(cli_path)
        .with_plugin_dir(runtime_plugin_dir_for_gate1_persona_test())
        .with_working_directory(temp.path());

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "send after edit",
            SendMessageOptions::default(),
        )
        .await
        .expect("idle persona mismatch should spawn a replacement process");

    assert!(!result.was_queued);
    let metadata = state
        .interactive_process_registry
        .get_metadata(&interactive_key)
        .await
        .expect("fresh spawn must replace the stale IPR entry");
    assert_eq!(metadata.persona_id.as_deref(), Some("persona-gate1-edit"));
    assert_eq!(metadata.persona_content_hash.as_deref(), Some("new-hash"));
    let captured = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let captured = fs::read_to_string(&capture).unwrap_or_default();
            if captured.contains("--resume")
                && captured.contains("preserved-provider-session")
                && captured.contains("new persona body")
            {
                break captured;
            }
            assert!(
                Instant::now() < deadline,
                "fresh Claude invocation did not include resumed session and fresh persona prompt: {captured}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    assert!(captured.contains("--resume"));
    assert!(captured.contains("preserved-provider-session"));
    assert!(captured.contains("new persona body"));

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let _ = old_process.kill().await;
}

#[tokio::test]
async fn mid_turn_persona_mismatch_queues_behind_active_run() {
    let state = AppState::new_test();
    let context_id = "project-gate1-persona-active";
    let persona = active_persona("persona-gate1-active", "new persona body", "new-hash");
    state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed persona");
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    let conversation_id = conversation.id.as_str().to_string();
    let run = AgentRun::new(conversation.id);
    let run_id = run.id.as_str().to_string();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    state.agent_run_repo.create(run).await.unwrap();

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn active interactive process");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("active process stdin"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                provider_session_id: None,
                persona_id: Some(persona.id.to_string()),
                persona_content_hash: Some("old-hash".to_string()),
            },
        )
        .await;
    let running_key = RunningAgentKey::new("project", context_id);
    state
        .running_agent_registry
        .register(running_key.clone(), 0, conversation_id, run_id, None, None)
        .await;

    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true);
    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "queue after persona edit",
            SendMessageOptions::default(),
        )
        .await
        .expect("active persona mismatch should queue");

    assert!(result.was_queued);
    let queued = state
        .message_queue
        .get_queued(ChatContextType::Project, context_id);
    assert_eq!(queued.len(), 1);
    assert!(!queued[0].force_new_provider_session);
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let _ = child.kill().await;
}

#[tokio::test]
async fn matching_persona_metadata_reuses_stdin_fast_path() {
    let (_runtime_plugin_guard, _generated_plugin_root) =
        configure_runtime_plugin_dirs_for_gate1_persona_test();
    let state = AppState::new_test();
    let context_id = "project-gate1-persona-match";
    let persona = active_persona("persona-gate1-match", "persona body", "persona-hash");
    state.persona_repo.create(persona.clone()).await.unwrap();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let run_id = seed_live_run_owner(&state, context_id, conversation_id).await;
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("observer stdin"),
            InteractiveProcessMetadata {
                agent_run_id: Some(run_id.clone()),
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                provider_session_id: None,
                persona_id: Some(persona.id.to_string()),
                persona_content_hash: Some(persona.content_hash.clone()),
            },
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true);

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "fast-path message",
            SendMessageOptions::default(),
        )
        .await
        .expect("matching bound persona metadata must write stdin");
    assert!(!result.was_queued);
    assert_eq!(result.agent_run_id, run_id);
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut observed)
        .await
        .unwrap();
    let _ = child.wait().await;
    assert!(
        !observed.is_empty(),
        "matching metadata must use the stdin fast path"
    );
}

#[tokio::test]
async fn native_agent_flag_with_bound_persona_keeps_stdin_fast_path() {
    let _native_agent_flag =
        crate::support::env::EnvVarGuard::set("RALPHX_USE_NATIVE_AGENT_FLAG", "1");
    let (_runtime_plugin_guard, _generated_plugin_root) =
        configure_runtime_plugin_dirs_for_gate1_persona_test();
    let state = AppState::new_test();
    let context_id = "project-gate1-native-agent-persona";
    let persona = active_persona("persona-gate1-native", "persona body", "persona-hash");
    state.persona_repo.create(persona.clone()).await.unwrap();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let run_id = seed_live_run_owner(&state, context_id, conversation_id).await;

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("observer stdin"),
            InteractiveProcessMetadata {
                agent_run_id: Some(run_id.clone()),
                ..Default::default()
            },
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true);

    for message in ["native persona first", "native persona second"] {
        let result = service
            .send_message(
                ChatContextType::Project,
                context_id,
                message,
                SendMessageOptions::default(),
            )
            .await
            .expect("native --agent suppression must preserve the existing stdin process");
        assert_eq!(result.agent_run_id, run_id);
    }
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut observed)
        .await
        .unwrap();
    let _ = child.wait().await;
    assert!(
        String::from_utf8_lossy(&observed).contains("native persona second"),
        "both sends must reach the reused stdin instead of triggering a respawn"
    );
}

#[tokio::test]
async fn queue_message_persona_mismatch_queues_instead_of_writing_stale_stdin() {
    let state = AppState::new_test();
    let context_id = "project-queue-persona-mismatch";
    let persona = active_persona("persona-queue-mismatch", "persona body", "new-hash");
    state.persona_repo.create(persona.clone()).await.unwrap();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("observer stdin"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                provider_session_id: None,
                persona_id: Some(persona.id.to_string()),
                persona_content_hash: Some("old-hash".to_string()),
            },
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true);

    let queued = service
        .queue_message(
            ChatContextType::Project,
            context_id,
            "must queue behind persona replacement",
            Some("client-queue-persona-mismatch"),
        )
        .await
        .expect("persona mismatch must fall through to the durable queue");
    assert_eq!(queued.id, "client-queue-persona-mismatch");
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, context_id)
            .len(),
        1
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut observed)
        .await
        .unwrap();
    let _ = child.wait().await;
    assert!(
        observed.is_empty(),
        "stale persona stdin must not receive the queue message"
    );
}

#[tokio::test]
async fn queue_message_matching_persona_writes_through() {
    let state = AppState::new_test();
    let context_id = "project-queue-persona-match";
    let persona = active_persona("persona-queue-match", "persona body", "persona-hash");
    state.persona_repo.create(persona.clone()).await.unwrap();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("observer stdin"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                provider_session_id: None,
                persona_id: Some(persona.id.to_string()),
                persona_content_hash: Some(persona.content_hash.clone()),
            },
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true);

    service
        .queue_message(
            ChatContextType::Project,
            context_id,
            "matching persona queue write",
            None,
        )
        .await
        .expect("matching persona must preserve the immediate stdin write");
    assert!(state
        .message_queue
        .get_queued(ChatContextType::Project, context_id)
        .is_empty());

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut observed)
        .await
        .unwrap();
    let _ = child.wait().await;
    assert!(String::from_utf8_lossy(&observed).contains("matching persona queue write"));
}

#[tokio::test]
async fn queue_message_persona_guard_flag_off_writes_through() {
    let state = AppState::new_test();
    let context_id = "project-queue-persona-flag-off";
    state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            context_id.to_string(),
        )))
        .await
        .unwrap();
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("observer stdin"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: None,
                provider_session_id: None,
                persona_id: Some("stale-persona".to_string()),
                persona_content_hash: Some("stale-hash".to_string()),
            },
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_feature_enabled(false);

    service
        .queue_message(ChatContextType::Project, context_id, "flag off write", None)
        .await
        .expect("feature-off queue_message behavior must remain a direct stdin write");
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut observed)
        .await
        .unwrap();
    let _ = child.wait().await;
    assert!(String::from_utf8_lossy(&observed).contains("flag off write"));
}

#[tokio::test]
async fn gate1_without_active_conversation_bypasses_fast_path() {
    let (_runtime_plugin_guard, _generated_plugin_root) =
        configure_runtime_plugin_dirs_for_gate1_persona_test();
    let state = AppState::new_test();
    let context_id = "project-gate1-no-active-conversation";
    let _project_dir = seed_project_context(&state, context_id).await;
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register(
            interactive_key.clone(),
            child.stdin.take().expect("observer stdin"),
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_feature_enabled(true)
        .with_cli_path(std::env::temp_dir().join("missing-gate1-no-conversation-cli"));

    let error = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "must not reach an orphaned stdin",
            SendMessageOptions::default(),
        )
        .await
        .expect_err("orphaned IPR must fall through to the normal spawn path");
    assert!(error
        .to_string()
        .contains("missing-gate1-no-conversation-cli"));
    assert!(
        !state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await
    );
    let mut observed = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut observed)
        .await
        .unwrap();
    let _ = child.wait().await;
    assert!(observed.is_empty());
}

#[tokio::test]
async fn harness_override_persona_mismatch_removes_stale_ipr_before_spawn() {
    let state = AppState::new_test();
    let context_id = "project-gate1-persona-override-order";
    let _project_dir = seed_project_context(&state, context_id).await;
    let persona = active_persona("persona-override-order", "persona body", "new-hash");
    state.persona_repo.create(persona.clone()).await.unwrap();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let (stdin, mut child) = create_test_stdin().await;
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                provider_session_id: None,
                persona_id: Some(persona.id.to_string()),
                persona_content_hash: Some("old-hash".to_string()),
            },
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true)
        .with_cli_path(std::env::temp_dir().join("missing-gate1-override-order-cli"));

    let error = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "persona mismatch with explicit harness",
            SendMessageOptions {
                harness_override: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                ..Default::default()
            },
        )
        .await
        .expect_err("persona mismatch must still bypass the IPR with an override");
    assert!(error
        .to_string()
        .contains("missing-gate1-override-order-cli"));
    assert!(
        !state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await
    );
    let _ = child.kill().await;
}

#[tokio::test]
async fn suppressed_send_does_not_invalidate_unbound_process() {
    let state = AppState::new_test();
    let context_id = "project-gate1-persona-suppressed";
    let persona = active_persona("persona-gate1-suppressed", "persona body", "persona-hash");
    state.persona_repo.create(persona.clone()).await.unwrap();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let (stdin, mut child) = create_test_stdin().await;
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            stdin,
            InteractiveProcessMetadata::default(),
        )
        .await;
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_persona_feature_enabled(true);

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "suppressed persona message",
            SendMessageOptions {
                persona_directive: ralphx_lib::domain::entities::PersonaDirective::Suppress,
                ..Default::default()
            },
        )
        .await
        .expect("suppressed persona must preserve an unbound process");
    assert!(!result.was_queued);
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let _ = child.kill().await;
}

/// An agent-conversation follow-up with the composer-provided conversation and
/// model ids must continue through the registered Claude stdin. In particular,
/// a first turn has neither a completed run nor provider_session_ref yet, so an
/// unavailable continuation runtime must not be mistaken for a model switch.
#[tokio::test]
async fn gate1_project_agent_conversation_delivers_exact_stream_json_to_live_claude() {
    let state = AppState::new_test();
    let context_id = "project-gate1-agent-conversation";
    let (_project_dir, conversation_id, run_id, interactive_key, mut child) =
        setup_live_project_continuation(&state, context_id, None).await;

    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));
    let exact_user_text = "continue the existing Claude conversation immediately";
    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            exact_user_text,
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("sonnet".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("live Claude continuation must use Gate 1");

    assert!(
        !result.was_queued,
        "an IPR continuation must never show a queue"
    );
    assert!(result.queued_message_id.is_none());
    assert_eq!(result.conversation_id, conversation_id.as_str());
    assert_eq!(result.agent_run_id, run_id);
    assert!(
        state
            .queued_message_repo
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation_id.as_str()
            ))
            .await
            .expect("read durable queue")
            .is_empty(),
        "a successful Gate-1 send must not leave a durable queue row"
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = String::new();
    child
        .stdout
        .take()
        .expect("observer stdout")
        .read_to_string(&mut observed)
        .await
        .expect("read stream-json stdin");
    let _ = child.wait().await;
    let envelope: serde_json::Value = serde_json::from_str(observed.trim())
        .expect("Gate 1 must write one well-formed stream-json envelope");
    assert_eq!(envelope["type"], "user");
    assert_eq!(envelope["message"]["role"], "user");
    assert!(
        envelope["message"]["content"]
            .as_str()
            .is_some_and(|content| content.contains(exact_user_text)),
        "the live process must receive the exact user content"
    );
}

#[tokio::test]
async fn gate1_ownerless_process_fails_before_stdin_or_message_side_effects() {
    let events = RecordingEventSink::new();
    let mut state = AppState::new_test();
    state.events = Arc::new(events.clone());
    let context_id = "project-gate1-ownerless";
    let (_project_dir, conversation_id, _run_id, interactive_key, mut original_child) =
        setup_live_project_continuation(&state, context_id, None).await;

    let mut ownerless_child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn ownerless stdin observer");
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            ownerless_child.stdin.take().expect("ownerless stdin"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                ..Default::default()
            },
        )
        .await;

    let error = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .send_message(
            ChatContextType::Project,
            context_id,
            "must not be assigned a fabricated run",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("sonnet".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect_err("an ownerless IPR entry must fail closed");

    assert!(
        error.to_string().contains("no authoritative run owner"),
        "owner failure must be explicit: {error}"
    );
    assert!(
        state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read conversation messages")
            .is_empty(),
        "owner rejection must happen before user-message persistence"
    );
    assert!(
        events.events().is_empty(),
        "owner rejection must not emit message_created or run_started"
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = Vec::new();
    ownerless_child
        .stdout
        .take()
        .expect("ownerless stdout")
        .read_to_end(&mut observed)
        .await
        .expect("read ownerless stdout");
    let _ = ownerless_child.wait().await;
    assert!(
        observed.is_empty(),
        "owner rejection must happen before any stdin write"
    );
    let _ = original_child.kill().await;
}

#[tokio::test]
async fn gate1_message_persistence_failure_returns_error_without_success_side_effects() {
    let events = RecordingEventSink::new();
    let mut state = AppState::new_test();
    state.events = Arc::new(events.clone());
    let context_id = "project-gate1-message-persistence-failure";
    let (_project_dir, conversation_id, _run_id, interactive_key, mut child) =
        setup_live_project_continuation(&state, context_id, None).await;
    state.chat_message_repo = Arc::new(FailingChatMessageRepository::new());

    let error = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .send_message(
            ChatContextType::Project,
            context_id,
            "deliver stdin before persistence fails",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("sonnet".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect_err("a Gate-1 message persistence failure must not return SendResult success");

    assert!(
        matches!(
            error,
            ChatServiceError::RepositoryError(ref message)
                if message.contains(CHAT_MESSAGE_CREATE_FAILURE)
        ),
        "Gate 1 must return the typed repository error: {error}"
    );
    assert!(
        state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read failed-message repository")
            .is_empty(),
        "a failed user-message create must leave no message row"
    );
    assert_eq!(
        state
            .chat_timeline_repo
            .count_by_conversation(&conversation_id)
            .await
            .expect("count timeline rows"),
        0,
        "a failed user-message create must not create a timeline row"
    );
    assert!(
        !events.events().iter().any(|event| {
            matches!(
                event.event.as_str(),
                "agent:message_created" | "agent:run_started"
            )
        }),
        "a failed user-message create must not emit Gate-1 success events"
    );

    // Gate 1 intentionally writes the live continuation before persistence. That stdin
    // delivery is unavoidable once the live process accepts the turn; only later success
    // effects (message/timeline rows and success events) must be withheld on failure.
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = String::new();
    child
        .stdout
        .take()
        .expect("stdin observer stdout")
        .read_to_string(&mut observed)
        .await
        .expect("read delivered Gate-1 stdin");
    let _ = child.wait().await;
    let stdin_line = observed.trim();
    assert_eq!(
        stdin_line.lines().count(),
        1,
        "Gate 1 must send one stdin line"
    );
    let envelope: serde_json::Value =
        serde_json::from_str(stdin_line).expect("Gate 1 must deliver stream-json stdin");
    assert_eq!(envelope["type"], "user");
    assert!(
        envelope["message"]["content"]
            .as_str()
            .is_some_and(|content| content.contains("deliver stdin before persistence fails")),
        "Gate 1 must deliver the failed-persistence turn to the live stdin"
    );
}

#[tokio::test]
async fn gate1_model_alias_matches_effective_claude_identity() {
    let state = AppState::new_test();
    let context_id = "project-gate1-alias-model";
    let (_project_dir, conversation_id, _run_id, interactive_key, mut child) =
        setup_live_project_continuation(&state, context_id, Some(("sonnet", "claude-sonnet-4-6")))
            .await;
    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "same model through an effective id",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("sonnet".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("a model alias must preserve the live Claude continuation");

    assert!(!result.was_queued);
    assert!(result.queued_message_id.is_none());
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let _ = child.wait().await;
}

#[tokio::test]
async fn gate1_genuine_model_change_queues_behind_the_live_claude_run() {
    let state = AppState::new_test();
    let context_id = "project-gate1-different-model";
    let (_project_dir, conversation_id, _run_id, interactive_key, mut child) =
        setup_live_project_continuation(&state, context_id, Some(("sonnet", "claude-sonnet-4-6")))
            .await;
    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "switch to a different model",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("opus".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("a live provider switch must use the existing queue contract");

    assert!(result.was_queued);
    assert!(result.queued_message_id.is_some());
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let _ = child.kill().await;
}

// ============================================================================
// Test 2: Gate 1 MISS — IPR has NO entry, falls through to Gate 2/3
// ============================================================================

/// When IPR has no registered process for the context, Gate 1 should miss
/// and the logic should fall through to Gate 2 (try_register) and eventually
/// Gate 3 (spawn a new process).
#[tokio::test]
async fn test_gate1_miss_no_ipr_entry_falls_through() {
    let ipr = InteractiveProcessRegistry::new();
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-gate1-miss-1";

    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // --- Gate 1: Check IPR ---
    let has_ipr_entry = ipr.has_process(&ipr_key).await;
    assert!(
        !has_ipr_entry,
        "Gate 1 miss: has_process must return false when no process registered"
    );

    // --- Gate 2: try_register should succeed (no existing agent) ---
    let agent_key = RunningAgentKey {
        context_type: context_type_str.to_string(),
        context_id: context_id.to_string(),
    };
    let register_result = running_agent_registry
        .try_register(
            agent_key.clone(),
            "new-conv-id".to_string(),
            "new-run-id".to_string(),
        )
        .await;
    assert!(
        register_result.is_ok(),
        "Gate 2: try_register must succeed when no agent is running"
    );

    // Verify we're now registered (Gate 3 would spawn the process next)
    assert!(
        running_agent_registry.is_running(&agent_key).await,
        "After Gate 2: agent must be registered in running registry"
    );
}

// ============================================================================
// Test 3: Gate 1 conversation reuse vs force_fresh divergence
// ============================================================================

/// Gate 1 MUST use get_active_for_context (returns existing conversation).
/// Gate 3 (spawn path) uses get_or_create_conversation which for TaskExecution
/// creates a FRESH conversation (force_fresh=true).
///
/// This test verifies that get_active_for_context returns the pre-existing
/// conversation, while a new conversation created afterward has a DIFFERENT id.
#[tokio::test]
async fn test_gate1_reuses_existing_conversation_vs_fresh_on_miss() {
    let conversation_repo = MemoryChatConversationRepository::new();
    let context_id = "task-gate1-reuse-1";
    let task_id = TaskId::from_string(context_id.to_string());

    // Create the "original" conversation (from initial spawn)
    let original_conv = ChatConversation::new_task_execution(task_id.clone());
    let original_conv_id = original_conv.id;
    conversation_repo.create(original_conv).await.unwrap();

    // Gate 1 path: get_active_for_context returns the existing one
    let gate1_conv = conversation_repo
        .get_active_for_context(ChatContextType::TaskExecution, context_id)
        .await
        .unwrap()
        .expect("Gate 1 must find existing conversation");
    assert_eq!(
        gate1_conv.id, original_conv_id,
        "Gate 1 must return the original conversation_id"
    );

    // Gate 3 path (simulated): creating a new conversation yields a DIFFERENT id
    let fresh_conv = ChatConversation::new_task_execution(task_id);
    let fresh_conv_id = fresh_conv.id;
    conversation_repo.create(fresh_conv).await.unwrap();

    assert_ne!(
        original_conv_id, fresh_conv_id,
        "Force-fresh conversation must have a different id than the original"
    );

    // After creating the fresh one, get_active_for_context returns the MOST RECENT
    // (max by created_at), which would be the fresh one — demonstrating why
    // Gate 1 must be checked BEFORE get_or_create_conversation
    let latest_conv = conversation_repo
        .get_active_for_context(ChatContextType::TaskExecution, context_id)
        .await
        .unwrap()
        .expect("Must find a conversation");
    assert_eq!(
        latest_conv.id, fresh_conv_id,
        "After force_fresh, get_active_for_context returns the newest — \
         Gate 1 must run BEFORE get_or_create to avoid creating an unwanted fresh conv"
    );
}

// ============================================================================
// Test 4: Gate 1 stdin write failure → remove IPR entry and fall through
// ============================================================================

/// When IPR has an entry but the stdin write fails (no process registered),
/// Gate 1 should:
/// 1. Remove the broken IPR entry
/// 2. Fall through to Gate 2/3 (normal spawn path)
///
/// Note: OS-level broken pipe detection is unreliable for small writes (kernel
/// buffers may absorb them even after the reader dies). Instead, we test the
/// write_message error path for a non-existent key, and the remove-on-failure
/// cleanup pattern that Gate 1 uses.
#[tokio::test]
async fn test_gate1_write_failure_removes_ipr_entry_and_falls_through() {
    let ipr = InteractiveProcessRegistry::new();
    let context_type_str = "task_execution";
    let context_id = "task-gate1-broken-1";

    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // Register a real process, then immediately remove it to simulate stale state
    // This leaves the IPR empty for this key, so write_message will fail
    let (stdin, _child) = create_test_stdin().await;
    ipr.register(ipr_key.clone(), stdin).await;
    assert!(
        ipr.has_process(&ipr_key).await,
        "Precondition: entry exists"
    );

    // Remove the entry (simulating what happens when IPR discovers a dead process)
    ipr.remove(&ipr_key).await;

    // Now verify the Gate 1 fallback pattern:
    // write_message fails for non-existent key
    let write_result = ipr.write_message(&ipr_key, "test message").await;
    assert!(
        write_result.is_err(),
        "Gate 1: write_message must fail when no process registered"
    );

    // After failure, ensure has_process returns false (fall through to Gate 2/3)
    assert!(
        !ipr.has_process(&ipr_key).await,
        "After removal: IPR must not report the broken process"
    );

    // Additionally test the full Gate 1 error-path pattern:
    // has_process → true, write_message → Err, remove (mirrors mod.rs lines 697-709)
    let (stdin2, _child2) = create_test_stdin().await;
    let broken_key = InteractiveProcessKey::new(context_type_str, "task-gate1-broken-2");
    ipr.register(broken_key.clone(), stdin2).await;

    // Simulate: has_process = true (stale check)
    assert!(ipr.has_process(&broken_key).await);

    // Simulate write failure by removing entry right before write (race-like scenario)
    // In production, this is the broken pipe case
    ipr.remove(&broken_key).await;
    let write_result = ipr
        .write_message(&broken_key, "message after removal")
        .await;
    assert!(write_result.is_err(), "Write must fail after entry removed");

    // Post-failure cleanup: remove (idempotent — already removed)
    let removed = ipr.remove(&broken_key).await;
    assert!(
        removed.is_none(),
        "Remove after removal should return None (idempotent cleanup)"
    );
    assert!(
        !ipr.has_process(&broken_key).await,
        "Gate 1 fallback complete: no stale entries remain"
    );
}

// ============================================================================
// Test 5: Gate 1 burst prevention — multiple messages, single increment
// ============================================================================

/// When multiple messages arrive for the same interactive context in quick
/// succession (burst), only the first should claim the interactive slot.
/// Subsequent messages should still write to stdin but NOT double-increment
/// the running count.
#[tokio::test]
async fn test_gate1_burst_prevention_multiple_messages_single_increment() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let execution_state = Arc::new(ExecutionState::new());

    let context_type_str = "task_execution";
    let context_id = "task-gate1-burst-1";
    let slot_key = format!("{}/{}", context_type_str, context_id);

    // Register interactive process
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);
    ipr.register(ipr_key.clone(), stdin).await;

    // Process finished a turn → idle
    execution_state.increment_running();
    execution_state.decrement_and_mark_idle(&slot_key);
    assert_eq!(execution_state.running_count(), 0);

    // Simulate 5 rapid Gate 1 hits (5 messages arriving nearly simultaneously)
    let mut successful_writes = 0;
    let mut successful_claims = 0;

    for i in 0..5 {
        // Each message hits Gate 1: write to stdin
        let msg = format!("burst message {}\n", i);
        if ipr.write_message(&ipr_key, &msg).await.is_ok() {
            successful_writes += 1;

            // Gate 1 burst prevention: claim_interactive_slot is atomic
            if execution_state.claim_interactive_slot(&slot_key) {
                execution_state.increment_running();
                successful_claims += 1;
            }
        }
    }

    assert_eq!(successful_writes, 5, "All 5 writes should succeed");
    assert_eq!(
        successful_claims, 1,
        "Only the first message should claim the slot (burst prevention)"
    );
    assert_eq!(
        execution_state.running_count(),
        1,
        "Running count must be 1 (not 5) after burst"
    );
}

// ============================================================================
// Test 6: Gate 1 with different context types — IPR isolation
// ============================================================================

/// IPR entries are keyed by (context_type, context_id). A TaskExecution entry
/// must not match an Ideation query for the same context_id.
#[tokio::test]
async fn test_gate1_ipr_context_type_isolation() {
    let ipr = InteractiveProcessRegistry::new();
    let context_id = "shared-id-123";

    // Register a TaskExecution process
    let (stdin, _child) = create_test_stdin().await;
    let task_exec_key = InteractiveProcessKey::new("task_execution", context_id);
    ipr.register(task_exec_key.clone(), stdin).await;

    // Verify TaskExecution key matches
    assert!(
        ipr.has_process(&task_exec_key).await,
        "TaskExecution key should match"
    );

    // Verify Ideation key does NOT match (different context_type, same context_id)
    let ideation_key = InteractiveProcessKey::new("ideation", context_id);
    assert!(
        !ipr.has_process(&ideation_key).await,
        "Ideation key must NOT match TaskExecution entry (context_type isolation)"
    );

    // Verify Merge key does NOT match
    let merge_key = InteractiveProcessKey::new("merge", context_id);
    assert!(
        !ipr.has_process(&merge_key).await,
        "Merge key must NOT match TaskExecution entry"
    );
}

// ============================================================================
// Test 7: Gate 1 full lifecycle — spawn → idle → Gate 1 hit → TurnComplete
// ============================================================================

/// End-to-end Gate 1 lifecycle:
/// 1. Initial spawn (Gate 3) creates conversation + registers in IPR
/// 2. TurnComplete → process goes idle
/// 3. New message → Gate 1 hits, writes to stdin, reuses conversation
/// 4. TurnComplete → process goes idle again
/// 5. Process exits → IPR entry removed
#[tokio::test]
async fn test_gate1_full_lifecycle_spawn_idle_hit_complete_exit() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let execution_state = Arc::new(ExecutionState::new());

    let context_type_str = "task_execution";
    let context_id = "task-lifecycle-1";
    let task_id = TaskId::from_string(context_id.to_string());
    let slot_key = format!("{}/{}", context_type_str, context_id);
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // === Phase 1: Initial spawn (Gate 3 would do this) ===
    let original_conv = ChatConversation::new_task_execution(task_id);
    let original_conv_id = original_conv.id;
    conversation_repo.create(original_conv).await.unwrap();

    let (stdin, _child) = create_test_stdin().await;
    ipr.register(ipr_key.clone(), stdin).await;
    execution_state.increment_running();
    assert_eq!(
        execution_state.running_count(),
        1,
        "Phase 1: process running"
    );

    // === Phase 2: TurnComplete → idle ===
    execution_state.decrement_and_mark_idle(&slot_key);
    assert_eq!(execution_state.running_count(), 0, "Phase 2: process idle");
    assert!(execution_state.is_interactive_idle(&slot_key));

    // === Phase 3: New message → Gate 1 hit ===
    assert!(ipr.has_process(&ipr_key).await, "Phase 3: IPR has entry");

    let write_result = ipr.write_message(&ipr_key, "follow-up message").await;
    assert!(write_result.is_ok(), "Phase 3: stdin write succeeds");

    // Claim slot + increment
    assert!(execution_state.claim_interactive_slot(&slot_key));
    execution_state.increment_running();
    assert_eq!(
        execution_state.running_count(),
        1,
        "Phase 3: process active again"
    );

    // Reuse existing conversation
    let reused_conv = conversation_repo
        .get_active_for_context(ChatContextType::TaskExecution, context_id)
        .await
        .unwrap()
        .expect("Phase 3: must find existing conversation");
    assert_eq!(
        reused_conv.id, original_conv_id,
        "Phase 3: must reuse original conversation_id"
    );

    // === Phase 4: Second TurnComplete → idle again ===
    execution_state.decrement_and_mark_idle(&slot_key);
    assert_eq!(execution_state.running_count(), 0, "Phase 4: idle again");
    assert!(execution_state.is_interactive_idle(&slot_key));

    // === Phase 5: Process exits → cleanup ===
    ipr.remove(&ipr_key).await;
    assert!(
        !ipr.has_process(&ipr_key).await,
        "Phase 5: IPR entry removed"
    );
    execution_state.remove_interactive_slot(&slot_key);
    assert!(
        !execution_state.is_interactive_idle(&slot_key),
        "Phase 5: slot cleaned up"
    );
}

// ============================================================================
// Test 8: Gate 1 with shared IPR — verify same Arc sees same entries
// ============================================================================

/// The shared IPR pattern (CRITICAL from MEMORY.md): all services must use
/// the same Arc<InteractiveProcessRegistry>. This test verifies that two
/// references to the same Arc see the same entries.
#[tokio::test]
async fn test_gate1_shared_ipr_arc_sees_same_entries() {
    let shared_ipr = Arc::new(InteractiveProcessRegistry::new());
    let ipr_ref1 = Arc::clone(&shared_ipr);
    let ipr_ref2 = Arc::clone(&shared_ipr);

    let context_type_str = "task_execution";
    let context_id = "task-shared-ipr-1";
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // Reference 1 registers the process
    let (stdin, _child) = create_test_stdin().await;
    ipr_ref1.register(ipr_key.clone(), stdin).await;

    // Reference 2 should see it (same underlying HashMap)
    assert!(
        ipr_ref2.has_process(&ipr_key).await,
        "Shared IPR: Arc clone must see entries registered by sibling reference"
    );

    // Reference 2 can write to it
    let write_result = ipr_ref2.write_message(&ipr_key, "hello from ref2").await;
    assert!(
        write_result.is_ok(),
        "Shared IPR: Arc clone must be able to write to process registered by sibling"
    );

    // Reference 1 removes it
    ipr_ref1.remove(&ipr_key).await;

    // Reference 2 should no longer see it
    assert!(
        !ipr_ref2.has_process(&ipr_key).await,
        "Shared IPR: removal via one Arc clone must be visible to the other"
    );
}

// ============================================================================
// Helpers
// ============================================================================

/// Create a real stdin pipe via `cat` subprocess for testing InteractiveProcessRegistry.
async fn create_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}
