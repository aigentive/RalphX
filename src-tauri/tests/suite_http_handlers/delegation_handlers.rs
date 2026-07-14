use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use std::{fs, os::unix::fs::PermissionsExt};

use axum::{extract::State, Json};
use ralphx_lib::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    DelegatedSessionId, IdeationAnalysisBaseRefKind, IdeationSession, Persona, PersonaId,
    PersonaStatus, Project, ProjectId, SessionPurpose,
};
use ralphx_lib::http_server::delegation::{DelegationHistoryEntry, DelegationJobSnapshot};
use ralphx_lib::http_server::handlers::{
    build_delegated_task_completed_payload, build_delegated_task_started_payload, cancel_delegate,
    get_delegated_session_status, start_delegate, wait_delegate,
};
use ralphx_lib::http_server::types::{
    DelegateCancelRequest, DelegateStartRequest, DelegateWaitRequest, DelegatedRunSummary,
    HttpServerState,
};
use tempfile::TempDir;
use tokio::sync::Mutex;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a repo-root parent")
        .to_path_buf()
}

fn codex_cli_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn prepend_fake_codex_to_path(fake_codex_path: &Path) -> crate::support::env::EnvVarGuard {
    crate::support::env::prepend_to_path(
        fake_codex_path
            .parent()
            .expect("fake codex script should have parent dir"),
    )
}

fn install_fake_codex_cli() -> (TempDir, PathBuf) {
    let tempdir = TempDir::new().expect("tempdir");
    let script_path = tempdir.path().join("codex");
    let script = r#"#!/bin/sh
if [ "$1" = "--help" ]; then
cat <<'EOF'
Codex CLI

Commands:
  exec        Run Codex non-interactively [aliases: e]
  mcp         Manage external MCP servers for Codex
  resume      Resume a previous interactive session

Options:
  -c, --config <key=value>
  -m, --model <MODEL>
  -s, --sandbox <SANDBOX_MODE>
      --search
      --add-dir <DIR>
EOF
exit 0
fi

if [ "$1" = "--version" ]; then
echo "codex-cli 0.116.0"
exit 0
fi

if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
cat <<'EOF'
Run Codex non-interactively

Usage: codex exec [OPTIONS] [PROMPT] [COMMAND]

Options:
  -c, --config <key=value>
  -m, --model <MODEL>
  -s, --sandbox <SANDBOX_MODE>
      --add-dir <DIR>
      --json
  -C, --cd <DIR>
      --skip-git-repo-check
EOF
exit 0
fi

if [ "$1" = "exec" ]; then
if [ -n "$RALPHX_TEST_CODEX_ARGS_PATH" ]; then
  printf '%s\n' "$@" > "$RALPHX_TEST_CODEX_ARGS_PATH"
fi
printf '%s\n' '{"type":"thread.started","thread_id":"delegation-thread-1"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"MOCK_COMPLETION"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":11,"cached_input_tokens":2,"output_tokens":7}}'
exit 0
fi

echo "unsupported invocation" >&2
exit 2
"#;
    fs::write(&script_path, script).expect("write fake codex cli");
    let mut permissions = fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("chmod fake codex cli");
    (tempdir, script_path)
}

