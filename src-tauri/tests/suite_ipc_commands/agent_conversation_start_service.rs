use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use ralphx_lib::application::agent_conversation_start_service::{
    AgentConversationStartDeps, AgentConversationStartResult, AgentConversationStartService,
    AgentWorkspaceSourcePullRequestInput, StartAgentConversationInput,
};
use ralphx_lib::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
};
use ralphx_lib::application::automation::provisioning::AutomationRunProvisioner;
use ralphx_lib::application::automation::transition::NoopAutomationEventEmitter;
use ralphx_lib::application::startup_background::AgentConversationAutomationRunStarter;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceBranchMode, AgentConversationWorkspaceMode, Artifact, ArtifactType,
    Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationStatus,
    ChatContextType, ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind,
    IdeationSessionFlow, Persona, PersonaId, PersonaStatus, Project, ProjectId, TaskId,
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

fn git(repo: &Path, args: &[&str]) -> String {
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

fn setup_repo(repo: &Path) {
    std::fs::create_dir_all(repo).expect("repo dir should be created");
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "hello\n").expect("fixture file should be written");
    git(repo, &["add", "README.md"]);
    git(repo, &["commit", "-m", "initial"]);
}

async fn seed_project(
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

fn build_app(
    state: AppState,
    execution_state: Arc<ExecutionState>,
) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(execution_state)
        .manage(Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        ))))
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

fn service_start_input(
    project_id: &ProjectId,
    content: &str,
    mode: &str,
    base_ref: Option<&str>,
    branch_mode: Option<&str>,
    conversation_id: Option<&ChatConversationId>,
    source_pull_request: Option<AgentWorkspaceSourcePullRequestInput>,
) -> StartAgentConversationInput {
    StartAgentConversationInput {
        project_id: project_id.as_str().to_string(),
        content: content.to_string(),
        conversation_id: conversation_id.map(ChatConversationId::as_str),
        parent_conversation_id: None,
        title: None,
        persona_id: None,
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
        team_intent: None,
    }
}

struct CapturingFakeClaude {
    _path_guard: super::support::env::EnvVarGuard,
    _capture_guard: super::support::env::EnvVarGuard,
    _temp_dir: tempfile::TempDir,
    capture_path: PathBuf,
    cli_path: PathBuf,
}

