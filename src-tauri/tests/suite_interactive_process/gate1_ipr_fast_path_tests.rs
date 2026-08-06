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
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::{Listener, Manager};
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;
use tokio_util::sync::CancellationToken;

use ralphx_lib::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use ralphx_lib::application::chat_service::{
    process_stream_background, ChatService, ChatServiceError, SendMessageOptions,
    StreamingStateCache,
};
use ralphx_lib::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata, InteractiveProcessRegistry,
};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::ProviderSessionRef;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, ChatContextType,
    ChatConversation, IdeationAnalysisBaseRefKind, Persona, PersonaId, PersonaStatus, Project,
    ProjectId, TaskId,
};
use ralphx_lib::domain::repositories::ChatConversationRepository;
use ralphx_lib::domain::services::running_agent_registry::{
    MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry,
};
use ralphx_lib::domain::services::QueueKey;
use ralphx_lib::infrastructure::memory::MemoryChatConversationRepository;
use ralphx_lib::testing::{GetByIdFailingAgentRunRepository, AGENT_RUN_GET_BY_ID_FAILURE};

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

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn initialize_git_project(root: &Path) {
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test User"]);
    fs::write(root.join("README.md"), "gate-1 fixture\n").expect("write fixture file");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
}

async fn seed_project_context(state: &AppState, context_id: &str) -> tempfile::TempDir {
    let project_dir = tempfile::tempdir().expect("temp project dir");
    let mut project = Project::new(
        format!("Gate-1 test project {context_id}"),
        project_dir.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string(context_id.to_string());
    let worktree_parent = project_dir.path().join("worktrees");
    fs::create_dir_all(&worktree_parent).expect("create test worktree parent");
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().into_owned());
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

fn spawn_interactive_claude_result_after_two_messages() -> tokio::process::Child {
    let mut command = TokioCommand::new("sh");
    command
        .arg("-c")
        .arg(
            "IFS= read -r first || exit 1\n\
             IFS= read -r second || exit 1\n\
             printf '%s\\n' \"$RALPHX_STREAM_RESULT\"\n\
             exec sleep 10",
        )
        .env(
            "RALPHX_STREAM_RESULT",
            r#"{"type":"result","session_id":"gate1-burst-session","is_error":false,"result":"burst handled","cost_usd":0.0}"#,
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    command
        .spawn()
        .expect("spawn two-message interactive Claude fixture")
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
    live_runtime: Option<(&str, &str)>,
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

    let mut run = AgentRun::new(conversation_id);
    if let Some((logical_model, effective_model_id)) = live_runtime {
        run.harness = Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude);
        run.logical_model = Some(logical_model.to_string());
        run.effective_model_id = Some(effective_model_id.to_string());
    }
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
                agent_name: None,
                agent_profile: None,
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
async fn gate1_idle_plan_launch_identity_never_reuses_stale_stdin_after_edit_handoff() {
    let provider_home = tempfile::tempdir().expect("provider state home");
    let session_dir = provider_home
        .path()
        .join(".claude/projects/gate1-plan-to-edit");
    fs::create_dir_all(&session_dir).expect("session dir");
    fs::write(session_dir.join("planning-session.jsonl"), "{}\n").expect("session artifact");
    let _provider_home_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_PROVIDER_STATE_HOME_OVERRIDE",
        provider_home.path().as_os_str(),
    );
    let _allow_spawn =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (_runtime_plugin_guard, _generated_plugin_root) =
        configure_runtime_plugin_dirs_for_gate1_persona_test();
    let state = AppState::new_test();
    let context_id = "project-gate1-plan-to-edit";
    let project_dir = seed_project_context(&state, context_id).await;
    initialize_git_project(project_dir.path());
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string(context_id.to_string()));
    conversation.set_agent_mode(Some(
        ralphx_lib::domain::entities::AgentConversationWorkspaceMode::Edit,
    ));
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: ralphx_lib::domain::agents::AgentHarnessKind::Claude,
        provider_session_id: "planning-session".to_string(),
    });
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist Edit conversation");
    let mut completed_run = AgentRun::new(conversation_id);
    completed_run.complete();
    completed_run.harness = Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude);
    completed_run.provider_session_id = Some("planning-session".to_string());
    completed_run.logical_model = Some("sonnet".to_string());
    completed_run.effective_model_id = Some("sonnet".to_string());
    state
        .agent_run_repo
        .create(completed_run)
        .await
        .expect("seed stale planning continuation runtime");

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stale Plan stdin observer");
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("cat stdin"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                agent_name: Some("ralphx:ralphx-ideation".to_string()),
                agent_profile: Some("plan".to_string()),
                ..Default::default()
            },
        )
        .await;

    let temp = tempfile::tempdir().expect("test tempdir");
    let project_id = ProjectId::from_string(context_id.to_string());
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .expect("load test project")
        .expect("test project should exist");
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("resolve canonical Edit workspace path");
    let workspace_path_arg = workspace_path.to_string_lossy().into_owned();
    git(
        project_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            "ralphx/test/gate1-plan-to-edit",
            workspace_path_arg.as_str(),
            "main",
        ],
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(AgentConversationWorkspace::new(
            conversation_id,
            project_id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            "ralphx/test/gate1-plan-to-edit".to_string(),
            workspace_path_arg,
        ))
        .await
        .expect("persist Edit workspace");
    let capture = temp.path().join("claude-arguments-and-prompt");
    let cli_path = write_capturing_claude_cli(temp.path(), &capture);
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        cli_path.to_str().expect("UTF-8 CLI path"),
    );
    let service = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_cli_path(cli_path)
        .with_plugin_dir(runtime_plugin_dir_for_gate1_persona_test())
        .with_working_directory(temp.path());

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "must not reach stale Plan stdin",
            SendMessageOptions::default(),
        )
        .await
        .expect("launch identity mismatch must spawn a fresh Edit runtime");

    assert!(!result.was_queued);
    let fresh_metadata = state
        .interactive_process_registry
        .get_metadata(&interactive_key)
        .await
        .expect("fresh Edit runtime must replace the stale Plan registration");
    assert_eq!(
        fresh_metadata.agent_name.as_deref(),
        Some("ralphx:ralphx-general-worker")
    );
    assert_eq!(fresh_metadata.agent_profile, None);
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let captured = fs::read_to_string(&capture).unwrap_or_default();
            if captured.contains("ralphx-general-worker") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "fresh Edit invocation did not resolve the general worker: {captured}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };

    service
        .stop_agent(ChatContextType::Project, context_id)
        .await
        .expect("stop fresh Edit runtime after assertions");
    assert!(
        !state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await,
        "the stale Plan process must be retired before the Edit launch"
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
        "the stale Plan process must receive no Edit message"
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
                agent_name: Some("ralphx:ralphx-ideation".to_string()),
                agent_profile: Some("plan".to_string()),
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
    conversation.set_agent_mode(Some(
        ralphx_lib::domain::entities::AgentConversationWorkspaceMode::Edit,
    ));
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
                agent_name: Some("ralphx:ralphx-ideation".to_string()),
                agent_profile: Some("plan".to_string()),
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
    conversation.set_agent_mode(Some(
        ralphx_lib::domain::entities::AgentConversationWorkspaceMode::Edit,
    ));
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
                agent_name: Some("ralphx:ralphx-general-worker".to_string()),
                agent_profile: None,
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
                agent_name: None,
                agent_profile: None,
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
                agent_name: None,
                agent_profile: None,
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
                agent_name: None,
                agent_profile: None,
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
                agent_name: None,
                agent_profile: None,
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
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let run_id = seed_live_run_owner(&state, context_id, conversation_id).await;
    let (stdin, mut child) = create_test_stdin().await;
    let interactive_key = InteractiveProcessKey::new("project", context_id);
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            stdin,
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
        setup_live_project_continuation(
            &state,
            context_id,
            None,
            Some(("sonnet", "claude-sonnet-4-6")),
        )
        .await;

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
async fn verify_plan_retirement_after_settled_burst_does_not_requeue() {
    let _allow_spawn =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let (_runtime_plugin_guard, _generated_plugin_root) =
        configure_runtime_plugin_dirs_for_gate1_persona_test();
    let temp = tempfile::tempdir().expect("test tempdir");
    let capture = temp.path().join("fresh-verify-plan-cli");
    let cli_path = write_capturing_claude_cli(temp.path(), &capture);
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        cli_path.to_str().expect("UTF-8 CLI path"),
    );

    let app = mock_builder()
        .manage(AppState::new_test())
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let queued_events = Arc::new(Mutex::new(Vec::new()));
    let captured_events = Arc::clone(&queued_events);
    let _listener = app.listen("agent:message_queued", move |event| {
        captured_events
            .lock()
            .expect("queued event lock")
            .push(event.payload().to_string());
    });
    let state = app.state::<AppState>();
    let context_id = "project-gate1-verify-plan-settled-burst";
    let (project_dir, conversation_id, retired_run_id, interactive_key, mut observer_child) =
        setup_live_project_continuation(
            &state,
            context_id,
            None,
            Some(("sonnet", "claude-sonnet-4-6")),
        )
        .await;
    initialize_git_project(project_dir.path());

    drop(
        state
            .interactive_process_registry
            .remove(&interactive_key)
            .await
            .expect("remove stdin observer before registering stream fixture"),
    );
    let _ = observer_child.wait().await;
    let mut child = spawn_interactive_claude_result_after_two_messages();
    let stream_token = state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("two-message fixture stdin"),
            InteractiveProcessMetadata {
                agent_run_id: Some(retired_run_id.clone()),
                harness: Some(ralphx_lib::domain::agents::AgentHarnessKind::Claude),
                ..Default::default()
            },
        )
        .await;
    let retired_owner = state
        .interactive_process_registry
        .capture_owner(&interactive_key)
        .await
        .expect("live IPR owner");
    assert_eq!(retired_owner.token, stream_token);
    let service = state
        .build_chat_service_for_runtime(
            Some(Arc::new(ExecutionState::new())),
            Some(app.handle().clone()),
        )
        .with_cli_path(cli_path)
        .with_plugin_dir(runtime_plugin_dir_for_gate1_persona_test())
        .with_working_directory(temp.path());

    for message in ["first delivered burst turn", "second delivered burst turn"] {
        let result = service
            .send_message(
                ChatContextType::Project,
                context_id,
                message,
                SendMessageOptions {
                    conversation_id_override: Some(conversation_id),
                    model_override: Some("sonnet".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("Gate 1 burst delivery");
        assert!(!result.was_queued);
        assert_eq!(result.agent_run_id, retired_run_id);
    }

    let interactive_registry = Arc::clone(&state.interactive_process_registry);
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let stream_conversation_id = conversation_id;
    let stream_interactive_key = interactive_key.clone();
    let stream_run_id = retired_run_id.clone();
    let stream_app_handle = app.handle().clone();
    let mut stream_task = tokio::spawn(async move {
        process_stream_background::<tauri::test::MockRuntime>(
            child,
            ralphx_lib::domain::agents::AgentHarnessKind::Claude,
            ChatContextType::Project,
            context_id,
            &stream_conversation_id,
            None,
            Some(stream_app_handle),
            None,
            None,
            None,
            None,
            None,
            None,
            CancellationToken::new(),
            StreamingStateCache::new(),
            None,
            Some(agent_run_repo),
            Some(stream_run_id),
            None,
            None,
            false,
            false,
            Some(interactive_registry),
            Some(stream_interactive_key),
            Some(stream_token),
        )
        .await
    });

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(
                state
                    .interactive_process_registry
                    .retire_after_turn_disposition_if_owner(
                        &interactive_key,
                        retired_owner.token,
                        &retired_run_id,
                    )
                    .await,
                ralphx_lib::application::interactive_process_registry::InteractiveProcessRetireAfterTurnDisposition::Idle {
                    is_armed: false
                }
            ) {
                break;
            }

            tokio::select! {
                result = &mut stream_task => match result {
                    Ok(Ok(_)) => panic!("stream exited before reaching the idle TurnComplete boundary"),
                    Ok(Err(error)) => panic!("stream finalization failed before reaching idle: {error}"),
                    Err(error) => panic!("stream task failed before reaching idle: {error}"),
                },
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    })
    .await
    .expect("stream must finalize its successful assistant turn and mark the owner idle");

    let verify_plan_result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "start fresh Verify Plan run",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                metadata: Some(format!(
                    r#"{{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"{}","ralphx_action_target_id":"plan-artifact"}}"#,
                    conversation_id.as_str(),
                )),
                ..Default::default()
            },
        )
        .await
        .expect("Verify Plan send must retire the idle owner and launch fresh");

    assert!(!verify_plan_result.was_queued);
    assert_ne!(verify_plan_result.agent_run_id, retired_run_id);
    let replacement_owner = state
        .interactive_process_registry
        .capture_owner(&interactive_key)
        .await
        .expect("fresh Verify Plan IPR owner");
    assert_ne!(replacement_owner.token, retired_owner.token);
    assert_eq!(
        replacement_owner.agent_run_id, verify_plan_result.agent_run_id,
        "the old IPR owner must be replaced by the fresh Verify Plan owner"
    );
    assert_eq!(
        state
            .running_agent_registry
            .get(&RunningAgentKey::new("project", conversation_id.as_str()))
            .await
            .expect("fresh Verify Plan running registration")
            .agent_run_id,
        verify_plan_result.agent_run_id,
        "retiring the old owner must unregister its run before fresh launch"
    );

    let queue_key = QueueKey::new(ChatContextType::Project, conversation_id.as_str());
    assert!(
        state
            .queued_message_repo
            .list(&queue_key)
            .await
            .expect("durable queue")
            .is_empty(),
        "settled stdin turns must not be durably requeued during Verify Plan retirement"
    );
    assert!(
        state
            .message_queue
            .get_queued_with_key(&queue_key)
            .is_empty(),
        "settled stdin turns must not be retained in the in-memory queue"
    );
    assert!(
        queued_events.lock().expect("queued event lock").is_empty(),
        "Verify Plan retirement must not publish phantom queued-message events"
    );

    stream_task.abort();
    assert!(
        stream_task
            .await
            .expect_err("stream fixture should be aborted after Verify Plan replaces its owner")
            .is_cancelled(),
        "the test must clean up its still-interactive stream task"
    );
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
}