async fn seed_bound_active_project_persona(
    state: &HttpServerState,
    project_id: &ProjectId,
    persona_id: &str,
    body: &str,
) {
    let now = chrono::Utc::now();
    let persona = Persona {
        id: PersonaId::from(persona_id),
        slug: persona_id.to_string(),
        name: "Delegation isolation persona".to_string(),
        description: "Must not reach delegated children".to_string(),
        content: body.to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: format!("{persona_id}-hash"),
        source_session_id: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    state
        .app_state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed active persona");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.persona_id = Some(persona.id.to_string());
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("bind active persona to a same-project conversation");
}

fn build_state(app_state: Arc<AppState>) -> HttpServerState {
    let execution_state = Arc::new(ExecutionState::new());
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state,
        execution_state,
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

#[test]
fn delegate_start_request_accepts_legacy_message_alias_for_prompt() {
    let parsed: DelegateStartRequest = serde_json::from_str(
        r#"{
            "agent_name": "ralphx:ralphx-ideation-specialist-intent",
            "message": "SESSION_ID: parent\nAnalyze intent alignment."
        }"#,
    )
    .expect("legacy verifier payload should deserialize");

    assert_eq!(
        parsed.prompt,
        "SESSION_ID: parent\nAnalyze intent alignment."
    );
}

async fn create_parent_session(state: &HttpServerState) -> IdeationSession {
    create_parent_session_in_working_directory(state, &repo_root()).await
}

async fn create_parent_session_in_working_directory(
    state: &HttpServerState,
    working_directory: &std::path::Path,
) -> IdeationSession {
    let project = Project::new(
        "Delegation Test Project".to_string(),
        working_directory.display().to_string(),
    );
    let project_id = project.id.clone();
    state.app_state.project_repo.create(project).await.unwrap();

    let session = IdeationSession::builder()
        .project_id(project_id)
        .title("Delegation Parent")
        .cross_project_checked(true)
        .build();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap()
}

async fn create_project_agent_workspace(
    state: &HttpServerState,
    worktree_parent: &Path,
) -> (Project, ChatConversation, AgentConversationWorkspace) {
    let mut project = Project::new(
        "Delegation Agent Workspace Project".to_string(),
        repo_root().display().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.display().to_string());
    let project = state.app_state.project_repo.create(project).await.unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.provider_harness = Some(AgentHarnessKind::Codex);
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let worktree_path =
        resolve_agent_conversation_workspace_path(&project, &conversation.id).unwrap();
    let safe_worktree_path = ralphx_lib::utils::path_safety::validate_absolute_non_root_path(
        &worktree_path,
        "test agent workspace",
    )
    .unwrap();
    fs::create_dir_all(safe_worktree_path.join(".git")).expect("create fake workspace git marker");
    let workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/agent-workspace-delegation".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    (project, conversation, workspace)
}

fn install_runtime_plugin_dir() -> (TempDir, PathBuf) {
    let tempdir = TempDir::new().expect("tempdir");
    let plugin_dir = tempdir.path().join("plugins/app");
    fs::create_dir_all(&plugin_dir).expect("create temp plugin dir");
    let source_plugin_dir = repo_root().join("plugins/app");

    for entry in fs::read_dir(&source_plugin_dir).expect("read source plugin dir") {
        let entry = entry.expect("source plugin entry");
        symlink_path(entry.path(), plugin_dir.join(entry.file_name()));
    }

    (tempdir, plugin_dir)
}

#[tokio::test]
async fn test_delegate_start_creates_delegated_session_and_completes_with_mock_client() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;
    let parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(parent.id.clone()))
        .await
        .unwrap();
    let parent_conversation_id = parent_conversation.id.as_str();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: Some("turn-42".to_string()),
            parent_message_id: Some("msg-99".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-parent-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Review the proposal set and summarize the main implementation risks."
                .to_string(),
            title: Some("Delegated Risk Review".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(start.parent_context_type, "ideation");
    assert_eq!(start.parent_context_id, parent.id.as_str());
    assert_eq!(start.parent_turn_id.as_deref(), Some("turn-42"));
    assert_eq!(start.parent_message_id.as_deref(), Some("msg-99"));
    assert_eq!(start.parent_tool_use_id.as_deref(), Some("toolu-parent-1"));
    assert_eq!(start.agent_name, "ralphx-ideation-specialist-backend");
    assert_eq!(start.harness, "codex");
    assert_eq!(start.status, "running");
    assert_ne!(start.delegated_session_id, parent.id.as_str());
    assert_eq!(
        start.parent_conversation_id.as_deref(),
        Some(parent_conversation_id.as_str())
    );
    assert!(start.delegated_conversation_id.is_some());
    assert!(start.delegated_agent_run_id.is_some());
    assert_eq!(start.history.len(), 1);
    assert_eq!(start.history[0].status, "running");

    let delegated_id = DelegatedSessionId::from_string(start.delegated_session_id.clone());
    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&delegated_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.parent_context_type, "ideation");
    assert_eq!(delegated.parent_context_id, parent.id.as_str());
    assert_eq!(delegated.status, "running");

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: start.job_id.clone(),
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(true),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("delegation job should settle")
    };

    assert_eq!(waited.job_id, start.job_id);
    assert_eq!(waited.status, "completed");
    assert_eq!(waited.content.as_deref(), Some("MOCK_COMPLETION"));
    assert!(waited.error.is_none());
    assert_eq!(waited.parent_turn_id.as_deref(), Some("turn-42"));
    assert_eq!(waited.parent_message_id.as_deref(), Some("msg-99"));
    assert_eq!(waited.parent_tool_use_id.as_deref(), Some("toolu-parent-1"));
    assert_eq!(
        waited
            .history
            .iter()
            .map(|entry| entry.status.as_str())
            .collect::<Vec<_>>(),
        vec!["running", "completed"]
    );
    let delegated_status = waited
        .delegated_status
        .expect("delegated status should be hydrated");
    assert_eq!(delegated_status.session.id, waited.delegated_session_id);
    assert_eq!(delegated_status.session.parent_context_type, "ideation");
    assert_eq!(
        delegated_status.session.parent_context_id,
        parent.id.as_str()
    );
    assert_eq!(delegated_status.session.status, "completed");
    assert_eq!(delegated_status.agent_state.estimated_status, "completed");
    assert_eq!(
        delegated_status.conversation_id.as_deref(),
        waited.delegated_conversation_id.as_deref()
    );
    let latest_run = delegated_status.latest_run.expect("latest delegated run");
    assert_eq!(
        Some(latest_run.agent_run_id.as_str()),
        waited.delegated_agent_run_id.as_deref()
    );
    assert_eq!(latest_run.status, "completed");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.upstream_provider.as_deref(), Some("openai"));
    assert_eq!(latest_run.logical_model.as_deref(), Some("gpt-5.4-mini"));
    let recent_messages = delegated_status
        .recent_messages
        .expect("delegated status should expose handoff messages when requested");
    assert_eq!(recent_messages.len(), 1);
    assert_eq!(recent_messages[0].content, "MOCK_COMPLETION");

    let delegated_after = state
        .app_state
        .delegated_session_repo
        .get_by_id(&delegated_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated_after.status, "completed");
    assert!(delegated_after.completed_at.is_some());

    assert_eq!(latest_run.input_tokens, Some(11));
    assert_eq!(latest_run.cache_read_tokens, Some(2));
    assert_eq!(latest_run.output_tokens, Some(7));
}