impl CapturingFakeClaude {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("fake CLI directory should be created");
        let capture_path = temp_dir.path().join("captured-prompt.txt");
        let cli_path = temp_dir.path().join("claude");
        std::fs::write(
            &cli_path,
            r#"#!/bin/sh
printf '%s\n' "$@" >> "$RALPHX_PERSONA_START_CAPTURE_PATH"
previous=""
for argument in "$@"; do
  if [ "$previous" = "--append-system-prompt-file" ]; then
    cat "$argument" >> "$RALPHX_PERSONA_START_CAPTURE_PATH"
  fi
  previous="$argument"
done
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
    async fn captured_prompt(&self) -> String {
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

fn enable_personas_for_test() -> super::support::env::EnvVarGuard {
    super::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "true")
}

async fn seed_persona(state: &AppState, id: &str, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    let persona = Persona {
        id: PersonaId::from(id),
        slug: format!("{id}-slug"),
        name: format!("{id} name"),
        description: "start service persona fixture".to_string(),
        content: "Use the requested project voice.".to_string(),
        status,
        version: 1,
        content_hash: format!("{id}-hash"),
        source_session_id: None,
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

async fn start_with_app(
    app: &tauri::App<tauri::test::MockRuntime>,
    input: StartAgentConversationInput,
) -> Result<AgentConversationStartResult, String> {
    let state = app.state::<AppState>();
    let execution_state = app.state::<Arc<ExecutionState>>();
    let team_service = app.state::<Arc<TeamService>>();
    AgentConversationStartService::new(AgentConversationStartDeps {
        state: state.inner(),
        execution_state: execution_state.inner(),
        team_service: Some(team_service.inner().clone()),
        app_handle: app.handle().clone(),
    })
    .start(input)
    .await
}

#[tokio::test]
async fn start_agent_conversation_rejects_persona_builder_mode() {
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let project_id = ProjectId::from_string("project-persona-builder-rejected".to_string());

    let error = start_with_app(
        &app,
        service_start_input(
            &project_id,
            "reserved mode must not start generically",
            "persona_builder",
            None,
            None,
            None,
            None,
        ),
    )
    .await
    .expect_err("generic start must reject PersonaBuilder before any project or workspace work");

    assert!(
        error.contains("PersonaBuilder"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn start_with_persona_persists_binding_and_first_send_includes_persona_block() {
    let _persona_feature = enable_personas_for_test();
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(&state, "project-start-persona", temp.path(), temp.path()).await;
    let persona = seed_persona(&state, "start-persona", PersonaStatus::Active).await;
    let app = build_app(state, Arc::new(ExecutionState::new()));
    let mut input = service_start_input(
        &project.id,
        "Start with a persona",
        "chat",
        None,
        None,
        None,
        None,
    );
    input.persona_id = Some(persona.id.as_str().to_string());
    input.provider_harness = Some("claude".to_string());

    let started = start_with_app(&app, input)
        .await
        .expect("persona-bound conversation should start");
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&started.conversation.id)
        .await
        .expect("conversation lookup should succeed")
        .expect("conversation should persist");
    assert_eq!(stored.persona_id.as_deref(), Some(persona.id.as_str()));

    assert!(
        fake_cli
            .captured_prompt()
            .await
            .contains("<ralphx_agent_persona>"),
        "the start-path override must not suppress the explicit first-send persona"
    );
}

#[tokio::test]
async fn start_input_persona_id_rejected_for_non_project_context() {
    let _persona_feature = enable_personas_for_test();
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-persona-non-project",
        temp.path(),
        temp.path(),
    )
    .await;
    let persona = seed_persona(&state, "non-project-persona", PersonaStatus::Active).await;
    let task_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_task(TaskId::from_string(
            "task-start-persona-non-project".to_string(),
        )))
        .await
        .expect("task conversation fixture should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);
    let mut input = service_start_input(
        &project.id,
        "Do not bind on a task conversation",
        "chat",
        None,
        None,
        Some(&task_conversation.id),
        None,
    );
    input.persona_id = Some(persona.id.as_str().to_string());

    let error = start_with_app(&app, input)
        .await
        .expect_err("persona input must reject non-Project conversations");
    assert!(
        error.contains("Persona bindings require Project conversation context"),
        "unexpected typed context rejection: {error}"
    );
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&task_conversation.id)
        .await
        .expect("task conversation lookup should succeed")
        .expect("task conversation should remain");
    assert!(stored.persona_id.is_none());
}

#[tokio::test]
async fn start_with_persona_flag_off_fails_before_creating_conversation_or_workspace() {
    let _persona_feature =
        super::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "false");
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-persona-flag-off",
        temp.path(),
        temp.path(),
    )
    .await;
    let persona = seed_persona(&state, "feature-off-persona", PersonaStatus::Active).await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);
    let mut input = service_start_input(
        &project.id,
        "Do not create an edit workspace when personas are disabled",
        "edit",
        None,
        None,
        None,
        None,
    );
    input.persona_id = Some(persona.id.as_str().to_string());

    let error = start_with_app(&app, input)
        .await
        .expect_err("persona feature flag must be enforced before setup side effects");

    assert!(
        error.contains("[Personas disabled:"),
        "unexpected error: {error}"
    );
    assert!(app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_context(ChatContextType::Project, project.id.as_str())
        .await
        .expect("conversation lookup should succeed")
        .is_empty());
    assert!(app
        .state::<AppState>()
        .agent_conversation_workspace_repo
        .get_by_project_id(&project.id)
        .await
        .expect("workspace lookup should succeed")
        .is_empty());
}

#[tokio::test]
async fn start_with_draft_or_archived_persona_fails_closed_without_binding() {
    let _persona_feature = enable_personas_for_test();
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-persona-inactive",
        temp.path(),
        temp.path(),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    for status in [PersonaStatus::Draft, PersonaStatus::Archived] {
        let persona = seed_persona(
            app.state::<AppState>().inner(),
            &format!("inactive-start-persona-{status}"),
            status,
        )
        .await;
        let conversation = app
            .state::<AppState>()
            .chat_conversation_repo
            .create(ChatConversation::new_project(project.id.clone()))
            .await
            .expect("seeded conversation should persist");
        let mut input = service_start_input(
            &project.id,
            "Reject inactive persona",
            "chat",
            None,
            None,
            Some(&conversation.id),
            None,
        );
        input.persona_id = Some(persona.id.as_str().to_string());

        let error = start_with_app(&app, input)
            .await
            .expect_err("draft and archived personas must fail closed");
        assert!(
            error.contains("[Persona unavailable:"),
            "unexpected inactive persona error: {error}"
        );
        let stored = app
            .state::<AppState>()
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation lookup should succeed")
            .expect("conversation should remain");
        assert!(stored.persona_id.is_none());
    }
}

#[tokio::test]
async fn start_without_persona_id_unchanged() {
    let _persona_feature = enable_personas_for_test();
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-without-persona",
        temp.path(),
        temp.path(),
    )
    .await;
    let app = build_app(state, Arc::new(ExecutionState::new()));

