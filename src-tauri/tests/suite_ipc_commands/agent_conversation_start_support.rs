pub(super) use std::path::{Path, PathBuf};
pub(super) use std::process::Command;
pub(super) use std::sync::Arc;
pub(super) use std::time::Duration;

pub(super) use chrono::Utc;
pub(super) use ralphx_lib::application::agent_conversation_start_service::{
    AgentConversationStartDeps, AgentConversationStartResult, AgentConversationStartService,
    AgentWorkspaceSourcePullRequestInput, StartAgentConversationInput,
};
pub(super) use ralphx_lib::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
};
pub(super) use ralphx_lib::application::automation::provisioning::AutomationRunProvisioner;
pub(super) use ralphx_lib::application::automation::transition::NoopAutomationEventEmitter;
pub(super) use ralphx_lib::application::builder_attachment_materializer::materialized_builder_attachment_path;
pub(super) use ralphx_lib::application::chat_attachment_service::ChatAttachmentService;
pub(super) use ralphx_lib::application::personas::{PersonaService, SavePersonaDraftInput};
pub(super) use ralphx_lib::application::seeded_agent_conversation_abort::abort_seeded_agent_conversation;
pub(super) use ralphx_lib::application::standalone_workspace::{
    create_workspace, standalone_workspace_path, standalone_workspaces_root,
    sweep_orphaned_standalone_workspaces,
};
pub(super) use ralphx_lib::application::startup_background::AgentConversationAutomationRunStarter;
pub(super) use ralphx_lib::application::{AppPaths, AppState};
pub(super) use ralphx_lib::commands::conversation_folder_reference_commands::{
    add_conversation_folder_reference_for_state, AddConversationFolderReferenceInput,
};
pub(super) use ralphx_lib::commands::unified_chat_commands::{
    create_agent_conversation, CreateAgentConversationInput,
};
pub(super) use ralphx_lib::commands::ExecutionState;
pub(super) use ralphx_lib::domain::agents::{
    AgentHarnessKind, ManualRoleDefault, ManualServiceTier, RoutingRole,
};
pub(super) use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceBranchMode, AgentConversationWorkspaceMode, AgentRun,
    AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewMonitorStatus, Artifact, ArtifactType,
    Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationStatus,
    ChatContextType, ChatConversation, ChatConversationId, ChatMessage, ChatMessageId,
    ConversationFolderReference, CoordinationMode, IdeationAnalysisBaseRefKind,
    IdeationSessionFlow, MessageRole, Persona, PersonaId, PersonaStatus, Project, ProjectId,
    TaskId, TeamIntent,
};
pub(super) use ralphx_lib::infrastructure::agents::{
    reset_agent_personas_override_for_test, reset_standalone_conversations_override_for_test,
    set_agent_personas_override, set_standalone_conversations_override,
};
pub(super) use ralphx_lib::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
pub(super) use ralphx_lib::testing::SqliteTestDb;
pub(super) use ralphx_lib::utils::path_safety::validate_absolute_non_root_path;
pub(super) use tauri::test::{mock_builder, mock_context, noop_assets};
pub(super) use tauri::Manager;

pub(super) use super::support::fake_codex::FakeCodex;

pub(super) fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(super) fn setup_repo(repo: &Path) {
    std::fs::create_dir_all(repo).expect("repo dir should be created");
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "hello\n").expect("fixture file should be written");
    git(repo, &["add", "README.md"]);
    git(repo, &["commit", "-m", "initial"]);
}