#[tokio::test]
async fn delegate_start_child_command_excludes_bound_project_persona() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_args_path = fake_codex_dir.path().join("delegated-child-args.txt");
    let _captured_args_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_ARGS_PATH",
        captured_args_path.clone(),
    );
    let _persona_flag_guard =
        crate::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "true");
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;
    let persona_body = "DELEGATION PERSONA BODY MUST NOT REACH CHILD";
    seed_bound_active_project_persona(
        &state,
        &parent.project_id,
        "delegation-cross-contamination-persona",
        persona_body,
    )
    .await;

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: Some("turn-persona-isolation".to_string()),
            parent_message_id: Some("msg-persona-isolation".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-persona-isolation".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Inspect delegated child persona isolation.".to_string(),
            title: Some("Delegated persona isolation".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("delegated child should spawn")
    .0;

    for _ in 0..20 {
        let status = wait_delegate(
            State(state.clone()),
            Json(DelegateWaitRequest {
                job_id: start.job_id.clone(),
                include_delegated_status: Some(false),
                include_child_status: None,
                include_messages: Some(false),
                message_limit: None,
            }),
        )
        .await
        .expect("delegation status should load")
        .0;
        if status.status != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let captured_args = fs::read_to_string(&captured_args_path)
        .expect("delegated child fake CLI should capture the final command arguments");
    assert!(
        !captured_args.contains("<ralphx_agent_persona>"),
        "delegated child command must not include a persona block: {captured_args}"
    );
    assert!(
        !captured_args.contains(persona_body),
        "delegated child command must not include the bound persona body: {captured_args}"
    );
}

#[tokio::test]
async fn test_delegate_start_from_project_agent_workspace_without_parent_session() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let (project, parent_conversation, _workspace) =
        create_project_agent_workspace(&state, worktree_parent.path()).await;
    let parent_conversation_id = parent_conversation.id.as_str();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-project".to_string()),
            parent_message_id: Some("msg-project".to_string()),
            parent_conversation_id: Some(parent_conversation_id.clone()),
            parent_tool_use_id: Some("toolu-project-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect the workspace and summarize the requested evidence.".to_string(),
            title: Some("Delegated Project Workspace Inspection".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(start.parent_context_type, "project");
    assert_eq!(start.parent_context_id, project.id.as_str());
    assert_eq!(
        start.parent_conversation_id.as_deref(),
        Some(parent_conversation_id.as_str())
    );
    assert_eq!(start.harness, "codex");

    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(
            start.delegated_session_id.clone(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.parent_context_type, "project");
    assert_eq!(delegated.parent_context_id, project.id.as_str());
    assert_eq!(delegated.project_id, project.id);
    assert_eq!(delegated.harness, AgentHarnessKind::Codex);
}

#[tokio::test]
async fn test_get_delegated_session_status_exposes_parent_context() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-plan-verifier".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-plan-critic-completeness".to_string(),
            prompt: "Publish a verification finding.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    let status = get_delegated_session_status(
        State(state),
        axum::extract::Path(start.delegated_session_id.clone()),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(status.session.id, start.delegated_session_id);
    assert_eq!(status.session.parent_context_type, "ideation");
    assert_eq!(status.session.parent_context_id, parent.id.as_str());
}

#[tokio::test]
async fn test_delegate_start_uses_verifier_subagent_lane_model_when_model_is_omitted() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-plan-verifier".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-plan-critic-completeness".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: start.job_id.clone(),
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(false),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("delegation job should settle")
    };

    let latest_run = waited
        .delegated_status
        .and_then(|status| status.latest_run)
        .expect("latest delegated run");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.logical_model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(latest_run.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        latest_run.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );
}

#[tokio::test]
async fn test_delegate_start_rejects_unknown_agent_name() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: Some("turn-bad".to_string()),
            parent_message_id: Some("msg-bad".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-does-not-exist".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("Unknown canonical agent"));
}

#[tokio::test]
async fn test_delegate_start_rejects_missing_caller_agent_name() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: None,
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("caller_agent_name"));
}