    let mut input = service_start_input(
        &project.id,
        "Start without a persona",
        "chat",
        None,
        None,
        None,
        None,
    );
    input.provider_harness = Some("claude".to_string());
    let started = start_with_app(&app, input)
        .await
        .expect("persona-free conversation should start");
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&started.conversation.id)
        .await
        .expect("conversation lookup should succeed")
        .expect("conversation should persist");
    assert!(stored.persona_id.is_none());
    assert!(
        !fake_cli
            .captured_prompt()
            .await
            .contains("<ralphx_agent_persona>"),
        "persona-free starts must keep the prior prompt shape"
    );
}

#[tokio::test]
async fn ipc_contract_start_service_pr_backed_local_branch_prepares_isolated_workspace() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-source-pr";
    git(&repo_path, &["checkout", "-b", branch]);
    std::fs::write(repo_path.join("README.md"), "source pr\n")
        .expect("fixture update should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "source pr"]);
    let head_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-success",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let result = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Start from PR",
            "edit",
            Some(branch),
            None,
            None,
            Some(AgentWorkspaceSourcePullRequestInput {
                number: 42,
                url: Some("https://github.com/owner/repo/pull/42".to_string()),
                title: Some("Service source PR".to_string()),
                head_ref_name: branch.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: Some(head_sha.clone()),
            }),
        ),
    )
    .await
    .expect("service start should queue while execution is paused");

    assert!(result.send_result.was_queued);
    let workspace = result.workspace.expect("edit mode creates workspace");
    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );
    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::LocalBranch
    );
    assert_eq!(workspace.base_ref, branch);
    assert_ne!(workspace.branch_name, branch);
    assert_eq!(workspace.publication_pr_number, None);
    assert_eq!(
        workspace
            .source_pull_request
            .as_ref()
            .map(|source| source.number),
        Some(42)
    );
}