#[tokio::test]
async fn gate1_ownerless_process_falls_through_without_stdin_or_message_side_effects() {
    let events = RecordingEventSink::new();
    let mut state = AppState::new_test();
    state.events = Arc::new(events.clone());
    let context_id = "project-gate1-ownerless";
    let (_project_dir, conversation_id, _run_id, interactive_key, mut original_child) =
        setup_live_project_continuation(
            &state,
            context_id,
            None,
            Some(("sonnet", "claude-sonnet-4-6")),
        )
        .await;

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

    let result = state
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
        .await;

    // Fail closed on WRITING into a process we may not own; do NOT fail closed on
    // DECIDING to reuse one. A registration that lost its owner between has_process()
    // and capture_owner() must degrade into the normal spawn path, not a send error.
    if let Err(ref error) = result {
        assert!(
            !error.to_string().contains("no authoritative run owner"),
            "an ownerless IPR entry must fall through instead of failing the send: {error}"
        );
    }
    let queued = result
        .expect("an ownerless IPR entry must fall through to the spawn path")
        .was_queued;
    assert!(
        queued,
        "the spawn path must queue behind the still-registered live run"
    );
    assert!(
        !events
            .events()
            .iter()
            .any(|event| event.event == "agent:run_started"),
        "Gate 1 must not claim run authority for an ownerless process"
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
        "fall-through must happen before any stdin write"
    );
    let _ = original_child.kill().await;
}