#[tokio::test]
async fn test_delegate_start_rejects_disallowed_target_for_caller() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-execution-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::FORBIDDEN);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("may not delegate"));
}

#[tokio::test]
async fn test_delegate_start_enforces_profile_specific_allowed_targets() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: Some("plan".to_string()),
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::FORBIDDEN);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("may not delegate"));
}

#[tokio::test]
async fn test_delegate_start_infers_parent_session_from_verification_child_context() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-plan-verifier".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-plan-critic-completeness".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(start.parent_context_id, parent.id.as_str());
}

#[tokio::test]
async fn test_delegate_start_verifier_context_survives_external_generated_plugin_dir() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let (_runtime_plugin_root, runtime_plugin_dir) = install_runtime_plugin_dir();
    let target_project_root = TempDir::new().expect("temp target project");
    let generated_plugin_root = TempDir::new().expect("temp generated plugin root");
    let generated_plugin_dir = generated_plugin_root.path().join("generated/claude-plugin");
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let _runtime_plugin_guard =
        ralphx_lib::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            runtime_plugin_dir,
            generated_plugin_dir.clone(),
        );
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent =
        create_parent_session_in_working_directory(&state, target_project_root.path()).await;

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-plan-verifier".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-plan-critic-completeness".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(start.parent_context_id, parent.id.as_str());
    assert!(
        generated_plugin_dir.exists(),
        "materialized generated plugin dir should exist for external desktop-style layouts"
    );
}