#[tokio::test]
async fn ipc_contract_start_service_plan_mode_links_planning_session_for_automation_conversation() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-plan-automation",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let spec = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Automation Spec",
            ArtifactType::Specification,
            "# Automation Spec\n\nKeep the run scoped.",
            "automation-test",
        ))
        .await
        .expect("spec artifact should persist");
    let now = Utc::now();
    let automation = Automation {
        id: AutomationId::from_string("automation-1"),
        project_id: project.id.clone(),
        name: "Spec-backed automation".to_string(),
        status: AutomationStatus::Active,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Build from the spec".to_string(),
        setup_conversation_id: None,
        provider_harness: "codex".to_string(),
        model_id: "gpt-5.4".to_string(),
        logical_effort: Some("high".to_string()),
        run_mode: "edit".to_string(),
        base_ref_kind: "local_branch".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("main".to_string()),
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Author the automation run plan".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: Some(spec.id.as_str().to_string()),
        created_at: now,
        updated_at: now,
    };
    state
        .automation_repo
        .create(automation.clone())
        .await
        .expect("automation should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app_state = state.clone();
    let app = build_app(state, Arc::clone(&execution_state));
    let team_service = app.state::<Arc<TeamService>>().inner().clone();
    let starter = Arc::new(AgentConversationAutomationRunStarter::new(
        app_state.clone(),
        Arc::clone(&execution_state),
        Some(team_service),
        app.handle().clone(),
    ));
    let provisioner = AutomationRunProvisioner::new(
        Arc::clone(&app_state.automation_repo),
        Arc::clone(&app_state.automation_run_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_conversation_workspace_repo),
        starter,
        Arc::new(NoopAutomationEventEmitter),
        Arc::clone(&app_state.artifact_repo),
        app_state.notification_service(),
    );

    let started = provisioner
        .provision_first_run(&automation)
        .await
        .expect("plan-mode automation start should queue while execution is paused")
        .expect("first automation run should be provisioned");

    let conversation_id = *started
        .conversation_id
        .as_ref()
        .expect("automation run should link conversation");
    let state = app.state::<AppState>();
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace query should succeed")
        .expect("plan workspace should exist");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    let session_id = workspace
        .linked_ideation_session_id
        .as_ref()
        .expect("plan workspace should link a Planning session");
    let session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await
        .expect("session query should succeed")
        .expect("planning session should exist");
    assert_eq!(session.session_flow, IdeationSessionFlow::Planning);
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation query should succeed")
        .expect("conversation should still exist");
    assert_eq!(conversation.automation_run_id, Some(started.id.clone()));
    assert_eq!(
        conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Plan)
    );
    let queued_messages = state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str());
    assert_eq!(queued_messages.len(), 1);
    let queued_reference = queued_messages[0]
        .composer_artifact_references
        .first()
        .expect("automation spec reference should be queued");
    assert_eq!(queued_reference.kind, "spec");
    assert_eq!(queued_reference.artifact_id, spec.id.as_str());
    assert!(queued_reference.session_id.is_none());
}

#[tokio::test]
async fn ipc_contract_start_service_linked_workspace_conflict_archives_supplied_draft() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-linked-conflict";
    git(&repo_path, &["checkout", "-b", branch]);
    git(&repo_path, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-conflict",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let existing = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("existing conversation should persist");
    let mut draft = ChatConversation::new_project(project.id.clone());
    draft.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    let draft = state
        .chat_conversation_repo
        .create(draft)
        .await
        .expect("draft conversation should persist");
    let workspace = prepare_agent_conversation_workspace(
        &project,
        &existing.id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some(branch.to_string()),
            display_name: Some(branch.to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("linked workspace should prepare");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("linked workspace should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let error = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Start linked conflict",
            "edit",
            Some(branch),
            Some("linked"),
            Some(&draft.id),
            None,
        ),
    )
    .await
    .expect_err("linked branch conflict should fail before creating a chat");

    assert!(
        error.contains("[ralphx:linked_setup_failure]"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains(branch) && error.contains(&existing.id.as_str()),
        "error should explain the conflict: {error}"
    );
    let stored_draft = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&draft.id)
        .await
        .expect("draft should load")
        .expect("draft should still exist");
    assert!(
        stored_draft.archived_at.is_some(),
        "supplied failed draft should be archived"
    );
    let draft_workspace = app
        .state::<AppState>()
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&draft.id)
        .await
        .expect("draft workspace lookup should succeed");
    assert!(draft_workspace.is_none());
    let conversations = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_context(ChatContextType::Project, project.id.as_str())
        .await
        .expect("project conversations should load");
    assert_eq!(conversations.len(), 1);
}

#[tokio::test]
async fn ipc_contract_start_service_archives_seeded_draft_on_linked_workspace_setup_failure() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-primary-linked";
    git(&repo_path, &["checkout", "-b", branch]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-archive",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let mut draft = ChatConversation::new_project(project.id.clone());
    draft.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    let draft = state
        .chat_conversation_repo
        .create(draft)
        .await
        .expect("draft conversation should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let error = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Start linked primary checkout",
            "edit",
            Some(branch),
            Some("linked"),
            Some(&draft.id),
            None,
        ),
    )
    .await
    .expect_err("primary checkout linked setup should fail");

    assert!(
        error.contains("[ralphx:linked_setup_failure]"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("checked out in the project root"),
        "error should explain the checkout conflict: {error}"
    );
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&draft.id)
        .await
        .expect("draft should load")
        .expect("draft should still exist");
    assert!(stored.archived_at.is_some());
    let workspace = app
        .state::<AppState>()
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&draft.id)
        .await
        .expect("workspace lookup should succeed");
    assert!(workspace.is_none());
}