#[tokio::test]
async fn gate1_message_persistence_failure_prevents_untracked_stdin_delivery() {
    let mut seed_state = AppState::new_test();
    seed_state.chat_message_repo = Arc::new(FailingChatMessageRepository::new());
    let app = mock_builder()
        .manage(seed_state)
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let handle = app.handle().clone();
    let state = app.state::<AppState>();
    let context_id = "project-gate1-message-persistence-failure";
    let (_project_dir, conversation_id, _run_id, interactive_key, mut child) =
        setup_live_project_continuation(
            &state,
            context_id,
            None,
            Some(("sonnet", "claude-sonnet-4-6")),
        )
        .await;

    let run_started_events = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let run_started_listener = Arc::clone(&run_started_events);
    handle.listen("agent:run_started", move |event| {
        let payload = serde_json::from_str(event.payload()).expect("run_started payload JSON");
        run_started_listener
            .lock()
            .expect("run_started event lock")
            .push(payload);
    });
    let message_created_events = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let message_created_listener = Arc::clone(&message_created_events);
    handle.listen("agent:message_created", move |event| {
        let payload = serde_json::from_str(event.payload()).expect("message_created payload JSON");
        message_created_listener
            .lock()
            .expect("message_created event lock")
            .push(payload);
    });

    let error = state
        .build_chat_service_for_runtime(Some(Arc::new(ExecutionState::new())), Some(handle.clone()))
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
        "Gate 1 must fail before delivery when the pending turn cannot be persisted: {error}"
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
        message_created_events
            .lock()
            .expect("message_created event lock")
            .is_empty(),
        "a failed user-message create must not claim the message was persisted"
    );
    let observed_run_started = run_started_events
        .lock()
        .expect("run_started event lock")
        .clone();
    assert_eq!(
        observed_run_started.len(),
        0,
        "an undelivered turn must not emit run_started"
    );

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
    assert!(
        observed.is_empty(),
        "failed persistence must not write an untracked user turn to stdin"
    );
}