#[tokio::test]
async fn test_delegate_start_uses_verifier_subagent_harness_when_harness_is_omitted() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    state
        .app_state
        .agent_lane_settings_repo
        .upsert_global(
            AgentLane::IdeationVerifierSubagent,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-5.4-mini".to_string()),
                effort: None,
                approval_policy: Some("never".to_string()),
                sandbox_mode: Some("danger-full-access".to_string()),
            },
        )
        .await
        .expect("verifier subagent lane upsert should succeed");

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-plan-verifier".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-plan-critic-completeness".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: start.job_id.clone(),
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(false),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("delegation job should settle")
    };

    let latest_run = waited
        .delegated_status
        .as_ref()
        .and_then(|status| status.latest_run.as_ref())
        .expect("latest delegated run");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        latest_run.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );

    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(
            start.delegated_session_id.clone(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.harness, AgentHarnessKind::Codex);
}

#[cfg(unix)]
fn symlink_path(source: impl AsRef<std::path::Path>, target: impl AsRef<std::path::Path>) {
    std::os::unix::fs::symlink(source, target).expect("create symlink");
}

#[cfg(windows)]
fn symlink_path(source: impl AsRef<std::path::Path>, target: impl AsRef<std::path::Path>) {
    let source = source.as_ref();
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, target).expect("create dir symlink");
    } else {
        std::os::windows::fs::symlink_file(source, target).expect("create file symlink");
    }
}

#[tokio::test]
async fn test_delegate_start_uses_ideation_subagent_harness_when_harness_is_omitted() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    state
        .app_state
        .agent_lane_settings_repo
        .upsert_global(
            AgentLane::IdeationSubagent,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-5.4-mini".to_string()),
                effort: None,
                approval_policy: Some("never".to_string()),
                sandbox_mode: Some("danger-full-access".to_string()),
            },
        )
        .await
        .expect("ideation subagent lane upsert should succeed");

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-ideation".to_string()),
            parent_message_id: Some("msg-ideation".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-ideation-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-ideation-specialist-intent".to_string(),
            prompt: "Analyze the plan intent and summarize any scope drift risks.".to_string(),
            title: Some("Delegated Intent Specialist".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: start.job_id.clone(),
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(false),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("delegation job should settle")
    };

    let latest_run = waited
        .delegated_status
        .as_ref()
        .and_then(|status| status.latest_run.as_ref())
        .expect("latest delegated run");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        latest_run.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );

    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(
            start.delegated_session_id.clone(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.harness, AgentHarnessKind::Codex);
}

#[tokio::test]
async fn test_delegate_start_links_parent_conversation_to_verification_child_chat() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    let parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(parent.id.clone()))
        .await
        .unwrap();

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let verification_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(
            verification_child.id.clone(),
        ))
        .await
        .unwrap();
    let parent_conversation_id = parent_conversation.id.as_str();
    let verification_conversation_id = verification_conversation.id.as_str();

    let start = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-plan-verifier".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-plan-critic-completeness".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(
        start.parent_conversation_id.as_deref(),
        Some(verification_conversation_id.as_str())
    );
    assert_ne!(
        start.parent_conversation_id.as_deref(),
        Some(parent_conversation_id.as_str())
    );
}