pub(super) async fn seed_project(
    state: &AppState,
    project_id: &str,
    repo_path: &Path,
    worktree_parent: &Path,
) -> Project {
    let mut project = Project::new(
        format!("Start service {project_id}"),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string(project_id.to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project)
        .await
        .expect("project should persist")
}

pub(super) fn build_app(
    state: AppState,
    execution_state: Arc<ExecutionState>,
) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(execution_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

pub(super) fn service_start_input(
    project_id: &ProjectId,
    content: &str,
    mode: &str,
    base_ref: Option<&str>,
    branch_mode: Option<&str>,
    conversation_id: Option<&ChatConversationId>,
    source_pull_request: Option<AgentWorkspaceSourcePullRequestInput>,
) -> StartAgentConversationInput {
    StartAgentConversationInput {
        project_id: Some(project_id.as_str().to_string()),
        content: content.to_string(),
        conversation_id: conversation_id.map(ChatConversationId::as_str),
        parent_conversation_id: None,
        title: None,
        persona_id: None,
        source_persona_id: None,
        provider_harness: None,
        model_override: None,
        logical_effort: None,
        codex_fast_mode: None,
        mode: Some(mode.to_string()),
        base_ref_kind: Some("local_branch".to_string()),
        base_branch_mode: branch_mode.map(str::to_string),
        base_ref: base_ref.map(str::to_string),
        base_display_name: base_ref.map(str::to_string),
        base_source_pull_request: source_pull_request,
        composer_project_references: Vec::new(),
        composer_integration_references: Vec::new(),
        composer_artifact_references: Vec::new(),
        composer_selection_snapshot: None,
        team_intent: None,
    }
}

pub(super) fn standalone_start_input(
    content: &str,
    mode: Option<&str>,
    conversation_id: Option<&ChatConversationId>,
    team_intent: Option<TeamIntent>,
    parent_conversation_id: Option<&str>,
) -> StartAgentConversationInput {
    StartAgentConversationInput {
        project_id: None,
        content: content.to_string(),
        conversation_id: conversation_id.map(ChatConversationId::as_str),
        parent_conversation_id: parent_conversation_id.map(str::to_string),
        title: None,
        persona_id: None,
        source_persona_id: None,
        provider_harness: None,
        model_override: None,
        logical_effort: None,
        codex_fast_mode: None,
        mode: mode.map(str::to_string),
        base_ref_kind: None,
        base_branch_mode: None,
        base_ref: None,
        base_display_name: None,
        base_source_pull_request: None,
        composer_project_references: Vec::new(),
        composer_integration_references: Vec::new(),
        composer_artifact_references: Vec::new(),
        composer_selection_snapshot: None,
        team_intent,
    }
}

pub(super) fn manual_role_default(harness: AgentHarnessKind) -> ManualRoleDefault {
    ManualRoleDefault {
        harness,
        model: None,
        effort: None,
        service_tier: ManualServiceTier::ProviderDefault,
        coordination_mode: None,
        persona_id: None,
        approval_policy: None,
        sandbox_mode: None,
        atlassian_access: None,
    }
}

pub(super) async fn configure_provider_cli(
    state: &AppState,
    harness: AgentHarnessKind,
    cli_path: impl Into<String>,
) {
    let mut settings = state
        .agent_provider_settings_repo
        .get(harness)
        .await
        .expect("provider settings should load")
        .expect("provider settings should exist");
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(cli_path.into());
    state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("custom provider CLI should persist");
}

pub(super) struct StandaloneConversationsFlagOverrideReset;

impl Drop for StandaloneConversationsFlagOverrideReset {
    fn drop(&mut self) {
        reset_standalone_conversations_override_for_test();
    }
}

pub(super) struct PersonaFlagsOverrideReset;

impl Drop for PersonaFlagsOverrideReset {
    fn drop(&mut self) {
        reset_agent_personas_override_for_test();
        reset_standalone_conversations_override_for_test();
    }
}

pub(super) struct CapturingFakeClaude {
    _path_guard: super::support::env::EnvVarGuard,
    _capture_guard: super::support::env::EnvVarGuard,
    _temp_dir: tempfile::TempDir,
    capture_path: PathBuf,
    pub(super) cli_path: PathBuf,
}

impl CapturingFakeClaude {
    pub(super) fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("fake CLI directory should be created");
        let capture_path = temp_dir.path().join("captured-prompt.txt");
        let cli_path = temp_dir.path().join("claude");
        std::fs::write(
            &cli_path,
            r#"#!/bin/sh
printf '%s\n' "$@" >> "$RALPHX_PERSONA_START_CAPTURE_PATH"
pwd >> "$RALPHX_PERSONA_START_CAPTURE_PATH"
previous=""
for argument in "$@"; do
  if [ "$previous" = "--append-system-prompt-file" ]; then
    cat "$argument" >> "$RALPHX_PERSONA_START_CAPTURE_PATH"
  fi
  if [ "$previous" = "--mcp-config" ]; then
    cat "$argument" >> "$RALPHX_PERSONA_START_CAPTURE_PATH"
  fi
  previous="$argument"
done
cat >/dev/null
"#,
        )
        .expect("fake CLI should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(&cli_path)
                .expect("fake CLI metadata should load")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&cli_path, permissions)
                .expect("fake CLI should be executable");
        }

        Self {
            _path_guard: super::support::env::prepend_to_path(temp_dir.path()),
            _capture_guard: super::support::env::EnvVarGuard::set(
                "RALPHX_PERSONA_START_CAPTURE_PATH",
                capture_path.clone(),
            ),
            _temp_dir: temp_dir,
            capture_path,
            cli_path,
        }
    }

    /// Waits for a real send spawn. The harness probes the pinned binary with
    /// `--version`/`--help` first, so "file is non-empty" is not enough — poll
    /// until a send-shaped invocation (composed system prompt) lands, then
    /// return everything captured so far. On timeout, returns whatever was
    /// captured so assertions produce a useful diff instead of a hang.
    pub(super) async fn captured_prompt(&self) -> String {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let captured = std::fs::read_to_string(&self.capture_path).unwrap_or_default();
            if captured.contains("--append-system-prompt") {
                // One more settle poll so the prompt-file `cat` finishes.
                tokio::time::sleep(Duration::from_millis(100)).await;
                return std::fs::read_to_string(&self.capture_path).unwrap_or(captured);
            }
            if tokio::time::Instant::now() >= deadline {
                return captured;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

pub(super) fn enable_personas_for_test() -> super::support::env::EnvVarGuard {
    super::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "true")
}

pub(super) async fn seed_persona(state: &AppState, id: &str, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    let persona = Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: None,
        slug: format!("{id}-slug"),
        name: format!("{id} name"),
        description: "start service persona fixture".to_string(),
        content: format!(
            "---\nname: {id}-slug\nkind: persona\ndescription: Start service persona fixture\n---\nUse the requested project voice."
        ),
        status,
        version: 1,
        content_hash: format!("{id}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("persona fixture should persist");
    persona
}

pub(super) async fn seed_project_persona(
    state: &AppState,
    id: &str,
    project_id: &ProjectId,
) -> Persona {
    let now = Utc::now();
    let persona = Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: Some(project_id.clone()),
        slug: format!("{id}-slug"),
        name: format!("{id} name"),
        description: "scoped start service persona fixture".to_string(),
        content: format!(
            "---\nname: {id}-slug\nkind: persona\ndescription: Scoped start service persona fixture\n---\nUse the scoped project voice."
        ),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: format!("{id}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    state.persona_repo.create(persona.clone()).await.unwrap();
    persona
}

pub(super) async fn start_with_app(
    app: &tauri::App<tauri::test::MockRuntime>,
    input: StartAgentConversationInput,
) -> Result<AgentConversationStartResult, String> {
    let state = app.state::<AppState>();
    let execution_state = app.state::<Arc<ExecutionState>>();
    AgentConversationStartService::new(AgentConversationStartDeps {
        state: state.inner(),
        execution_state: execution_state.inner(),
        events: Arc::clone(&state.events),
    })
    .start(input)
    .await
}