#[tokio::test]
async fn gate1_model_alias_matches_effective_claude_identity() {
    let state = AppState::new_test();
    let context_id = "project-gate1-alias-model";
    let (_project_dir, conversation_id, _run_id, interactive_key, mut child) =
        setup_live_project_continuation(
            &state,
            context_id,
            None,
            Some(("sonnet", "claude-sonnet-4-6")),
        )
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
        setup_live_project_continuation(
            &state,
            context_id,
            None,
            Some(("sonnet", "claude-sonnet-4-6")),
        )
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
    let queued = state
        .queued_message_repo
        .list(&QueueKey::new(
            ChatContextType::Project,
            conversation_id.as_str(),
        ))
        .await
        .expect("read durable queue");
    assert_eq!(queued.len(), 1);
    assert!(queued[0].force_new_provider_session);
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let _ = child.kill().await;
}

#[tokio::test]
async fn gate1_stale_completed_run_model_does_not_queue_same_model_live_turn() {
    let state = AppState::new_test();
    let context_id = "project-gate1-stale-completed-model";
    let (_project_dir, conversation_id, live_run_id, interactive_key, mut child) =
        setup_live_project_continuation(
            &state,
            context_id,
            Some(("opus", "claude-opus-5")),
            Some(("fable", "claude-fable-5")),
        )
        .await;
    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));
    let exact_user_text = "continue on the live fable run";

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            exact_user_text,
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("fable".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("the live run model must control Gate 1");

    assert!(!result.was_queued);
    assert!(result.queued_message_id.is_none());
    assert_eq!(result.agent_run_id, live_run_id);
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
        "stale completed-run evidence must not create a durable queue row"
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
async fn gate1_live_model_change_queues_even_when_completed_run_matches() {
    let state = AppState::new_test();
    let context_id = "project-gate1-live-model-change";
    let (_project_dir, conversation_id, _run_id, interactive_key, mut child) =
        setup_live_project_continuation(
            &state,
            context_id,
            Some(("fable", "claude-fable-5")),
            Some(("opus", "claude-opus-5")),
        )
        .await;
    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "switch the live run to fable",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("fable".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("a genuine live-run model switch must queue");

    assert!(result.was_queued);
    assert!(result.queued_message_id.is_some());
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = Vec::new();
    child
        .stdout
        .take()
        .expect("observer stdout")
        .read_to_end(&mut observed)
        .await
        .expect("read observer stdout");
    let _ = child.wait().await;
    assert!(
        observed.is_empty(),
        "a genuine model switch must not write to the live process"
    );
}

#[tokio::test]
async fn gate1_live_run_without_model_does_not_queue() {
    let state = AppState::new_test();
    let context_id = "project-gate1-live-run-without-model";
    let (_project_dir, conversation_id, live_run_id, interactive_key, mut child) =
        setup_live_project_continuation(&state, context_id, Some(("opus", "claude-opus-5")), None)
            .await;
    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));

    let result = service
        .send_message(
            ChatContextType::Project,
            context_id,
            "continue without model evidence",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("fable".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("missing live-run model evidence must remain non-switching");

    assert!(!result.was_queued);
    assert!(result.queued_message_id.is_none());
    assert_eq!(result.agent_run_id, live_run_id);
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let _ = child.wait().await;
}

#[tokio::test]
async fn gate1_live_run_read_failure_returns_repository_error_without_stdin_delivery() {
    let mut state = AppState::new_test();
    state.agent_run_repo = Arc::new(GetByIdFailingAgentRunRepository::new(Arc::clone(
        &state.agent_run_repo,
    )));
    let context_id = "project-gate1-live-run-read-failure";
    let (_project_dir, conversation_id, _live_run_id, interactive_key, mut child) =
        setup_live_project_continuation(
            &state,
            context_id,
            None,
            Some(("sonnet", "claude-sonnet-4-6")),
        )
        .await;

    let error = state
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .send_message(
            ChatContextType::Project,
            context_id,
            "must not write without live-run authority",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id),
                model_override: Some("sonnet".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect_err("an authoritative live-run read failure must abort Gate 1");

    assert!(
        matches!(
            error,
            ChatServiceError::RepositoryError(ref message)
                if message.contains(AGENT_RUN_GET_BY_ID_FAILURE)
        ),
        "live-run read failures must remain repository errors: {error}"
    );
    assert!(
        state
            .queued_message_repo
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation_id.as_str(),
            ))
            .await
            .expect("read durable queue")
            .is_empty(),
        "a failed authority read must not queue or claim delivery"
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let mut observed = Vec::new();
    child
        .stdout
        .take()
        .expect("observer stdout")
        .read_to_end(&mut observed)
        .await
        .expect("read observer stdout");
    let _ = child.wait().await;
    assert!(
        observed.is_empty(),
        "a failed authority read must not write to stdin"
    );
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