#[tokio::test]
async fn test_delegate_start_rejects_parent_session_mismatch_against_verification_child_context() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let other_parent = create_parent_session(&state).await;

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-plan-verifier".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: Some(other_parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            agent_name: "ralphx-plan-critic-completeness".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("does not match caller context parent"));
}

#[tokio::test]
async fn test_delegate_cancel_rejects_unknown_job() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));

    let error = cancel_delegate(
        State(state),
        Json(DelegateCancelRequest {
            job_id: "missing-job".to_string(),
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::NOT_FOUND);
}

#[test]
fn test_build_delegated_task_started_payload_uses_parent_lineage_and_delegated_metadata() {
    let snapshot = DelegationJobSnapshot {
        job_id: "job-123".to_string(),
        parent_context_type: "ideation".to_string(),
        parent_context_id: "parent-session-1".to_string(),
        parent_turn_id: Some("turn-1".to_string()),
        parent_message_id: Some("msg-1".to_string()),
        parent_conversation_id: Some("parent-conv-1".to_string()),
        parent_tool_use_id: Some("toolu-parent-1".to_string()),
        delegated_session_id: "delegated-session-1".to_string(),
        delegated_conversation_id: Some("delegated-conv-1".to_string()),
        delegated_agent_run_id: Some("run-1".to_string()),
        agent_name: "ralphx-execution-reviewer".to_string(),
        harness: "codex".to_string(),
        status: "running".to_string(),
        content: None,
        error: None,
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: None,
        history: vec![DelegationHistoryEntry {
            status: "running".to_string(),
            timestamp: "2026-04-12T10:00:00Z".to_string(),
            detail: None,
        }],
        delegated_status: None,
    };

    let payload = build_delegated_task_started_payload(
        &snapshot,
        Some("gpt-5.4"),
        Some("high"),
        Some("never"),
        Some("danger-full-access"),
        42,
    )
    .expect("parent linkage should produce a payload");

    assert_eq!(payload.tool_use_id, "toolu-parent-1");
    assert_eq!(payload.tool_name, "delegate_start");
    assert_eq!(
        payload.description.as_deref(),
        Some("ralphx-execution-reviewer")
    );
    assert_eq!(payload.subagent_type.as_deref(), Some("delegated"));
    assert_eq!(payload.delegated_job_id.as_deref(), Some("job-123"));
    assert_eq!(
        payload.delegated_session_id.as_deref(),
        Some("delegated-session-1")
    );
    assert_eq!(
        payload.delegated_conversation_id.as_deref(),
        Some("delegated-conv-1")
    );
    assert_eq!(payload.delegated_agent_run_id.as_deref(), Some("run-1"));
    assert_eq!(payload.provider_harness.as_deref(), Some("codex"));
    assert_eq!(payload.logical_model.as_deref(), Some("gpt-5.4"));
    assert_eq!(payload.logical_effort.as_deref(), Some("high"));
    assert_eq!(payload.approval_policy.as_deref(), Some("never"));
    assert_eq!(payload.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(payload.conversation_id, "parent-conv-1");
    assert_eq!(payload.context_type, "ideation");
    assert_eq!(payload.context_id, "parent-session-1");
    assert_eq!(payload.seq, 42);
}

#[test]
fn test_build_delegated_task_completed_payload_uses_latest_run_attribution() {
    let snapshot = DelegationJobSnapshot {
        job_id: "job-456".to_string(),
        parent_context_type: "ideation".to_string(),
        parent_context_id: "parent-session-2".to_string(),
        parent_turn_id: Some("turn-2".to_string()),
        parent_message_id: Some("msg-2".to_string()),
        parent_conversation_id: Some("parent-conv-2".to_string()),
        parent_tool_use_id: Some("toolu-parent-2".to_string()),
        delegated_session_id: "delegated-session-2".to_string(),
        delegated_conversation_id: Some("delegated-conv-2".to_string()),
        delegated_agent_run_id: Some("run-2".to_string()),
        agent_name: "ralphx-execution-reviewer".to_string(),
        harness: "codex".to_string(),
        status: "running".to_string(),
        content: None,
        error: None,
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: None,
        history: vec![DelegationHistoryEntry {
            status: "running".to_string(),
            timestamp: "2026-04-12T10:00:00Z".to_string(),
            detail: None,
        }],
        delegated_status: None,
    };
    let latest_run = DelegatedRunSummary {
        agent_run_id: "run-2".to_string(),
        status: "failed".to_string(),
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: Some("2026-04-12T10:00:05Z".to_string()),
        error_message: Some("Delegated reviewer failed validation".to_string()),
        harness: Some("codex".to_string()),
        provider_session_id: Some("provider-thread-1".to_string()),
        upstream_provider: Some("openai".to_string()),
        provider_profile: Some("openai".to_string()),
        logical_model: Some("gpt-5.4".to_string()),
        effective_model_id: Some("gpt-5.4".to_string()),
        logical_effort: Some("high".to_string()),
        effective_effort: Some("high".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        input_tokens: Some(100),
        output_tokens: Some(40),
        cache_creation_tokens: Some(6),
        cache_read_tokens: Some(2),
        estimated_usd: Some(0.12),
    };

    let payload = build_delegated_task_completed_payload(
        &snapshot,
        Some(&latest_run),
        "failed",
        Some("Delegated reviewer found a blocking issue"),
        Some("Delegated reviewer failed validation"),
        99,
    )
    .expect("parent linkage should produce a payload");

    assert_eq!(payload.tool_use_id, "toolu-parent-2");
    assert_eq!(payload.agent_id.as_deref(), Some("run-2"));
    assert_eq!(payload.status.as_deref(), Some("failed"));
    assert_eq!(payload.total_duration_ms, Some(5000));
    assert_eq!(payload.total_tokens, Some(148));
    assert_eq!(payload.delegated_job_id.as_deref(), Some("job-456"));
    assert_eq!(
        payload.delegated_session_id.as_deref(),
        Some("delegated-session-2")
    );
    assert_eq!(
        payload.delegated_conversation_id.as_deref(),
        Some("delegated-conv-2")
    );
    assert_eq!(payload.delegated_agent_run_id.as_deref(), Some("run-2"));
    assert_eq!(payload.provider_harness.as_deref(), Some("codex"));
    assert_eq!(
        payload.provider_session_id.as_deref(),
        Some("provider-thread-1")
    );
    assert_eq!(payload.upstream_provider.as_deref(), Some("openai"));
    assert_eq!(payload.provider_profile.as_deref(), Some("openai"));
    assert_eq!(payload.logical_model.as_deref(), Some("gpt-5.4"));
    assert_eq!(payload.effective_model_id.as_deref(), Some("gpt-5.4"));
    assert_eq!(payload.logical_effort.as_deref(), Some("high"));
    assert_eq!(payload.effective_effort.as_deref(), Some("high"));
    assert_eq!(payload.approval_policy.as_deref(), Some("never"));
    assert_eq!(payload.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(payload.input_tokens, Some(100));
    assert_eq!(payload.output_tokens, Some(40));
    assert_eq!(payload.cache_creation_tokens, Some(6));
    assert_eq!(payload.cache_read_tokens, Some(2));
    assert_eq!(payload.estimated_usd, Some(0.12));
    assert_eq!(
        payload.text_output.as_deref(),
        Some("Delegated reviewer found a blocking issue")
    );
    assert_eq!(
        payload.error.as_deref(),
        Some("Delegated reviewer failed validation")
    );
    assert_eq!(payload.conversation_id, "parent-conv-2");
    assert_eq!(payload.context_type, "ideation");
    assert_eq!(payload.context_id, "parent-session-2");
    assert_eq!(payload.seq, 99);
}
