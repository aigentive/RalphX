use async_trait::async_trait;
use ralphx_lib::application::builder_attachment_materializer::materialize_builder_attachment;
use ralphx_lib::application::chat_service::{
    build_command, build_command_for_harness, build_command_with_app_data_dir,
    build_initial_prompt, build_launch_plan_for_harness_with_persona_for_test,
    build_resume_command, build_resume_command_for_harness, build_resume_initial_prompt,
    create_assistant_message, finalize_assistant_message_for_test,
    finalize_structured_assistant_message_for_test, format_attachments_for_agent,
    format_session_history, get_entity_status_for_resume, is_text_file,
    provider_resume_mode_for_session_under, resolve_conversation_spawn_context,
    resolve_mcp_filesystem_read_roots, resolve_working_directory, ProviderResumeMode,
    ResolvedChatHarnessLaunch,
};
use ralphx_lib::application::conversation_folder_reference_service::ConversationFolderReferenceService;
use ralphx_lib::application::persona_ingest::{
    persona_ingest_conversation_path, persona_ingest_storage_path,
};
use ralphx_lib::application::persona_resolver::{resolve_persona_for_send, PersonaResolveFlags};
use ralphx_lib::application::standalone_workspace::create_workspace;
use ralphx_lib::application::AppState;
use ralphx_lib::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use ralphx_lib::domain::entities::{self, *};
use ralphx_lib::domain::repositories::{self, *};
use ralphx_lib::error::AppResult;
use ralphx_lib::infrastructure::agents::claude::{ContentBlockItem, ToolCall};
use ralphx_lib::infrastructure::memory::*;
use ralphx_lib::testing::create_mock_app;
use std::fs;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Listener;
use tempfile::TempDir;

fn provider_state_home_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn claude_spawn_override_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[allow(clippy::await_holding_lock)]
async fn with_provider_state_home_override<T, Fut>(home: &Path, f: impl FnOnce() -> Fut) -> T
where
    Fut: Future<Output = T>,
{
    let _guard = provider_state_home_lock().lock().expect("lock poisoned");
    let _env_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_PROVIDER_STATE_HOME_OVERRIDE",
        home.as_os_str(),
    );
    f().await
}

#[allow(clippy::await_holding_lock)]
async fn with_claude_spawn_allowed_in_tests<T, Fut>(f: impl FnOnce() -> Fut) -> T
where
    Fut: Future<Output = T>,
{
    let _guard = claude_spawn_override_lock().lock().expect("lock poisoned");
    let _env_guard =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    f().await
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, contents).expect("write test file");
}

fn env_value(envs: &[(std::ffi::OsString, std::ffi::OsString)], key: &str) -> Option<String> {
    envs.iter()
        .find(|(env_key, _)| env_key == key)
        .map(|(_, value)| value.to_string_lossy().into_owned())
}

fn spawnable_prompt(
    spawnable: &ralphx_lib::infrastructure::agents::claude::SpawnableCommand,
) -> String {
    spawnable
        .get_stdin_prompt_for_test()
        .map(str::to_string)
        .unwrap_or_else(|| spawnable.get_args_for_test().join("\n"))
}

fn final_spawnable_command(
    spawnable: &ralphx_lib::infrastructure::agents::claude::SpawnableCommand,
) -> String {
    let args = spawnable.get_args_for_test();
    let mut rendered = args.join("\n");
    for window in args.windows(2) {
        if window[0] == "--append-system-prompt-file" {
            rendered.push('\n');
            rendered
                .push_str(&fs::read_to_string(&window[1]).expect("read appended system prompt"));
        }
    }
    rendered
}

fn mcp_runtime_args(
    spawnable: &ralphx_lib::infrastructure::agents::claude::SpawnableCommand,
) -> Vec<String> {
    let command_args = spawnable.get_args_for_test();
    if let Some(config_path) = command_args
        .iter()
        .position(|arg| arg == "--mcp-config")
        .and_then(|index| command_args.get(index + 1))
    {
        let config = if config_path.trim_start().starts_with('{') {
            config_path.clone()
        } else {
            fs::read_to_string(config_path).unwrap_or_default()
        };
        let json: serde_json::Value = serde_json::from_str(&config).expect("valid MCP config");
        return json
            .get("mcpServers")
            .and_then(|servers| servers.as_object())
            .into_iter()
            .flat_map(|servers| servers.values())
            .filter_map(|server| server.get("args").and_then(|args| args.as_array()))
            .flatten()
            .filter_map(|arg| arg.as_str().map(str::to_string))
            .collect();
    }

    command_args
        .iter()
        .filter_map(|arg| arg.split_once(".args="))
        .filter_map(|(_, encoded)| serde_json::from_str::<Vec<String>>(encoded).ok())
        .find(|args| args.iter().any(|arg| arg == "--conversation-id"))
        .unwrap_or_default()
}

async fn persona_read_root_fixture() -> (
    TempDir,
    Arc<MemoryProjectRepository>,
    ProjectId,
    PathBuf,
    PathBuf,
) {
    let root = tempfile::tempdir_in(std::env::current_dir().expect("current workspace"))
        .expect("persona read-root fixture");
    let project_directory = root.path().join("project");
    let working_directory = root.path().join("agent-workspace");
    fs::create_dir_all(&project_directory).expect("create project directory");
    fs::create_dir_all(&working_directory).expect("create working directory");

    let project_repo = Arc::new(MemoryProjectRepository::new());
    let project_id = ProjectId::from_string("persona-read-root-project".to_string());
    let mut project = Project::new(
        "Persona read roots".to_string(),
        project_directory.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project_repo
        .create(project)
        .await
        .expect("seed project read root");

    (
        root,
        project_repo,
        project_id,
        project_directory,
        working_directory,
    )
}

#[tokio::test]
async fn persona_builder_read_roots_resolve_to_ingest_store_only() {
    let (root, project_repo, project_id, project_directory, working_directory) =
        persona_read_root_fixture().await;
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let app_data_dir = root.path().join("app-data");
    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &conversation.id.as_str(),
    );
    write_file(&ingest_root.join("content"), "approved ingest text");
    let workspace = create_workspace(&app_data_dir, &conversation.id.as_str())
        .expect("legacy builder workspace");

    let roots = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        &working_directory,
        conversation.agent_mode,
        Some(&conversation.id.as_str()),
        Some(app_data_dir.as_path()),
    )
    .await;

    assert_eq!(roots, vec![ingest_root.clone(), workspace.clone()]);
    assert!(
        !roots.contains(&project_directory),
        "PersonaBuilder must not expose the project working directory"
    );

    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            Path::new("/fake/claude"),
            &repo_plugin_dir(),
            &conversation,
            "read ingest context",
            None,
            &working_directory,
            None,
            Some(project_id.as_str()),
            &roots,
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await
    .expect("PersonaBuilder command should build");

    // Read roots travel as --filesystem-read-root args inside the generated
    // MCP config (mcp_runtime_context), not as a command env var. The config
    // may be inline or a temp-file path, so resolve the JSON either way.
    let command_args = {
        let raw = final_spawnable_command(&command);
        let args = command.get_args_for_test();
        let mcp_config = args
            .iter()
            .position(|arg| arg == "--mcp-config")
            .and_then(|i| args.get(i + 1))
            .map(|value| {
                if value.trim_start().starts_with('{') {
                    value.clone()
                } else {
                    std::fs::read_to_string(value).unwrap_or_default()
                }
            })
            .unwrap_or_default();
        format!("{raw}\n{mcp_config}")
    };
    assert!(
        command_args.contains("--filesystem-read-root"),
        "MCP config must carry a filesystem read root"
    );
    assert!(
        command_args.contains(ingest_root.to_string_lossy().as_ref()),
        "MCP read roots must include the ingest store"
    );
    assert!(
        !command_args.contains(project_directory.to_string_lossy().as_ref()),
        "MCP read roots must exclude the project working directory"
    );
    assert!(
        command_args.contains("--filesystem-enforced") && command_args.contains("\"1\""),
        "fresh PersonaBuilder MCP config must enable filesystem enforcement: {command_args}"
    );
    assert!(
        mcp_runtime_args(&command)
            .windows(2)
            .any(|pair| pair == ["--conversation-id".to_string(), conversation.id.as_str()]),
        "fresh PersonaBuilder MCP config must carry the conversation row id: {command_args}"
    );
    assert!(
        !command_args.contains("RALPHX_FILESYSTEM_ENFORCED")
            && env_value(&command.get_envs_for_test(), "RALPHX_FILESYSTEM_ENFORCED").is_none(),
        "filesystem enforcement must not use process env"
    );
}

#[tokio::test]
async fn persona_builder_attachment_uses_real_app_data_when_project_path_contains_workspace_name() {
    let root = tempfile::tempdir_in(std::env::current_dir().expect("current workspace"))
        .expect("builder attachment fixture");
    let app_data_dir = root.path().join("actual-app-data");
    let attachment_storage = root.path().join("attachment-storage");
    let project_directory = root.path().join("standalone_workspaces").join("project");
    fs::create_dir_all(&app_data_dir).expect("create app data");
    fs::create_dir_all(&attachment_storage).expect("create attachment storage");
    fs::create_dir_all(&project_directory).expect("create project path collision");

    let project_id = ProjectId::from_string("workspace-name-collision".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let source = attachment_storage.join("source.txt");
    write_file(&source, "builder context marker");
    let attachment = ChatAttachment::new(
        conversation.id,
        "source.txt",
        source.to_string_lossy(),
        22,
        Some("text/plain".to_string()),
    );
    materialize_builder_attachment(&app_data_dir, &attachment_storage, &attachment)
        .expect("materialize attachment under the real app data root");
    let attachment_repo = Arc::new(MemoryChatAttachmentRepository::new());
    attachment_repo
        .create(attachment)
        .await
        .expect("seed pending attachment");

    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_command_with_app_data_dir(
            Path::new("/fake/claude"),
            &repo_plugin_dir(),
            &conversation,
            "use attached builder context",
            None,
            &project_directory,
            None,
            Some(project_id.as_str()),
            std::slice::from_ref(&project_directory),
            Some(&app_data_dir),
            attachment_repo,
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await
    .expect("builder command must use the real app data root, not infer it from project names");

    let prompt = spawnable_prompt(&command);
    assert!(prompt.contains("<file_path>"));
    assert!(prompt.contains(app_data_dir.to_string_lossy().as_ref()));
    assert!(!prompt.contains("builder context marker"));
}

#[tokio::test]
async fn persona_builder_without_ingest_session_resolves_zero_roots() {
    let (root, project_repo, project_id, project_directory, working_directory) =
        persona_read_root_fixture().await;
    let app_data_dir = root.path().join("app-data");

    let roots = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        &working_directory,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
        Some("persona-builder-no-ingest"),
        Some(app_data_dir.as_path()),
    )
    .await;

    assert!(
        roots.is_empty(),
        "missing PersonaBuilder ingest destination must provide no MCP read roots"
    );

    let empty_ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        "persona-builder-no-ingest",
    );
    fs::create_dir_all(&empty_ingest_root).expect("create empty ingest destination");
    let workspace = create_workspace(&app_data_dir, "persona-builder-no-ingest")
        .expect("new-pipeline builder workspace");
    let roots = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        project_repo as Arc<dyn ProjectRepository>,
        &working_directory,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
        Some("persona-builder-no-ingest"),
        Some(app_data_dir.as_path()),
    )
    .await;

    assert_eq!(roots, vec![project_directory, workspace]);
}

#[tokio::test]
async fn resolved_spawn_context_keeps_builder_roots_and_folder_prompt_in_lockstep() {
    let (root, project_repo, project_id, project_directory, _working_directory) =
        persona_read_root_fixture().await;
    let app_data_dir = root.path().join("app-data");
    let folder_reference_app_data_dir = root.path().join("folder-reference-app-data");
    fs::create_dir_all(&folder_reference_app_data_dir).expect("create folder-reference app data");
    let folder = root.path().join("live-folder-ref");
    fs::create_dir_all(&folder).expect("create folder ref");
    let folder_repo = Arc::new(MemoryConversationFolderReferenceRepository::new());

    let project_builder = ChatConversation::new_project(project_id.clone());
    let effective_builder_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let project_workspace = create_workspace(&app_data_dir, &project_builder.id.as_str())
        .expect("project builder workspace");
    ConversationFolderReferenceService::new(
        Arc::clone(&folder_repo) as Arc<dyn ConversationFolderReferenceRepository>,
        folder_reference_app_data_dir.clone(),
        5,
    )
    .add(project_builder.id, &folder, "Live folder".to_string())
    .await
    .expect("add project builder folder");
    let project_context = resolve_conversation_spawn_context(
        &project_builder,
        effective_builder_mode,
        Some(project_id.as_str()),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        &project_directory,
        Some(&app_data_dir),
        Some(&folder_reference_app_data_dir),
        Some(Arc::clone(&folder_repo) as Arc<dyn ConversationFolderReferenceRepository>),
    )
    .await
    .expect("resolve Project builder context");
    assert_eq!(
        project_context.folder_roots,
        vec![project_directory.clone(), project_workspace, folder.clone()],
        "enforced Project builders must retain the project root even when it equals CWD"
    );
    let project_block = project_context
        .folder_refs_block
        .expect("reachable folder root must render a prompt hint");
    assert!(project_block.contains(folder.to_string_lossy().as_ref()));
    assert!(project_context.enforce_filesystem_roots);

    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &project_builder.id.as_str(),
    );
    write_file(&ingest_root.join("legacy.txt"), "legacy ingest context");
    let legacy_context = resolve_conversation_spawn_context(
        &project_builder,
        effective_builder_mode,
        Some(project_id.as_str()),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        &project_directory,
        Some(&app_data_dir),
        Some(&folder_reference_app_data_dir),
        Some(Arc::clone(&folder_repo) as Arc<dyn ConversationFolderReferenceRepository>),
    )
    .await
    .expect("resolve legacy ingest builder context");
    assert_eq!(
        legacy_context.folder_roots,
        vec![
            ingest_root,
            create_workspace(&app_data_dir, &project_builder.id.as_str())
                .expect("resolve project builder workspace"),
            folder.clone(),
        ]
    );
    assert!(legacy_context
        .folder_refs_block
        .expect("legacy folder hint")
        .contains(folder.to_string_lossy().as_ref()));

    let mut standalone_builder = ChatConversation::new_standalone();
    standalone_builder.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let standalone_workspace = create_workspace(&app_data_dir, &standalone_builder.id.as_str())
        .expect("standalone builder workspace");
    ConversationFolderReferenceService::new(
        Arc::clone(&folder_repo) as Arc<dyn ConversationFolderReferenceRepository>,
        folder_reference_app_data_dir.clone(),
        5,
    )
    .add(standalone_builder.id, &folder, "Live folder".to_string())
    .await
    .expect("add standalone builder folder");
    let standalone_context = resolve_conversation_spawn_context(
        &standalone_builder,
        standalone_builder.agent_mode,
        None,
        Arc::new(MemoryProjectRepository::new()),
        &standalone_workspace,
        Some(&app_data_dir),
        Some(&folder_reference_app_data_dir),
        Some(Arc::clone(&folder_repo) as Arc<dyn ConversationFolderReferenceRepository>),
    )
    .await
    .expect("resolve Standalone builder context");
    assert_eq!(
        standalone_context.folder_roots,
        vec![standalone_workspace, folder.clone()]
    );
    assert!(standalone_context
        .folder_refs_block
        .expect("standalone folder hint")
        .contains(folder.to_string_lossy().as_ref()));

    let mut workspace_less = ChatConversation::new_project(project_id.clone());
    workspace_less.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    ConversationFolderReferenceService::new(
        Arc::clone(&folder_repo) as Arc<dyn ConversationFolderReferenceRepository>,
        folder_reference_app_data_dir.clone(),
        5,
    )
    .add(workspace_less.id, &folder, "Unreachable folder".to_string())
    .await
    .expect("store legacy folder ref");
    let workspace_less_context = resolve_conversation_spawn_context(
        &workspace_less,
        workspace_less.agent_mode,
        Some(project_id.as_str()),
        project_repo as Arc<dyn ProjectRepository>,
        &project_directory,
        Some(&app_data_dir),
        Some(&folder_reference_app_data_dir),
        Some(folder_repo as Arc<dyn ConversationFolderReferenceRepository>),
    )
    .await
    .expect("resolve workspace-less legacy builder");
    assert!(workspace_less_context.folder_roots.is_empty());
    assert!(
        workspace_less_context.folder_refs_block.is_none(),
        "a folder hint must never render when its enforced root is absent"
    );
}

#[tokio::test]
async fn persona_builder_read_roots_fail_closed_without_owned_identity() {
    let (root, project_repo, project_id, project_directory, working_directory) =
        persona_read_root_fixture().await;
    let app_data_dir = root.path().join("app-data");

    let without_app_data = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        &working_directory,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
        Some("persona-builder-no-app-data"),
        None,
    )
    .await;
    let without_conversation = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        &working_directory,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
        None,
        Some(app_data_dir.as_path()),
    )
    .await;
    let with_relative_app_data = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        project_repo as Arc<dyn ProjectRepository>,
        &working_directory,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
        Some("persona-builder-relative-app-data"),
        Some(Path::new("relative-app-data")),
    )
    .await;

    for roots in [
        without_app_data,
        without_conversation,
        with_relative_app_data,
    ] {
        assert!(
            roots.is_empty(),
            "PersonaBuilder must fail closed without a safe ingest root"
        );
        assert!(
            !roots.contains(&project_directory),
            "PersonaBuilder must never fall back to the project directory"
        );
    }
}

#[tokio::test]
async fn non_persona_modes_keep_project_read_root_behavior() {
    let (root, project_repo, project_id, project_directory, working_directory) =
        persona_read_root_fixture().await;

    let roots = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        project_repo as Arc<dyn ProjectRepository>,
        &working_directory,
        Some(AgentConversationWorkspaceMode::Chat),
        Some("non-persona-conversation"),
        Some(root.path().join("app-data").as_path()),
    )
    .await;

    assert_eq!(roots, vec![project_directory]);
}

#[tokio::test]
async fn non_persona_modes_preserve_unenforced_mcp_spawn_shape() {
    let temp = tempfile::tempdir().expect("working directory");
    let project_id = ProjectId::from_string("unenforced-mode-project".to_string());
    let modes = [
        AgentConversationWorkspaceMode::Chat,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr,
    ];

    for mode in modes {
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.agent_mode = Some(mode);
        let command = with_claude_spawn_allowed_in_tests(|| async {
            build_command(
                Path::new("/fake/claude"),
                &repo_plugin_dir(),
                &conversation,
                "preserve the unenforced config",
                None,
                temp.path(),
                None,
                Some(project_id.as_str()),
                &[],
                Arc::new(MemoryChatAttachmentRepository::new()),
                Arc::new(MemoryArtifactRepository::new()),
                None,
                None,
                None,
                &[],
                0,
                None,
                None,
                None,
                None,
            )
            .await
        })
        .await
        .expect("non-builder command should build");
        let args = mcp_runtime_args(&command);
        assert!(
            !args.iter().any(|arg| arg == "--filesystem-enforced"),
            "{mode} must preserve the pre-change unenforced MCP args: {args:?}"
        );
        assert!(
            env_value(&command.get_envs_for_test(), "RALPHX_FILESYSTEM_ENFORCED").is_none(),
            "{mode} must not receive enforcement through process env"
        );
    }
}

#[tokio::test]
async fn queued_flush_uses_persona_builder_read_roots() {
    let (root, project_repo, project_id, project_directory, working_directory) =
        persona_read_root_fixture().await;
    let conversation_id = ChatConversationId::from_string("queued-persona-builder".to_string());
    let app_data_dir = root.path().join("app-data");
    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &conversation_id.as_str(),
    );
    write_file(&ingest_root.join("content"), "queued ingest text");

    let roots = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        project_repo as Arc<dyn ProjectRepository>,
        &working_directory,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
        Some(&conversation_id.as_str()),
        Some(app_data_dir.as_path()),
    )
    .await;
    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_resume_command(
            Path::new("/fake/claude"),
            &repo_plugin_dir(),
            ChatContextType::Project,
            project_id.as_str(),
            CoordinationMode::Solo,
            &conversation_id.as_str(),
            Some(AgentConversationWorkspaceMode::PersonaBuilder),
            None,
            "queued ingest follow-up",
            Some("ralphx-persona-extractor"),
            None,
            None,
            &working_directory,
            "queued-persona-session",
            Some(project_id.as_str()),
            &roots,
            Some(conversation_id.as_str().to_string()),
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            Arc::new(MemoryIdeationSessionRepository::new()),
            empty_delegated_session_repo(),
            Arc::new(MemoryTaskRepository::new()),
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await
    .expect("queued PersonaBuilder resume command should build");

    // Same transport as the fresh path: --filesystem-read-root args in the
    // generated MCP config. The resume builder may pass the config inline or
    // as a temp-file path, so resolve the JSON either way.
    let command_args = {
        let raw = final_spawnable_command(&command);
        let args = command.get_args_for_test();
        let mcp_config = args
            .iter()
            .position(|arg| arg == "--mcp-config")
            .and_then(|i| args.get(i + 1))
            .map(|value| {
                if value.trim_start().starts_with('{') {
                    value.clone()
                } else {
                    std::fs::read_to_string(value).unwrap_or_default()
                }
            })
            .unwrap_or_default();
        format!("{raw}\n{mcp_config}")
    };
    assert!(
        command_args.contains("--filesystem-read-root"),
        "queued MCP config must carry a filesystem read root"
    );
    assert!(
        command_args.contains(ingest_root.to_string_lossy().as_ref()),
        "queued MCP read roots must include the ingest store"
    );
    assert!(
        !command_args.contains(project_directory.to_string_lossy().as_ref()),
        "queued MCP read roots must exclude the project working directory"
    );
    assert!(
        command_args.contains("--filesystem-enforced") && command_args.contains("\"1\""),
        "queued PersonaBuilder MCP config must enable filesystem enforcement: {command_args}"
    );
    assert!(
        mcp_runtime_args(&command)
            .windows(2)
            .any(|pair| pair == ["--conversation-id".to_string(), conversation_id.as_str()])
            && !mcp_runtime_args(&command)
                .windows(2)
                .any(|pair| pair == ["--conversation-id", project_id.as_str()]),
        "queued MCP identity must be the conversation row id, not the project id: {command_args}"
    );
    assert!(
        env_value(&command.get_envs_for_test(), "RALPHX_FILESYSTEM_ENFORCED").is_none(),
        "queued filesystem enforcement must not use process env"
    );
}

fn repo_plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .join("plugins/app")
}

/// The fresh-spawn build path resolves the chat harness CLI and requires the
/// path to exist on disk, so persona shape tests need a real stub file.
fn stub_claude_cli(dir: &Path) -> std::path::PathBuf {
    let cli_path = dir.join("claude");
    write_file(&cli_path, "#!/bin/sh\nexit 0\n");
    cli_path
}

async fn bound_project_persona() -> (
    ChatConversation,
    ralphx_lib::application::persona_prompt::ResolvedPersona,
) {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let now = chrono::Utc::now();
    let persona = Persona {
        id: PersonaId::from("persona-bound-project"),
        artifact_id: None,

        project_id: None,
        slug: "bound-project".to_string(),
        name: "Bound Project".to_string(),
        description: "test persona".to_string(),
        content: "Use the bound project voice.".to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: "bound-project-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    repo.create(persona.clone())
        .await
        .expect("seed active persona");

    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("persona-project".to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        PersonaResolveFlags {
            feature_enabled: true,
            is_external_mcp: false,
            agent_name_override_set: false,
            agent_conversation_mode: None,
            is_verification: false,
        },
        repo,
    )
    .await
    .expect("bound persona resolution")
    .expect("bound persona is injected");
    (conversation, resolved)
}

async fn assert_suppressed_persona_has_no_final_command_block(
    context_type: ChatContextType,
    flags: PersonaResolveFlags,
) {
    let (mut conversation, _) = bound_project_persona().await;
    conversation.context_type = context_type;
    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        flags,
        Arc::new(MemoryPersonaRepository::new()),
    )
    .await
    .expect("excluded spawn family must suppress before a persona repository read");
    assert!(
        resolved.is_none(),
        "excluded spawn family must suppress personas"
    );

    let working_dir = tempfile::tempdir().expect("working directory");
    let stub_cli = stub_claude_cli(working_dir.path());
    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_command_for_harness(
            AgentHarnessKind::Claude,
            &stub_cli,
            &repo_plugin_dir(),
            &conversation,
            "excluded-family persona absence",
            resolved,
            working_dir.path(),
            None,
            Some(conversation.context_id.as_str()),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("excluded-family command should build");

    assert!(
        !final_spawnable_command(&command.spawnable).contains("<ralphx_agent_persona>"),
        "the final command for an excluded spawn family must not contain a persona block"
    );
}

#[tokio::test]
async fn fresh_spawn_prompt_includes_bound_persona_block() {
    let (conversation, persona) = bound_project_persona().await;
    let working_dir = tempfile::tempdir().expect("working directory");
    let stub_cli = stub_claude_cli(working_dir.path());
    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_command_for_harness(
            AgentHarnessKind::Claude,
            &stub_cli,
            &repo_plugin_dir(),
            &conversation,
            "fresh persona send",
            Some(persona),
            working_dir.path(),
            None,
            Some(conversation.context_id.as_str()),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("fresh command should build");

    assert!(
        final_spawnable_command(&command.spawnable).contains("<ralphx_agent_persona>"),
        "the final fresh-spawn command must include the bound persona block"
    );
}

#[tokio::test]
async fn codex_fresh_persona_builder_spawn_uses_conversation_identity_and_cli_enforcement() {
    let temp = tempfile::tempdir().expect("working directory");
    let cli_path = make_fake_codex_cli(&temp);
    let project_id = ProjectId::from_string("codex-builder-project".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);

    let command = build_command_for_harness(
        AgentHarnessKind::Codex,
        &cli_path,
        &repo_plugin_dir(),
        &conversation,
        "fresh builder send",
        None,
        temp.path(),
        None,
        Some(project_id.as_str()),
        &[],
        Arc::new(MemoryChatAttachmentRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        None,
        None,
        None,
        &[],
        0,
        None,
        None,
        false,
        None,
        None,
    )
    .await
    .expect("Codex PersonaBuilder command should build");

    let args = mcp_runtime_args(&command.spawnable);
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--filesystem-enforced", "1"]),
        "fresh Codex builder MCP args must enable filesystem enforcement: {args:?}"
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--conversation-id".to_string(), conversation.id.as_str()])
            && !args
                .windows(2)
                .any(|pair| pair == ["--conversation-id", project_id.as_str()]),
        "fresh Codex MCP identity must be the conversation row id: {args:?}"
    );
    assert!(
        env_value(
            &command.spawnable.get_envs_for_test(),
            "RALPHX_FILESYSTEM_ENFORCED"
        )
        .is_none(),
        "Codex filesystem enforcement must not use process env"
    );
}

#[tokio::test]
async fn legacy_refine_builder_without_ingest_spawns_deny_all_with_draft_tools() {
    let (root, project_repo, project_id, _project_directory, working_directory) =
        persona_read_root_fixture().await;
    let cli_path = make_fake_codex_cli(&root);
    let app_data_dir = root.path().join("app-data");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    conversation.builder_draft_id = Some("legacy-bound-refine-draft".to_string());
    let roots = resolve_mcp_filesystem_read_roots(
        ChatContextType::Project,
        Some(project_id.as_str()),
        project_repo as Arc<dyn ProjectRepository>,
        &working_directory,
        conversation.agent_mode,
        Some(&conversation.id.as_str()),
        Some(&app_data_dir),
    )
    .await;
    assert!(
        roots.is_empty(),
        "no ingest store must produce deny-all roots"
    );

    let agent_name =
        ralphx_lib::infrastructure::agents::claude::agent_names::AGENT_PERSONA_EXTRACTOR;
    let resolved_spawn_settings =
        ralphx_lib::application::agent_lane_resolution::resolve_agent_spawn_settings(
            agent_name,
            Some(project_id.as_str()),
            ChatContextType::Project,
            None,
            Some(AgentHarnessKind::Codex),
            None,
            None,
        )
        .await;
    let launch = build_launch_plan_for_harness_with_persona_for_test(
        AgentHarnessKind::Codex,
        &cli_path,
        &repo_plugin_dir(),
        &conversation,
        "refine this legacy persona",
        None,
        Some(agent_name),
        None,
        conversation.context_type,
        conversation.context_id.as_str(),
        Some(conversation.id.as_str()),
        None,
        &working_directory,
        None,
        Some(project_id.as_str()),
        &roots,
        Arc::new(MemoryChatAttachmentRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryDelegatedSessionRepository::new()),
        Arc::new(MemoryTaskRepository::new()),
        &[],
        0,
        false,
        None,
        &resolved_spawn_settings,
        None,
        None,
    )
    .await
    .expect("legacy refine builder should reach the production launch path");
    let command = match launch {
        ResolvedChatHarnessLaunch::Background { spawnable, .. } => spawnable,
        ResolvedChatHarnessLaunch::Interactive { .. } => {
            panic!("Codex legacy refine launch must remain background")
        }
    };

    let args = mcp_runtime_args(&command);
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--filesystem-enforced", "1"]),
        "legacy refine must keep filesystem enforcement enabled: {args:?}"
    );
    assert!(
        !args.iter().any(|arg| arg == "--filesystem-read-root"),
        "empty roots must remain deny-all: {args:?}"
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--conversation-id".to_string(), conversation.id.as_str()]),
        "MCP identity must remain conversation-owned: {args:?}"
    );
    let rendered = final_spawnable_command(&command);
    for tool in [
        "ask_user_question",
        "save_persona_draft",
        "get_persona_draft",
    ] {
        assert!(
            rendered.contains(tool),
            "legacy refine must retain interview/draft tool guidance for {tool}: {rendered}"
        );
    }
}

#[tokio::test]
async fn resume_command_prompt_includes_bound_persona_block() {
    let (conversation, persona) = bound_project_persona().await;
    let working_dir = tempfile::tempdir().expect("working directory");
    let stub_cli = stub_claude_cli(working_dir.path());
    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_resume_command_for_harness(
            AgentHarnessKind::Claude,
            &stub_cli,
            &repo_plugin_dir(),
            ChatContextType::Project,
            conversation.context_id.as_str(),
            CoordinationMode::Solo,
            &conversation.id.as_str(),
            conversation.agent_mode,
            None,
            "resume persona send",
            Some(persona),
            None,
            None,
            working_dir.path(),
            "persona-resume-session",
            Some(conversation.context_id.as_str()),
            &[],
            Some(conversation.id.to_string()),
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            Arc::new(MockIdeationRepo::empty()),
            empty_delegated_session_repo(),
            Arc::new(MockTaskRepo),
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("resume command should build");

    assert!(
        final_spawnable_command(&command.spawnable).contains("<ralphx_agent_persona>"),
        "the final resume command must include the bound persona block"
    );
}

#[tokio::test]
async fn recovery_command_prompt_includes_bound_persona_block() {
    let (conversation, persona) = bound_project_persona().await;
    let working_dir = tempfile::tempdir().expect("working directory");
    let stub_cli = stub_claude_cli(working_dir.path());
    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_command_for_harness(
            AgentHarnessKind::Claude,
            &stub_cli,
            &repo_plugin_dir(),
            &conversation,
            "recovery persona bootstrap",
            Some(persona),
            working_dir.path(),
            None,
            Some(conversation.context_id.as_str()),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("recovery command should build");

    assert!(
        final_spawnable_command(&command.spawnable).contains("<ralphx_agent_persona>"),
        "the final recovery command must include the bound persona block"
    );
}

#[tokio::test]
async fn automation_override_send_has_no_persona_block() {
    assert_suppressed_persona_has_no_final_command_block(
        ChatContextType::Project,
        PersonaResolveFlags {
            feature_enabled: true,
            is_external_mcp: false,
            agent_name_override_set: true,
            agent_conversation_mode: None,
            is_verification: false,
        },
    )
    .await;
}

#[tokio::test]
async fn automation_setup_conversation_send_has_no_persona_block() {
    assert_suppressed_persona_has_no_final_command_block(
        ChatContextType::Project,
        PersonaResolveFlags {
            feature_enabled: true,
            is_external_mcp: false,
            agent_name_override_set: false,
            agent_conversation_mode: Some(AgentConversationWorkspaceMode::Automation),
            is_verification: false,
        },
    )
    .await;
}

#[tokio::test]
async fn persona_builder_final_command_has_no_persona_block() {
    assert_suppressed_persona_has_no_final_command_block(
        ChatContextType::Project,
        PersonaResolveFlags {
            feature_enabled: true,
            is_external_mcp: false,
            agent_name_override_set: false,
            agent_conversation_mode: Some(AgentConversationWorkspaceMode::PersonaBuilder),
            is_verification: false,
        },
    )
    .await;
}

#[tokio::test]
async fn ideation_context_send_has_no_persona_block() {
    assert_suppressed_persona_has_no_final_command_block(
        ChatContextType::Ideation,
        PersonaResolveFlags {
            feature_enabled: true,
            is_external_mcp: false,
            agent_name_override_set: false,
            agent_conversation_mode: None,
            is_verification: false,
        },
    )
    .await;
}

#[tokio::test]
async fn task_chat_send_has_no_persona_block() {
    assert_suppressed_persona_has_no_final_command_block(
        ChatContextType::Task,
        PersonaResolveFlags {
            feature_enabled: true,
            is_external_mcp: false,
            agent_name_override_set: false,
            agent_conversation_mode: None,
            is_verification: false,
        },
    )
    .await;
}

#[tokio::test]
async fn merge_chat_send_has_no_persona_block() {
    assert_suppressed_persona_has_no_final_command_block(
        ChatContextType::Merge,
        PersonaResolveFlags {
            feature_enabled: true,
            is_external_mcp: false,
            agent_name_override_set: false,
            agent_conversation_mode: None,
            is_verification: false,
        },
    )
    .await;
}

fn empty_delegated_session_repo() -> Arc<dyn DelegatedSessionRepository> {
    Arc::new(MemoryDelegatedSessionRepository::new())
}

fn make_fake_codex_cli(temp: &TempDir) -> PathBuf {
    let script_path = temp.path().join("codex");
    let script = r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex-cli 0.116.0"
  exit 0
fi
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
exit 0
"#;

    write_file(&script_path, script);
    let mut permissions = fs::metadata(&script_path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("chmod script");
    script_path
}

fn codex_command_fixture() -> (TempDir, TempDir, PathBuf, PathBuf, PathBuf) {
    let home = tempfile::tempdir().expect("tempdir");
    let cli_temp = tempfile::tempdir().expect("tempdir");
    let cli_path = make_fake_codex_cli(&cli_temp);
    let plugin_dir = cli_temp.path().join("plugins").join("app");
    fs::create_dir_all(&plugin_dir).expect("create plugin dir");
    write_file(
        &plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp server",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-plan-verifier/agent.yaml"),
        "name: ralphx-plan-verifier\nrole: plan_verifier\n",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-plan-verifier/codex/agent.yaml"),
        "runtime_features:\n  shell_tool: false\n",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-chat-project/agent.yaml"),
        "name: ralphx-chat-project\nrole: project_chat\n",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-chat-project/codex/prompt.md"),
        "You are the RalphX project chat agent.",
    );
    let working_dir = cli_temp.path().to_path_buf();
    (home, cli_temp, cli_path, plugin_dir, working_dir)
}

#[tokio::test]
async fn codex_fresh_command_forwards_a_resolved_persona_block() {
    let (conversation, persona) = bound_project_persona().await;
    let (home, _cli_temp, cli_path, plugin_dir, working_dir) = codex_command_fixture();

    let command = with_provider_state_home_override(home.path(), || async {
        build_command_for_harness(
            AgentHarnessKind::Codex,
            &cli_path,
            &plugin_dir,
            &conversation,
            "fresh codex persona send",
            Some(persona),
            &working_dir,
            None,
            Some(conversation.context_id.as_str()),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("codex fresh command should build");

    assert!(
        spawnable_prompt(&command.spawnable).contains("<ralphx_agent_persona>"),
        "the Codex fresh prompt must retain the resolved persona block"
    );
    assert!(command.persona_injected());
    assert_eq!(command.persona_injection_skipped_reason(), None);
}

#[tokio::test]
async fn persona_codex_command_reports_reasoned_skip_when_agent_prompt_is_missing() {
    let (conversation, persona) = bound_project_persona().await;
    let home = tempfile::tempdir().expect("provider home");
    let cli_temp = tempfile::tempdir().expect("codex cli tempdir");
    let cli_path = make_fake_codex_cli(&cli_temp);
    let plugin_dir = cli_temp.path().join("plugins").join("app");
    fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build")).expect("create plugin dir");
    write_file(
        &plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp server",
    );

    let command = with_provider_state_home_override(home.path(), || async {
        build_command_for_harness(
            AgentHarnessKind::Codex,
            &cli_path,
            &plugin_dir,
            &conversation,
            "fresh codex persona fallback",
            Some(persona),
            cli_temp.path(),
            None,
            Some(conversation.context_id.as_str()),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("codex fallback command should build");

    assert!(!command.persona_injected());
    assert_eq!(
        command.persona_injection_skipped_reason(),
        Some("codex_agent_prompt_unavailable")
    );
    assert!(!spawnable_prompt(&command.spawnable).contains("<ralphx_agent_persona>"));
}

#[tokio::test]
async fn codex_launch_plans_preserve_personas_for_fresh_and_recovery_modes() {
    let (conversation, persona) = bound_project_persona().await;
    let (home, _cli_temp, cli_path, plugin_dir, working_directory) = codex_command_fixture();
    let spawn_settings =
        ralphx_lib::application::agent_lane_resolution::resolve_agent_spawn_settings(
            "ralphx-chat-project",
            Some(conversation.context_id.as_str()),
            ChatContextType::Project,
            None,
            Some(AgentHarnessKind::Codex),
            None,
            None,
        )
        .await;

    for (stored_session_id, expected_subcommand) in
        [(None, "exec"), (Some("missing-session"), "exec")]
    {
        let launch = with_provider_state_home_override(home.path(), || async {
            build_launch_plan_for_harness_with_persona_for_test(
                AgentHarnessKind::Codex,
                &cli_path,
                &plugin_dir,
                &conversation,
                "launch with the bound persona",
                Some(persona.clone()),
                None,
                None,
                ChatContextType::Project,
                conversation.context_id.as_str(),
                Some(conversation.id.to_string()),
                None,
                &working_directory,
                None,
                Some(conversation.context_id.as_str()),
                &[],
                Arc::new(MemoryChatAttachmentRepository::new()),
                Arc::new(MemoryArtifactRepository::new()),
                Arc::new(MockIdeationRepo::empty()),
                empty_delegated_session_repo(),
                Arc::new(MockTaskRepo),
                &[],
                0,
                false,
                stored_session_id,
                &spawn_settings,
                None,
                None,
            )
            .await
        })
        .await
        .expect("Codex launch plan should build");

        let spawnable = match launch {
            ResolvedChatHarnessLaunch::Background { spawnable, .. } => spawnable,
            ResolvedChatHarnessLaunch::Interactive { .. } => {
                panic!("Codex launch plans must remain background commands")
            }
        };
        let args = spawnable.get_args_for_test();
        assert_eq!(args.first().map(String::as_str), Some(expected_subcommand));
        assert!(
            !args.iter().any(|arg| arg == "resume"),
            "missing Codex provider state must use recovery exec: {args:?}"
        );
        assert!(
            spawnable_prompt(&spawnable).contains("<ralphx_agent_persona>"),
            "fresh and recovery launch prompts must retain the resolved persona"
        );
    }
}

fn make_codex_home_with_session(session_id: &str) -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let session_path = temp
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("04")
        .join("11")
        .join(format!("rollout-2026-04-11T03-49-25-{session_id}.jsonl"));
    write_file(&session_path, "{\"type\":\"thread.started\"}\n");
    temp
}

fn make_claude_home_with_session(session_id: &str) -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let transcript_path = temp
        .path()
        .join(".claude")
        .join("projects")
        .join("project-a")
        .join(format!("{session_id}.jsonl"));
    write_file(&transcript_path, "{\"type\":\"assistant\"}\n");
    temp
}

#[test]
fn test_is_text_file_by_mime_type() {
    // Text MIME types
    assert!(is_text_file(Some("text/plain"), "file.txt"));
    assert!(is_text_file(Some("text/html"), "file.html"));
    assert!(is_text_file(Some("application/json"), "file.json"));
    assert!(is_text_file(Some("application/xml"), "file.xml"));
    assert!(is_text_file(Some("application/javascript"), "file.js"));
    assert!(is_text_file(Some("application/typescript"), "file.ts"));

    // Binary MIME types
    assert!(!is_text_file(Some("image/png"), "file.png"));
    assert!(!is_text_file(Some("application/pdf"), "file.pdf"));
    assert!(!is_text_file(Some("video/mp4"), "file.mp4"));
}

#[test]
fn test_is_text_file_by_extension() {
    // Common text extensions (no MIME type provided)
    assert!(is_text_file(None, "file.txt"));
    assert!(is_text_file(None, "file.md"));
    assert!(is_text_file(None, "file.rs"));
    assert!(is_text_file(None, "file.ts"));
    assert!(is_text_file(None, "file.tsx"));
    assert!(is_text_file(None, "file.js"));
    assert!(is_text_file(None, "file.jsx"));
    assert!(is_text_file(None, "file.json"));
    assert!(is_text_file(None, "file.yaml"));
    assert!(is_text_file(None, "file.yml"));
    assert!(is_text_file(None, "file.xml"));
    assert!(is_text_file(None, "file.html"));
    assert!(is_text_file(None, "file.css"));
    assert!(is_text_file(None, "file.py"));
    assert!(is_text_file(None, "file.java"));
    assert!(is_text_file(None, "file.c"));
    assert!(is_text_file(None, "file.cpp"));
    assert!(is_text_file(None, "file.h"));
    assert!(is_text_file(None, "file.go"));
    assert!(is_text_file(None, "file.sh"));
    assert!(is_text_file(None, "file.toml"));
    assert!(is_text_file(None, "file.csv"));
    assert!(is_text_file(None, "file.log"));
    assert!(is_text_file(None, "file.sql"));
    assert!(is_text_file(None, "file.graphql"));

    // Binary extensions
    assert!(!is_text_file(None, "file.png"));
    assert!(!is_text_file(None, "file.jpg"));
    assert!(!is_text_file(None, "file.pdf"));
    assert!(!is_text_file(None, "file.mp4"));
    assert!(!is_text_file(None, "file.zip"));

    // Files without extensions
    assert!(!is_text_file(None, "README"));
    assert!(!is_text_file(None, "no-extension"));
}

#[test]
fn create_assistant_message_keeps_delegation_conversation_scope() {
    let conversation_id = ChatConversationId::new();

    let message = create_assistant_message(
        ChatContextType::Delegation,
        "delegated-session",
        "delegated reply",
        conversation_id,
        &[],
        &[],
    );

    assert_eq!(message.role, MessageRole::Orchestrator);
    assert_eq!(message.session_id, None);
    assert_eq!(message.project_id, None);
    assert_eq!(message.task_id, None);
    assert_eq!(message.conversation_id, Some(conversation_id));
}

#[tokio::test]
async fn finalize_assistant_message_emits_delegated_conversation_id() {
    let state = AppState::new_test();
    let app = create_mock_app();
    let handle = app.handle().clone();
    let conversation_id = ChatConversationId::new();
    let delegated_conversation_id = conversation_id.as_str();
    let orchestrator_role = MessageRole::Orchestrator.to_string();

    let message = create_assistant_message(
        ChatContextType::Delegation,
        "delegated-session",
        "queued delegated reply",
        conversation_id,
        &[],
        &[],
    );
    let message_id = message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(message)
        .await
        .expect("insert delegated assistant message");

    let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let captured_clone = Arc::clone(&captured);
    handle.listen("agent:message_created", move |event| {
        let payload: serde_json::Value =
            serde_json::from_str(event.payload()).expect("event payload JSON");
        *captured_clone.lock().expect("capture lock") = Some(payload);
    });

    finalize_assistant_message_for_test(
        &state.chat_message_repo,
        Some(&handle),
        &delegated_conversation_id,
        &ChatContextType::Delegation.to_string(),
        "delegated-session",
        &message_id,
        &orchestrator_role,
        "final delegated reply",
        None,
        None,
    )
    .await;

    let payload = captured
        .lock()
        .expect("capture lock")
        .clone()
        .expect("agent:message_created payload");
    assert_eq!(
        payload["conversation_id"].as_str(),
        Some(delegated_conversation_id.as_str()),
        "delegated finalize must emit the child conversation id"
    );
    assert_eq!(
        payload["context_type"].as_str(),
        Some(ChatContextType::Delegation.to_string().as_str())
    );
    assert_eq!(payload["context_id"].as_str(), Some("delegated-session"));
}

#[tokio::test]
async fn finalize_structured_assistant_message_splits_verification_transcript_segments() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();

    let message = create_assistant_message(
        ChatContextType::Ideation,
        context_id.as_str(),
        "",
        conversation_id,
        &[],
        &[],
    );
    let message_id = message.id.as_str().to_string();
    let role = message.role.to_string();
    state
        .chat_message_repo
        .create(message)
        .await
        .expect("insert verification assistant message");

    let tool_calls = vec![
        ToolCall {
            id: Some("tool-1".to_string()),
            name: "mcp__ralphx__fs_read_file".to_string(),
            arguments: serde_json::json!({ "path": "frontend/src/api/task-graph.ts" }),
            result: Some(
                serde_json::json!([{ "type": "text", "text": "FILE: /workspace/project/frontend/src/api/task-graph.ts" }]),
            ),
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
        ToolCall {
            id: Some("tool-2".to_string()),
            name: "mcp__ralphx__fs_grep".to_string(),
            arguments: serde_json::json!({ "pattern": "getTimelineEvents" }),
            result: Some(
                serde_json::json!([{ "type": "text", "text": "frontend/src/api/task-graph.ts:103:getTimelineEvents" }]),
            ),
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
        ToolCall {
            id: Some("tool-3".to_string()),
            name: "mcp__ralphx__run_verification_round".to_string(),
            arguments: serde_json::json!({ "round": 2 }),
            result: Some(serde_json::json!({ "status": "running" })),
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
    ];
    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "Round 1: needs_revision. Reading source to address gaps.".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("tool-1".to_string()),
            name: "mcp__ralphx__fs_read_file".to_string(),
            arguments: serde_json::json!({ "path": "frontend/src/api/task-graph.ts" }),
            result: Some(
                serde_json::json!([{ "type": "text", "text": "FILE: /workspace/project/frontend/src/api/task-graph.ts" }]),
            ),
            parent_tool_use_id: None,
            diff_context: None,
        },
        ContentBlockItem::ToolUse {
            id: Some("tool-2".to_string()),
            name: "mcp__ralphx__fs_grep".to_string(),
            arguments: serde_json::json!({ "pattern": "getTimelineEvents" }),
            result: Some(
                serde_json::json!([{ "type": "text", "text": "frontend/src/api/task-graph.ts:103:getTimelineEvents" }]),
            ),
            parent_tool_use_id: None,
            diff_context: None,
        },
        ContentBlockItem::Text {
            text: "Plan revised. Running round 2.".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("tool-3".to_string()),
            name: "mcp__ralphx__run_verification_round".to_string(),
            arguments: serde_json::json!({ "round": 2 }),
            result: Some(serde_json::json!({ "status": "running" })),
            parent_tool_use_id: None,
            diff_context: None,
        },
    ];

    finalize_structured_assistant_message_for_test::<tauri::Wry>(
        &state.chat_message_repo,
        None,
        ChatContextType::Ideation,
        context_id.as_str(),
        &conversation_id,
        &message_id,
        &role,
        "Round 1: needs_revision. Reading source to address gaps.Plan revised. Running round 2.",
        &tool_calls,
        &content_blocks,
        true,
    )
    .await;

    let messages = state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("load conversation messages");
    let assistant_messages: Vec<_> = messages
        .into_iter()
        .filter(|message| message.role == MessageRole::Orchestrator)
        .collect();

    assert_eq!(
        assistant_messages.len(),
        2,
        "verification transcript should split into step-level messages"
    );
    assert_eq!(assistant_messages[0].id.as_str(), message_id);
    assert_eq!(
        assistant_messages[0].content,
        "Round 1: needs_revision. Reading source to address gaps."
    );
    assert_eq!(
        assistant_messages[1].content,
        "Plan revised. Running round 2."
    );

    let first_tools: Vec<ToolCall> = serde_json::from_str(
        assistant_messages[0]
            .tool_calls
            .as_deref()
            .expect("first assistant tool calls"),
    )
    .expect("parse first tool_calls");
    let second_tools: Vec<ToolCall> = serde_json::from_str(
        assistant_messages[1]
            .tool_calls
            .as_deref()
            .expect("second assistant tool calls"),
    )
    .expect("parse second tool_calls");

    assert_eq!(first_tools.len(), 2);
    assert_eq!(second_tools.len(), 1);
    assert_eq!(second_tools[0].name, "mcp__ralphx__run_verification_round");
}

#[tokio::test]
async fn test_format_attachments_empty() {
    let attachments: Vec<ChatAttachment> = vec![];
    let result =
        format_attachments_for_agent(&attachments, ChatContextType::Project, None, None).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "");
}

#[tokio::test]
async fn test_format_attachments_binary_file() {
    let conversation_id = ChatConversationId::new();
    let attachment = ChatAttachment::new(
        conversation_id,
        "screenshot.png",
        "/path/to/screenshot.png",
        1024,
        Some("image/png".to_string()),
    );

    let result =
        format_attachments_for_agent(&[attachment], ChatContextType::Project, None, None).await;
    assert!(result.is_ok());

    let formatted = result.unwrap();
    assert!(formatted.contains("<attachments>"));
    assert!(formatted.contains("<filename>screenshot.png</filename>"));
    assert!(formatted.contains("<mime_type>image/png</mime_type>"));
    assert!(formatted.contains("<file_path>/path/to/screenshot.png</file_path>"));
    assert!(formatted.contains("Use the Read tool to access this file"));
    assert!(formatted.contains("</attachments>"));
}

#[tokio::test]
async fn test_format_attachments_text_file() {
    use std::fs;

    // Create a temporary text file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join("test_attachment.txt");
    let test_content = "Hello, this is a test file!";
    fs::write(&temp_file, test_content).expect("Failed to write test file");

    let conversation_id = ChatConversationId::new();
    let attachment = ChatAttachment::new(
        conversation_id,
        "test_attachment.txt",
        temp_file.to_str().unwrap(),
        test_content.len() as i64,
        Some("text/plain".to_string()),
    );

    let result =
        format_attachments_for_agent(&[attachment], ChatContextType::Project, None, None).await;
    assert!(result.is_ok());

    let formatted = result.unwrap();
    assert!(formatted.contains("<attachments>"));
    assert!(formatted.contains("<filename>test_attachment.txt</filename>"));
    assert!(formatted.contains("<mime_type>text/plain</mime_type>"));
    assert!(formatted.contains("<content>"));
    assert!(formatted.contains(test_content));
    assert!(formatted.contains("</content>"));
    assert!(formatted.contains("</attachments>"));

    // Cleanup
    fs::remove_file(temp_file).ok();
}

#[tokio::test]
async fn test_format_attachments_multiple_files() {
    use std::fs;

    // Create a temporary text file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join("test_multiple.txt");
    let test_content = "Test content";
    fs::write(&temp_file, test_content).expect("Failed to write test file");

    let conversation_id = ChatConversationId::new();
    let text_attachment = ChatAttachment::new(
        conversation_id,
        "test_multiple.txt",
        temp_file.to_str().unwrap(),
        test_content.len() as i64,
        Some("text/plain".to_string()),
    );

    let binary_attachment = ChatAttachment::new(
        conversation_id,
        "image.png",
        "/path/to/image.png",
        2048,
        Some("image/png".to_string()),
    );

    let result = format_attachments_for_agent(
        &[text_attachment, binary_attachment],
        ChatContextType::Project,
        None,
        None,
    )
    .await;
    assert!(result.is_ok());

    let formatted = result.unwrap();

    // Should contain both attachments
    assert!(formatted.contains("test_multiple.txt"));
    assert!(formatted.contains(test_content));
    assert!(formatted.contains("image.png"));
    assert!(formatted.contains("/path/to/image.png"));
    assert!(formatted.contains("Use the Read tool to access this file"));

    // Cleanup
    fs::remove_file(temp_file).ok();
}

#[tokio::test]
async fn test_format_attachments_file_read_error() {
    let conversation_id = ChatConversationId::new();
    let attachment = ChatAttachment::new(
        conversation_id,
        "nonexistent.txt",
        "/nonexistent/path/file.txt",
        0,
        Some("text/plain".to_string()),
    );

    let result =
        format_attachments_for_agent(&[attachment], ChatContextType::Project, None, None).await;
    assert!(result.is_ok());

    let formatted = result.unwrap();
    assert!(formatted.contains("<filename>nonexistent.txt</filename>"));
    assert!(formatted.contains("<error>Failed to read file:"));
}

// Tests for get_entity_status_for_resume

// Mock for testing
struct MockIdeationRepo {
    session: Option<IdeationSession>,
}

impl MockIdeationRepo {
    fn with_session(session: IdeationSession) -> Self {
        Self {
            session: Some(session),
        }
    }
    fn empty() -> Self {
        Self { session: None }
    }
}

#[async_trait]
impl IdeationSessionRepository for MockIdeationRepo {
    async fn create(&self, _session: IdeationSession) -> AppResult<IdeationSession> {
        unimplemented!()
    }
    async fn get_by_id(&self, _id: &IdeationSessionId) -> AppResult<Option<IdeationSession>> {
        Ok(self.session.clone())
    }
    async fn get_by_project(&self, _project_id: &ProjectId) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }
    async fn update_status(
        &self,
        _id: &IdeationSessionId,
        _status: IdeationSessionStatus,
    ) -> AppResult<()> {
        unimplemented!()
    }
    async fn update_title(
        &self,
        _id: &IdeationSessionId,
        _title: Option<String>,
        _title_source: &str,
    ) -> AppResult<()> {
        unimplemented!()
    }
    async fn delete(&self, _id: &IdeationSessionId) -> AppResult<()> {
        unimplemented!()
    }
    async fn get_active_by_project(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }
    async fn count_by_status(
        &self,
        _project_id: &ProjectId,
        _status: IdeationSessionStatus,
    ) -> AppResult<u32> {
        unimplemented!()
    }
    async fn update_plan_artifact_id(
        &self,
        _id: &IdeationSessionId,
        _plan_artifact_id: Option<String>,
    ) -> AppResult<()> {
        unimplemented!()
    }
    async fn get_by_plan_artifact_id(
        &self,
        _plan_artifact_id: &str,
    ) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }
    async fn get_by_inherited_plan_artifact_id(
        &self,
        _artifact_id: &str,
    ) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }
    async fn get_children(
        &self,
        _parent_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }
    async fn get_ancestor_chain(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }
    async fn set_parent(
        &self,
        _session_id: &IdeationSessionId,
        _parent_id: Option<&IdeationSessionId>,
    ) -> AppResult<()> {
        unimplemented!()
    }

    async fn update_verification_state(
        &self,
        _id: &IdeationSessionId,
        _status: VerificationStatus,
        _in_progress: bool,
    ) -> AppResult<()> {
        unimplemented!()
    }

    async fn reset_verification(&self, _id: &IdeationSessionId) -> AppResult<bool> {
        unimplemented!()
    }

    async fn reset_and_begin_reverify(
        &self,
        _session_id: &str,
    ) -> AppResult<(i32, entities::VerificationRunSnapshot)> {
        unimplemented!()
    }

    async fn get_verification_status(
        &self,
        _id: &IdeationSessionId,
    ) -> AppResult<Option<(VerificationStatus, bool)>> {
        unimplemented!()
    }

    async fn save_verification_run_snapshot(
        &self,
        _id: &IdeationSessionId,
        _snapshot: &entities::VerificationRunSnapshot,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_verification_run_snapshot(
        &self,
        _id: &IdeationSessionId,
        _generation: i32,
    ) -> AppResult<Option<entities::VerificationRunSnapshot>> {
        Ok(None)
    }

    async fn revert_plan_and_skip_verification(
        &self,
        _id: &IdeationSessionId,
        _new_plan_artifact_id: String,
        _convergence_reason: String,
    ) -> AppResult<()> {
        unimplemented!()
    }

    async fn revert_plan_and_skip_with_artifact(
        &self,
        _session_id: &IdeationSessionId,
        _new_artifact_id: String,
        _artifact_type_str: String,
        _artifact_name: String,
        _content_text: String,
        _version: u32,
        _previous_version_id: String,
        _convergence_reason: String,
    ) -> AppResult<()> {
        unimplemented!()
    }

    async fn increment_verification_generation(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_stale_in_progress_sessions(
        &self,
        _stale_before: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }

    async fn get_all_in_progress_sessions(&self) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }

    async fn get_verification_children(
        &self,
        _parent_session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }

    async fn get_by_project_and_status(
        &self,
        _project_id: &str,
        _status: &str,
        _limit: u32,
    ) -> AppResult<Vec<IdeationSession>> {
        unimplemented!()
    }

    async fn get_group_counts(
        &self,
        _project_id: &ProjectId,
        _search: Option<&str>,
    ) -> AppResult<repositories::ideation_session_repository::SessionGroupCounts> {
        unimplemented!()
    }

    async fn list_by_group(
        &self,
        _project_id: &ProjectId,
        _group: &str,
        _offset: u32,
        _limit: u32,
        _search: Option<&str>,
    ) -> AppResult<(
        Vec<repositories::ideation_session_repository::IdeationSessionWithProgress>,
        u32,
    )> {
        unimplemented!()
    }

    fn set_expected_proposal_count_sync(
        _conn: &rusqlite::Connection,
        _session_id: &str,
        _count: u32,
    ) -> AppResult<()>
    where
        Self: Sized,
    {
        unimplemented!()
    }

    async fn set_auto_accept_status(
        &self,
        _session_id: &str,
        _status: &str,
        _auto_accept_started_at: Option<String>,
    ) -> AppResult<()> {
        unimplemented!()
    }

    fn count_active_by_session_sync(
        _conn: &rusqlite::Connection,
        _session_id: &str,
    ) -> AppResult<i64>
    where
        Self: Sized,
    {
        unimplemented!()
    }

    async fn get_by_idempotency_key(
        &self,
        _api_key_id: &str,
        _idempotency_key: &str,
    ) -> AppResult<Option<IdeationSession>> {
        Ok(None)
    }

    async fn update_external_activity_phase(
        &self,
        _id: &IdeationSessionId,
        _phase: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_external_last_read_message_id(
        &self,
        _id: &IdeationSessionId,
        _message_id: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn list_active_external_by_project(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn list_active_external_sessions_for_archival(
        &self,
        _stale_before: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn list_stalled_external_sessions(
        &self,
        _stalled_before: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn set_dependencies_acknowledged(&self, _session_id: &str) -> AppResult<()> {
        unimplemented!()
    }

    async fn reset_acceptance_cycle_fields(&self, _session_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn touch_updated_at(&self, _session_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn update_last_effective_model(&self, _session_id: &str, _model: &str) -> AppResult<()> {
        Ok(())
    }

    async fn list_active_verification_children(
        &self,
    ) -> AppResult<Vec<ralphx_lib::domain::entities::IdeationSession>> {
        Ok(vec![])
    }

    async fn set_pending_initial_prompt(
        &self,
        _session_id: &str,
        _prompt: Option<String>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn set_pending_initial_prompt_if_unset(
        &self,
        _session_id: &str,
        _prompt: String,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn claim_pending_session_for_project(
        &self,
        _project_id: &str,
    ) -> AppResult<Option<(String, String)>> {
        Ok(None)
    }

    async fn list_projects_with_pending_sessions(&self) -> AppResult<Vec<String>> {
        Ok(vec![])
    }

    async fn count_pending_sessions_for_project(
        &self,
        _project_id: &ralphx_lib::domain::entities::ProjectId,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn update_acceptance_status(
        &self,
        _session_id: &ralphx_lib::domain::entities::IdeationSessionId,
        _expected_current: Option<ralphx_lib::domain::entities::AcceptanceStatus>,
        _new_status: Option<ralphx_lib::domain::entities::AcceptanceStatus>,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn get_sessions_with_pending_acceptance(
        &self,
        _project_id: &ralphx_lib::domain::entities::ProjectId,
    ) -> AppResult<Vec<ralphx_lib::domain::entities::IdeationSession>> {
        Ok(vec![])
    }

    async fn set_verification_confirmation_status(
        &self,
        _session_id: &ralphx_lib::domain::entities::IdeationSessionId,
        _status: Option<ralphx_lib::domain::entities::VerificationConfirmationStatus>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_pending_verification_confirmations(
        &self,
        _project_id: &ralphx_lib::domain::entities::ProjectId,
    ) -> AppResult<Vec<ralphx_lib::domain::entities::IdeationSession>> {
        Ok(vec![])
    }

    async fn count_active_proposals(&self, _session_id: &IdeationSessionId) -> AppResult<usize> {
        Ok(0)
    }

    async fn get_latest_verification_child(
        &self,
        _parent_id: &IdeationSessionId,
    ) -> AppResult<Option<IdeationSession>> {
        Ok(None)
    }
}

struct MockTaskRepo;

#[async_trait]
impl TaskRepository for MockTaskRepo {
    async fn create(&self, task: entities::Task) -> AppResult<entities::Task> {
        Ok(task)
    }

    async fn get_by_id(&self, _id: &TaskId) -> AppResult<Option<entities::Task>> {
        Ok(None)
    }

    async fn get_by_project(&self, _project_id: &ProjectId) -> AppResult<Vec<entities::Task>> {
        Ok(vec![])
    }

    async fn update(&self, _task: &entities::Task) -> AppResult<()> {
        Ok(())
    }

    async fn update_with_expected_status(
        &self,
        _task: &entities::Task,
        _expected_status: entities::InternalStatus,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn update_metadata(&self, _id: &TaskId, _metadata: Option<String>) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &TaskId) -> AppResult<()> {
        Ok(())
    }

    async fn get_by_status(
        &self,
        _project_id: &ProjectId,
        _status: entities::InternalStatus,
    ) -> AppResult<Vec<entities::Task>> {
        Ok(vec![])
    }

    async fn persist_status_change(
        &self,
        _id: &TaskId,
        _from: entities::InternalStatus,
        _to: entities::InternalStatus,
        _trigger: &str,
    ) -> AppResult<String> {
        Ok(uuid::Uuid::new_v4().to_string())
    }

    async fn get_status_history(&self, _id: &TaskId) -> AppResult<Vec<StatusTransition>> {
        Ok(vec![])
    }

    async fn get_status_entered_at(
        &self,
        _task_id: &TaskId,
        _status: entities::InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }

    async fn get_status_last_entered_at(
        &self,
        _task_id: &TaskId,
        _status: entities::InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }

    async fn get_next_executable(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Option<entities::Task>> {
        Ok(None)
    }

    async fn get_by_ideation_session(
        &self,
        _session_id: &entities::IdeationSessionId,
    ) -> AppResult<Vec<entities::Task>> {
        Ok(vec![])
    }

    async fn get_by_project_filtered(
        &self,
        _project_id: &ProjectId,
        _include_archived: bool,
    ) -> AppResult<Vec<entities::Task>> {
        Ok(vec![])
    }

    async fn archive(&self, _task_id: &TaskId) -> AppResult<entities::Task> {
        unimplemented!()
    }

    async fn restore(&self, _task_id: &TaskId) -> AppResult<entities::Task> {
        unimplemented!()
    }

    async fn get_archived_count(
        &self,
        _project_id: &ProjectId,
        _ideation_session_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn list_paginated(
        &self,
        _project_id: &ProjectId,
        _statuses: Option<Vec<entities::InternalStatus>>,
        _offset: u32,
        _limit: u32,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
        _categories: Option<&[String]>,
    ) -> AppResult<Vec<entities::Task>> {
        Ok(vec![])
    }

    async fn count_tasks(
        &self,
        _project_id: &ProjectId,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn search(
        &self,
        _project_id: &ProjectId,
        _query: &str,
        _include_archived: bool,
    ) -> AppResult<Vec<entities::Task>> {
        Ok(vec![])
    }

    async fn get_oldest_ready_task(&self) -> AppResult<Option<entities::Task>> {
        Ok(None)
    }

    async fn get_oldest_ready_tasks(&self, _limit: u32) -> AppResult<Vec<entities::Task>> {
        Ok(vec![])
    }

    async fn get_stale_ready_tasks(&self, _threshold_secs: u64) -> AppResult<Vec<entities::Task>> {
        Ok(vec![])
    }

    async fn update_latest_state_history_metadata(
        &self,
        _task_id: &TaskId,
        _metadata: &StateHistoryMetadata,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn has_task_in_states(
        &self,
        _project_id: &ProjectId,
        _statuses: &[entities::InternalStatus],
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn get_status_history_batch(
        &self,
        _task_ids: &[entities::TaskId],
    ) -> AppResult<std::collections::HashMap<entities::TaskId, Vec<repositories::StatusTransition>>>
    {
        Ok(std::collections::HashMap::new())
    }
}

#[tokio::test]
async fn test_get_entity_status_for_resume_ideation_accepted() {
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    let mut session = IdeationSession::new(project_id.clone());
    session.id = session_id.clone();
    session.status = IdeationSessionStatus::Accepted;

    let ideation_repo = Arc::new(MockIdeationRepo::with_session(session));
    let task_repo = Arc::new(MockTaskRepo);

    let status = get_entity_status_for_resume(
        ChatContextType::Ideation,
        session_id.as_str(),
        ideation_repo,
        empty_delegated_session_repo(),
        task_repo,
    )
    .await;

    assert_eq!(status, Some("accepted".to_string()));
}

#[tokio::test]
async fn test_get_entity_status_for_resume_ideation_active() {
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    let mut session = IdeationSession::new(project_id.clone());
    session.id = session_id.clone();
    session.status = IdeationSessionStatus::Active;

    let ideation_repo = Arc::new(MockIdeationRepo::with_session(session));
    let task_repo = Arc::new(MockTaskRepo);

    let status = get_entity_status_for_resume(
        ChatContextType::Ideation,
        session_id.as_str(),
        ideation_repo,
        empty_delegated_session_repo(),
        task_repo,
    )
    .await;

    assert_eq!(status, Some("active".to_string()));
}

#[tokio::test]
async fn test_get_entity_status_for_resume_ideation_not_found() {
    let session_id = IdeationSessionId::new();

    let ideation_repo = Arc::new(MockIdeationRepo::empty());
    let task_repo = Arc::new(MockTaskRepo);

    let status = get_entity_status_for_resume(
        ChatContextType::Ideation,
        session_id.as_str(),
        ideation_repo,
        empty_delegated_session_repo(),
        task_repo,
    )
    .await;

    assert_eq!(status, None);
}

#[tokio::test]
async fn test_get_entity_status_for_resume_project_context() {
    let ideation_repo = Arc::new(MockIdeationRepo::empty());
    let task_repo = Arc::new(MockTaskRepo);

    let status = get_entity_status_for_resume(
        ChatContextType::Project,
        "project-id",
        ideation_repo,
        empty_delegated_session_repo(),
        task_repo,
    )
    .await;

    // Project context doesn't have status-based agent resolution
    assert_eq!(status, None);
}

// Tests for build_resume_initial_prompt

#[test]
fn test_build_resume_initial_prompt_ideation_includes_context_id_no_recovery_note() {
    let context_id = "test-session-123";
    let user_message = "hello";
    let result = build_resume_initial_prompt(
        ChatContextType::Ideation,
        context_id,
        user_message,
        &[],
        0,
        None,
    );
    assert!(result.contains(&format!("<context_id>{}</context_id>", context_id)));
    assert!(!result.contains("<recovery_note>"));
    assert!(!result.contains("get_session_messages"));
    assert!(!result.contains("<session_history"));
    assert!(result.contains(&format!("<user_message>{}</user_message>", user_message)));
}

#[test]
fn test_build_resume_initial_prompt_task_includes_context_id_no_recovery_note() {
    let context_id = "task-abc";
    let user_message = "hello";
    let result = build_resume_initial_prompt(
        ChatContextType::Task,
        context_id,
        user_message,
        &[],
        0,
        None,
    );
    assert!(result.contains(&format!("<task_id>{}</task_id>", context_id)));
    assert!(!result.contains("<recovery_note>"));
    assert!(result.contains(&format!("<user_message>{}</user_message>", user_message)));
}

#[test]
fn test_build_resume_initial_prompt_project_includes_context_id_no_recovery_note() {
    let context_id = "project-xyz";
    let user_message = "hello";
    let result = build_resume_initial_prompt(
        ChatContextType::Project,
        context_id,
        user_message,
        &[],
        0,
        None,
    );
    assert!(result.contains(&format!("<project_id>{}</project_id>", context_id)));
    assert!(!result.contains("<recovery_note>"));
}

#[test]
fn test_build_resume_initial_prompt_task_execution_delegates_to_initial_prompt() {
    let context_id = "task-exec-123";
    let user_message = "execute";
    let resume = build_resume_initial_prompt(
        ChatContextType::TaskExecution,
        context_id,
        user_message,
        &[],
        0,
        None,
    );
    let initial = build_initial_prompt(
        ChatContextType::TaskExecution,
        context_id,
        user_message,
        &[],
        0,
    );
    assert_eq!(resume, initial);
}

#[test]
fn provider_resume_mode_for_codex_requires_local_session_artifact() {
    let missing_home = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        provider_resume_mode_for_session_under(
            AgentHarnessKind::Codex,
            "019d7821-a3c9-7a92-ac91-25d19653181c",
            missing_home.path()
        ),
        ProviderResumeMode::Recovery
    );

    let existing_home = make_codex_home_with_session("019d7821-a3c9-7a92-ac91-25d19653181c");
    assert_eq!(
        provider_resume_mode_for_session_under(
            AgentHarnessKind::Codex,
            "019d7821-a3c9-7a92-ac91-25d19653181c",
            existing_home.path()
        ),
        ProviderResumeMode::Resume
    );
}

#[test]
fn provider_resume_mode_for_claude_requires_local_transcript() {
    let missing_home = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        provider_resume_mode_for_session_under(
            AgentHarnessKind::Claude,
            "00000000-0000-4000-8000-000000000000",
            missing_home.path()
        ),
        ProviderResumeMode::Recovery
    );

    let existing_home = make_claude_home_with_session("00000000-0000-4000-8000-000000000000");
    assert_eq!(
        provider_resume_mode_for_session_under(
            AgentHarnessKind::Claude,
            "00000000-0000-4000-8000-000000000000",
            existing_home.path()
        ),
        ProviderResumeMode::Resume
    );
}

#[tokio::test]
async fn codex_recovery_resume_command_forwards_a_resolved_persona_block() {
    let (conversation, persona) = bound_project_persona().await;
    let (home, _cli_temp, cli_path, plugin_dir, working_dir) = codex_command_fixture();

    let result = with_provider_state_home_override(home.path(), || async {
        build_resume_command_for_harness(
            AgentHarnessKind::Codex,
            &cli_path,
            &plugin_dir,
            ChatContextType::Project,
            conversation.context_id.as_str(),
            CoordinationMode::Solo,
            &conversation.id.as_str(),
            conversation.agent_mode,
            None,
            "continue",
            Some(persona),
            None,
            None,
            &working_dir,
            "missing-session",
            Some(conversation.context_id.as_str()),
            &[],
            Some(conversation.id.to_string()),
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            Arc::new(MockIdeationRepo::empty()),
            empty_delegated_session_repo(),
            Arc::new(MockTaskRepo),
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("codex recovery command should build");

    let args = result.spawnable.get_args_for_test();
    assert_eq!(args.first().map(String::as_str), Some("exec"));
    assert!(
        !args.iter().any(|arg| arg == "resume"),
        "missing Codex session should force recovery, not exec resume: {args:?}"
    );
    assert!(
        spawnable_prompt(&result.spawnable).contains("<ralphx_agent_persona>"),
        "the Codex recovery prompt must retain the resolved persona block"
    );
    assert!(result.persona_injected());
    assert_eq!(result.persona_injection_skipped_reason(), None);
}

#[tokio::test]
async fn persona_codex_resume_command_uses_resume_subcommand_and_reports_injection() {
    let (_, persona) = bound_project_persona().await;
    let home = make_codex_home_with_session("session-123");
    let cli_temp = tempfile::tempdir().expect("tempdir");
    let cli_path = make_fake_codex_cli(&cli_temp);
    let plugin_dir = cli_temp.path().join("plugins").join("app");
    fs::create_dir_all(&plugin_dir).expect("create plugin dir");
    write_file(
        &plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp server",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-plan-verifier/agent.yaml"),
        "name: ralphx-plan-verifier\nrole: plan_verifier\n",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-plan-verifier/codex/agent.yaml"),
        "runtime_features:\n  shell_tool: false\n",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-chat-project/agent.yaml"),
        "name: ralphx-chat-project\nrole: project_chat\n",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-chat-project/codex/prompt.md"),
        "You are the RalphX project chat agent.",
    );
    let working_dir = cli_temp.path().to_path_buf();

    let result = with_provider_state_home_override(home.path(), || async {
        build_resume_command_for_harness(
            AgentHarnessKind::Codex,
            &cli_path,
            &plugin_dir,
            ChatContextType::Project,
            "project-1",
            CoordinationMode::Solo,
            "codex-project-resume-conversation",
            None,
            None,
            "continue",
            Some(persona),
            None,
            None,
            &working_dir,
            "session-123",
            None,
            &[],
            None,
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            Arc::new(MockIdeationRepo::empty()),
            empty_delegated_session_repo(),
            Arc::new(MockTaskRepo),
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("codex resume command should build");

    let args = result.spawnable.get_args_for_test();
    assert!(
        args.windows(3)
            .any(|window| window == ["exec", "resume", "session-123"]),
        "existing Codex session should use exec resume: {args:?}"
    );
    assert!(
        spawnable_prompt(&result.spawnable).contains("<ralphx_agent_persona>"),
        "the queued Codex resume prompt must retain the resolved persona block"
    );
    assert!(result.persona_injected());
    assert_eq!(result.persona_injection_skipped_reason(), None);
}

#[tokio::test]
async fn codex_legacy_verification_session_uses_active_ideation_features() {
    let home = tempfile::tempdir().expect("tempdir");
    let cli_temp = tempfile::tempdir().expect("tempdir");
    let cli_path = make_fake_codex_cli(&cli_temp);
    let plugin_dir = cli_temp.path().join("plugins").join("app");
    fs::create_dir_all(&plugin_dir).expect("create plugin dir");
    write_file(
        &plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp server",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-plan-verifier/agent.yaml"),
        "name: ralphx-plan-verifier\nrole: plan_verifier\n",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-plan-verifier/codex/agent.yaml"),
        "runtime_features:\n  shell_tool: false\n",
    );
    write_file(
        &cli_temp.path().join("agents/ralphx-ideation/agent.yaml"),
        "name: ralphx-ideation\nrole: ideation_orchestrator\n",
    );
    write_file(
        &cli_temp
            .path()
            .join("agents/ralphx-ideation/codex/agent.yaml"),
        "runtime_features:\n  shell_tool: true\n",
    );
    let working_dir = cli_temp.path().to_path_buf();
    let parent_id = IdeationSessionId::new();
    let child_id = IdeationSessionId::new();
    let verification_child = IdeationSession::builder()
        .id(child_id.clone())
        .project_id(ProjectId::new())
        .parent_session_id(parent_id)
        .session_purpose(SessionPurpose::Verification)
        .build();

    let result = with_provider_state_home_override(home.path(), || async {
        build_resume_command_for_harness(
            AgentHarnessKind::Codex,
            &cli_path,
            &plugin_dir,
            ChatContextType::Ideation,
            child_id.as_str(),
            CoordinationMode::Solo,
            "codex-ideation-recovery-conversation",
            None,
            None,
            "continue",
            None,
            None,
            None,
            &working_dir,
            "missing-session",
            None,
            &[],
            None,
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            Arc::new(MockIdeationRepo::with_session(verification_child)),
            empty_delegated_session_repo(),
            Arc::new(MockTaskRepo),
            &[],
            0,
            None,
            None,
            false,
            None,
            None,
        )
        .await
    })
    .await
    .expect("codex ideation command should build");

    let args = result.spawnable.get_args_for_test();
    assert!(
        args.iter().any(|arg| arg == "features.shell_tool=true"),
        "verification must use the active ideation agent's Codex features: {args:?}"
    );
    assert!(
        !args.iter().any(|arg| arg == "features.shell_tool=false"),
        "legacy verifier features must not override the active ideation agent: {args:?}"
    );
    let rendered_args = args.join("\n");
    assert!(
        rendered_args.contains("--agent-type") && rendered_args.contains("ralphx-ideation"),
        "verification must launch the active ideation agent: {args:?}"
    );
    assert!(
        !rendered_args.contains("ralphx-plan-verifier"),
        "verification must not resurrect the removed fixed verifier: {args:?}"
    );

    let envs = result.spawnable.get_envs_for_test();
    let working_dir_env = envs
        .iter()
        .find(|(key, _)| key == "RALPHX_WORKING_DIRECTORY")
        .map(|(_, value)| value.to_string_lossy().into_owned());
    assert_eq!(
        working_dir_env.as_deref(),
        Some(working_dir.to_string_lossy().as_ref()),
        "spawn env must carry canonical working directory for MCP filesystem tools"
    );
}

#[tokio::test]
async fn task_execution_launch_injects_compact_runtime_context_and_state_env() {
    let task_id = TaskId::from_string("task-runtime-exec".to_string());
    let conversation = ChatConversation::new_task_execution(task_id.clone());
    let working_dir = tempfile::tempdir().expect("working dir");

    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conversation,
            "Execute task: task-runtime-exec",
            None,
            working_dir.path(),
            Some("executing"),
            Some("project-runtime"),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await
    .expect("task execution command should build");

    let prompt = spawnable_prompt(&command);
    assert!(prompt.contains("<task_runtime_context>"));
    assert!(prompt.contains("<task_id>task-runtime-exec</task_id>"));
    assert!(prompt.contains("<project_id>project-runtime</project_id>"));
    assert!(prompt.contains("<context_type>task_execution</context_type>"));
    assert!(prompt.contains("<task_state>executing</task_state>"));
    assert!(prompt.contains(&format!(
        "<working_directory>{}</working_directory>",
        working_dir.path().to_string_lossy()
    )));
    assert!(
        !prompt.contains("<source_proposal") && !prompt.contains("<plan_artifact"),
        "bootstrap runtime context must stay compact and avoid full plan/proposal payloads: {prompt}"
    );

    let envs = command.get_envs_for_test();
    assert_eq!(
        env_value(&envs, "RALPHX_TASK_ID").as_deref(),
        Some(task_id.as_str())
    );
    assert_eq!(
        env_value(&envs, "RALPHX_CONTEXT_ID").as_deref(),
        Some(task_id.as_str())
    );
    assert_eq!(
        env_value(&envs, "RALPHX_TASK_STATE").as_deref(),
        Some("executing")
    );
}

#[tokio::test]
async fn task_execution_launch_fails_closed_without_project_identity() {
    let task_id = TaskId::from_string("task-runtime-missing-project".to_string());
    let conversation = ChatConversation::new_task_execution(task_id);
    let working_dir = tempfile::tempdir().expect("working dir");

    let result = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conversation,
            "Execute task: task-runtime-missing-project",
            None,
            working_dir.path(),
            Some("executing"),
            None,
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await;

    let error = result.expect_err("task runtime context must fail without project id");
    assert!(
        error.contains("missing project identity"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn task_reexecution_launch_injects_reexecuting_state() {
    let task_id = TaskId::from_string("task-runtime-reexec".to_string());
    let conversation = ChatConversation::new_task_execution(task_id.clone());
    let working_dir = tempfile::tempdir().expect("working dir");

    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conversation,
            "Re-execute task (revision): task-runtime-reexec",
            None,
            working_dir.path(),
            Some("re_executing"),
            Some("project-runtime"),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await
    .expect("re-execution command should build");

    let prompt = spawnable_prompt(&command);
    assert!(prompt.contains("<task_runtime_context>"));
    assert!(prompt.contains("<task_state>re_executing</task_state>"));
    assert!(prompt.contains("<context_type>task_execution</context_type>"));
    assert_eq!(
        env_value(&command.get_envs_for_test(), "RALPHX_TASK_STATE").as_deref(),
        Some("re_executing")
    );
}

#[tokio::test]
async fn review_launch_injects_reviewing_runtime_context_and_state_env() {
    let task_id = TaskId::from_string("task-runtime-review".to_string());
    let conversation = ChatConversation::new_review(task_id.clone());
    let working_dir = tempfile::tempdir().expect("working dir");

    let command = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conversation,
            "Review task: task-runtime-review",
            None,
            working_dir.path(),
            Some("reviewing"),
            Some("project-runtime"),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            None,
            None,
            None,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await
    .expect("review command should build");

    let prompt = spawnable_prompt(&command);
    assert!(prompt.contains("<task_runtime_context>"));
    assert!(prompt.contains("<task_id>task-runtime-review</task_id>"));
    assert!(prompt.contains("<context_type>review</context_type>"));
    assert!(prompt.contains("<task_state>reviewing</task_state>"));
    assert_eq!(
        env_value(&command.get_envs_for_test(), "RALPHX_TASK_STATE").as_deref(),
        Some("reviewing")
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests for format_session_history
//
// Ordering assumption: the slice passed to format_session_history is in
// chronological order — index 0 is the oldest message, last index is the newest.
// format_session_history iterates with .rev() (newest-first) so that when the
// 8000-char cap is hit, oldest messages are evicted and newest messages survive.
// ──────────────────────────────────────────────────────────────────────────────

fn make_user_msg(session_id: &IdeationSessionId, content: &str) -> ChatMessage {
    ChatMessage::user_in_session(session_id.clone(), content)
}

fn make_orchestrator_msg(session_id: &IdeationSessionId, content: &str) -> ChatMessage {
    ChatMessage::orchestrator_in_session(session_id.clone(), content)
}

fn make_system_msg(session_id: &IdeationSessionId, content: &str) -> ChatMessage {
    ChatMessage::system_in_session(session_id.clone(), content)
}

#[test]
fn format_session_history_empty_slice_returns_empty_string() {
    let result = format_session_history(&[], 0);
    assert_eq!(result, "");
}

#[test]
fn format_session_history_only_system_messages_returns_empty_string() {
    let sid = IdeationSessionId::new();
    let msgs = vec![make_system_msg(&sid, "system init")];
    let result = format_session_history(&msgs, 1);
    assert_eq!(result, "");
}

#[test]
fn format_session_history_basic_user_and_orchestrator() {
    let sid = IdeationSessionId::new();
    let user_msg = make_user_msg(&sid, "hello");
    let orch_msg = make_orchestrator_msg(&sid, "hi back");
    let msgs = vec![user_msg, orch_msg];
    let result = format_session_history(&msgs, 2);
    assert!(result.contains("<session_history"));
    assert!(result.contains("count=\"2\""));
    assert!(result.contains("total_available=\"2\""));
    assert!(result.contains("truncated=\"false\""));
    assert!(result.contains(r#"role="user""#));
    assert!(result.contains("hello"));
    assert!(result.contains(r#"role="orchestrator""#));
    assert!(result.contains("hi back"));
    assert!(result.contains("</session_history>"));
}

#[test]
fn format_session_history_xml_escaping() {
    let sid = IdeationSessionId::new();
    let msg = make_user_msg(&sid, r#"5 < 10 & "hello" > world"#);
    let result = format_session_history(&[msg], 1);
    assert!(result.contains("5 &lt; 10 &amp; &quot;hello&quot; &gt; world"));
    // Raw chars must not appear unescaped inside tag content
    assert!(!result.contains("5 < 10"));
}

#[test]
fn format_session_history_recovery_context_filtered_out() {
    let sid = IdeationSessionId::new();
    let mut recovery_msg = make_user_msg(&sid, "this is recovery");
    recovery_msg.metadata = Some(r#"{"recovery_context": true}"#.to_string());
    let normal_msg = make_user_msg(&sid, "normal message");
    let msgs = vec![recovery_msg, normal_msg];
    let result = format_session_history(&msgs, 2);
    assert!(!result.contains("this is recovery"));
    assert!(result.contains("normal message"));
}

#[test]
fn format_session_history_all_recovery_context_returns_empty_string() {
    let sid = IdeationSessionId::new();
    let mut msg = make_user_msg(&sid, "recovery only");
    msg.metadata = Some(r#"{"recovery_context": true}"#.to_string());
    let result = format_session_history(&[msg], 1);
    assert_eq!(result, "");
}

#[test]
fn format_session_history_subagent_roles_filtered_out() {
    let sid = IdeationSessionId::new();
    // Worker, Reviewer, Merger roles should be excluded (not User or Orchestrator)
    let mut worker_msg = ChatMessage::user_in_session(sid.clone(), "worker output");
    worker_msg.role = MessageRole::Worker;
    let mut reviewer_msg = ChatMessage::user_in_session(sid.clone(), "reviewer output");
    reviewer_msg.role = MessageRole::Reviewer;
    let mut merger_msg = ChatMessage::user_in_session(sid.clone(), "merger output");
    merger_msg.role = MessageRole::Merger;
    let user_msg = make_user_msg(&sid, "user message");

    let msgs = vec![worker_msg, reviewer_msg, merger_msg, user_msg];
    let result = format_session_history(&msgs, 4);
    assert!(!result.contains("worker output"));
    assert!(!result.contains("reviewer output"));
    assert!(!result.contains("merger output"));
    assert!(result.contains("user message"));
}

#[test]
fn format_session_history_per_message_2000_char_truncation() {
    let sid = IdeationSessionId::new();
    let long_content = "x".repeat(3000);
    let msg = make_user_msg(&sid, &long_content);
    let result = format_session_history(&[msg], 1);
    // The 2000 x's should be there, but not 2001+
    assert!(result.contains(&"x".repeat(2000)));
    assert!(!result.contains(&"x".repeat(2001)));
    assert!(result.contains("[truncated]"));
}

#[test]
fn format_session_history_8000_char_cap() {
    let sid = IdeationSessionId::new();
    // Array position = chronological order; .rev() treats last element as newest.
    // Create messages that together exceed 8000 chars after escaping.
    let mut msgs = Vec::new();
    // Each message ~1500 chars, so 6 messages = 9000 chars; cap at 8000 should stop at ~5.
    // Messages are indexed 0 (oldest) through 5 (newest).
    for i in 0..6 {
        msgs.push(make_user_msg(&sid, &format!("{}: {}", i, "y".repeat(1490))));
    }
    let result = format_session_history(&msgs, 6);
    // Should be truncated
    assert!(result.contains("truncated=\"true\""));
    // Should NOT contain all 6 messages' count
    let count_attr_start = result.find("count=\"").unwrap();
    let count_start = count_attr_start + 7;
    let count_end = result[count_start..].find('"').unwrap() + count_start;
    let count: usize = result[count_start..count_end].parse().unwrap();
    assert!(
        count < 6,
        "Expected fewer than 6 messages due to 8000-char cap, got {}",
        count
    );
    // Directional: newest messages (highest index) MUST be present; oldest MUST be absent.
    // Without .rev(), oldest messages would survive the cap and newest would be dropped.
    // Use content-specific substrings (not just "0:" / "5:") to avoid false positives
    // from ISO timestamps like "20:xx:xxZ" which also contain those character sequences.
    assert!(
        result.contains("5: yyy"),
        "Newest message (index 5) must survive the char cap"
    );
    assert!(
        !result.contains("0: yyy"),
        "Oldest message (index 0) must be dropped when cap is hit"
    );
}

#[test]
fn format_session_history_tool_summary_aggregation() {
    let sid = IdeationSessionId::new();
    let mut orch_msg = make_orchestrator_msg(&sid, "");
    orch_msg.tool_calls = Some(
        r#"[{"name":"create_task_proposal","arguments":"{}","result":{"content":"ok","is_error":false}},{"name":"create_task_proposal","arguments":"{}","result":{"content":"ok","is_error":false}},{"name":"update_plan_artifact","arguments":"{}","result":{"content":"ok","is_error":false}}]"#
            .to_string(),
    );
    let result = format_session_history(&[orch_msg], 1);
    assert!(result.contains("[Used: create_task_proposal x2, update_plan_artifact]"));
    assert!(result.contains(r#"role="tool_summary""#));
}

#[test]
fn format_session_history_tool_summary_with_failed_call() {
    let sid = IdeationSessionId::new();
    let mut orch_msg = make_orchestrator_msg(&sid, "thinking");
    orch_msg.tool_calls = Some(
        r#"[{"name":"create_plan_artifact","arguments":"{}","result":{"content":"ok","is_error":false}},{"name":"get_proposal","arguments":"{}","result":{"content":"err","is_error":true}}]"#
            .to_string(),
    );
    let result = format_session_history(&[orch_msg], 1);
    assert!(result.contains("get_proposal (failed)"));
    assert!(result.contains("create_plan_artifact"));
}

#[test]
fn format_session_history_empty_tool_calls_no_summary() {
    let sid = IdeationSessionId::new();
    let mut orch_msg = make_orchestrator_msg(&sid, "just text");
    orch_msg.tool_calls = Some("[]".to_string());
    let result = format_session_history(&[orch_msg], 1);
    assert!(!result.contains("tool_summary"));
    assert!(result.contains("just text"));
}

#[test]
fn format_session_history_truncated_true_when_total_available_larger() {
    let sid = IdeationSessionId::new();
    let msg = make_user_msg(&sid, "hello");
    // Only 1 message provided but total_available=100 → truncated=true
    let result = format_session_history(&[msg], 100);
    assert!(result.contains("truncated=\"true\""));
    assert!(result.contains("total_available=\"100\""));
}

#[test]
fn format_session_history_truncated_false_when_all_included() {
    let sid = IdeationSessionId::new();
    let msgs = vec![make_user_msg(&sid, "msg1"), make_user_msg(&sid, "msg2")];
    let result = format_session_history(&msgs, 2);
    assert!(result.contains("truncated=\"false\""));
}

#[test]
fn format_session_history_orchestrator_with_text_and_tools() {
    let sid = IdeationSessionId::new();
    let mut orch_msg = make_orchestrator_msg(&sid, "Here is my analysis");
    orch_msg.tool_calls = Some(
        r#"[{"name":"search","arguments":"{}","result":{"content":"results","is_error":false}}]"#
            .to_string(),
    );
    let result = format_session_history(&[orch_msg], 1);
    // Both text AND tool_summary should appear
    assert!(result.contains("Here is my analysis"));
    assert!(result.contains("[Used: search]"));
}

#[test]
fn format_session_history_group_reversal_invariant() {
    // Verifies that per-message groups (text + tool_summary) are kept together and
    // in correct intra-message order after the group-level reversal.
    // A flat-list reversal would put tool_summary BEFORE the message text — this test catches that.
    let sid = IdeationSessionId::new();
    let mut orch_msg = make_orchestrator_msg(&sid, "analysis text");
    orch_msg.tool_calls = Some(
        r#"[{"name":"create_task_proposal","arguments":"{}","result":{"content":"ok","is_error":false}}]"#
            .to_string(),
    );
    let result = format_session_history(&[orch_msg], 1);
    // Both entries must appear
    assert!(result.contains("analysis text"));
    assert!(result.contains(r#"role="tool_summary""#));
    // Message text must appear BEFORE tool_summary — group order preserved after reversal.
    let text_pos = result.find("analysis text").unwrap();
    let summary_pos = result.find(r#"role="tool_summary""#).unwrap();
    assert!(
        text_pos < summary_pos,
        "Message text must come before tool_summary (flat-list reversal regression guard)"
    );
}

#[test]
fn format_session_history_newest_priority_under_char_cap() {
    // When the 8000-char cap is hit, newest messages (highest array index) must survive.
    // This test is distinct from 8000_char_cap: it uses unique sentinel strings so
    // presence/absence of specific messages can be asserted unambiguously.
    // Array position = chronological order; .rev() treats last element as newest.
    let sid = IdeationSessionId::new();
    let filler = "z".repeat(1490);
    // 4 messages ~1500 chars each = ~6000 chars fits; add a 5th and 6th to force truncation.
    let msgs = vec![
        make_user_msg(&sid, &format!("OLDEST_MSG {}", filler)),
        make_user_msg(&sid, &format!("SECOND_MSG {}", filler)),
        make_user_msg(&sid, &format!("THIRD_MSG {}", filler)),
        make_user_msg(&sid, &format!("FOURTH_MSG {}", filler)),
        make_user_msg(&sid, &format!("FIFTH_MSG {}", filler)),
        make_user_msg(&sid, &format!("NEWEST_MSG {}", filler)),
    ];
    let result = format_session_history(&msgs, 6);
    assert!(result.contains("truncated=\"true\""));
    // Newest message must always be present
    assert!(
        result.contains("NEWEST_MSG"),
        "Newest message must survive the char cap"
    );
    // Oldest message must be dropped (oldest-first eviction)
    assert!(
        !result.contains("OLDEST_MSG"),
        "Oldest message must be evicted when cap is hit"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests for build_initial_prompt with session_history injection
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn build_initial_prompt_ideation_with_messages_injects_session_history() {
    let sid = IdeationSessionId::new();
    let msg = make_user_msg(&sid, "prior message");
    let result = build_initial_prompt(
        ChatContextType::Ideation,
        sid.as_str(),
        "new message",
        &[msg],
        1,
    );
    assert!(result.contains("<session_history"));
    assert!(result.contains("prior message"));
    assert!(result.contains("<user_message>new message</user_message>"));
    // session_history should come before user_message
    let hist_pos = result.find("<session_history").unwrap();
    let user_pos = result.find("<user_message>").unwrap();
    assert!(hist_pos < user_pos);
}

#[test]
fn build_initial_prompt_ideation_empty_messages_no_session_history_block() {
    let result = build_initial_prompt(ChatContextType::Ideation, "session-123", "hello", &[], 0);
    assert!(!result.contains("<session_history"));
    assert!(result.contains("<user_message>hello</user_message>"));
}

#[test]
fn build_initial_prompt_non_ideation_ignores_messages() {
    let sid = IdeationSessionId::new();
    let msg = make_user_msg(&sid, "some prior message");
    // Task context should NOT inject session_history even if messages provided
    let result = build_initial_prompt(
        ChatContextType::TaskExecution,
        "task-abc",
        "execute",
        &[msg],
        0,
    );
    assert!(!result.contains("<session_history"));
}

// ──────────────────────────────────────────────────────────────────────────────
// Integration tests: send_message → prompt pipeline (Wave 3 wiring)
//
// These tests verify the full pipeline: repo-fetched messages → build_initial_prompt
// → <session_history> XML in the resulting prompt. They simulate what send_message()
// does when spawning a new Ideation process: fetch messages, pass to prompt builder.
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn integration_ideation_spawn_prompt_pipeline_injects_session_history() {
    // Simulate what send_message() does for Ideation context on new process spawn:
    //   1. get_recent_by_session() returns prior messages
    //   2. build_initial_prompt() receives them and injects <session_history>
    let sid = IdeationSessionId::new();

    // Simulate repo-fetched messages (user + orchestrator with tool usage)
    let user_msg1 = make_user_msg(&sid, "I want to add dark mode");
    let mut orch_msg = make_orchestrator_msg(&sid, "Let me explore the codebase");
    orch_msg.tool_calls = Some(
        r#"[{"name":"create_task_proposal","arguments":"{}","result":{"content":"ok","is_error":false}}]"#
            .to_string(),
    );
    let user_msg2 = make_user_msg(&sid, "Also add a light mode toggle");
    let repo_messages = vec![user_msg1, orch_msg, user_msg2];
    let total_available = repo_messages.len();

    // This mirrors what happens in send_message() → build_interactive_command() → build_initial_prompt()
    // total_available comes from count_by_session; here we simulate it as the actual DB count.
    let prompt = build_initial_prompt(
        ChatContextType::Ideation,
        sid.as_str(),
        "What is the current progress?",
        &repo_messages,
        total_available,
    );

    // Verify session_history is present and contains prior messages
    assert!(
        prompt.contains("<session_history"),
        "prompt must contain <session_history> block"
    );
    assert!(
        prompt.contains("I want to add dark mode"),
        "prior user message must appear in history"
    );
    assert!(
        prompt.contains("Let me explore the codebase"),
        "prior orchestrator message must appear in history"
    );
    assert!(
        prompt.contains("[Used: create_task_proposal]"),
        "tool usage must be summarised in history"
    );
    assert!(
        prompt.contains("Also add a light mode toggle"),
        "second prior user message must appear in history"
    );
    assert!(
        prompt.contains("What is the current progress?"),
        "current user message must appear in prompt"
    );
    assert!(
        prompt.contains(&format!("total_available=\"{}\"", total_available)),
        "total_available attribute must match message count"
    );

    // session_history block must appear before <user_message>
    let hist_pos = prompt.find("<session_history").unwrap();
    let user_pos = prompt.find("<user_message>").unwrap();
    assert!(
        hist_pos < user_pos,
        "<session_history> must appear before <user_message>"
    );
}

#[test]
fn integration_ideation_spawn_first_message_no_session_history_block() {
    // When send_message() fetches 0 messages (first ever message in session),
    // the prompt must NOT contain a <session_history> block.
    let sid = IdeationSessionId::new();

    let prompt = build_initial_prompt(
        ChatContextType::Ideation,
        sid.as_str(),
        "Hello, start a new plan",
        &[], // empty — simulates count_by_session() == 0
        0,
    );

    assert!(
        !prompt.contains("<session_history"),
        "first message in session must not have <session_history> block"
    );
    assert!(
        prompt.contains("Hello, start a new plan"),
        "current user message must be present"
    );
    assert!(
        prompt.contains("<session_bootstrap_mode>fresh</session_bootstrap_mode>"),
        "fresh ideation spawn must mark bootstrap mode explicitly so prompt logic can skip recovery-only MCP calls"
    );
}

#[test]
fn integration_ideation_resume_prompt_marks_provider_resume_bootstrap_mode() {
    let sid = IdeationSessionId::new();

    let prompt = build_resume_initial_prompt(
        ChatContextType::Ideation,
        sid.as_str(),
        "continue the same plan",
        &[],
        0,
        None,
    );

    assert!(
        prompt.contains("<session_bootstrap_mode>provider_resume</session_bootstrap_mode>"),
        "provider resume prompts must be distinguished from fresh ideation and explicit recovery flows"
    );
}

#[test]
fn integration_non_ideation_spawn_no_session_history_even_with_messages() {
    // send_message() passes empty slice for non-Ideation contexts.
    // Even if messages were somehow provided, non-Ideation build_initial_prompt ignores them.
    let sid = IdeationSessionId::new();
    let msg = make_user_msg(&sid, "prior work message");

    let prompt = build_initial_prompt(
        ChatContextType::TaskExecution,
        "task-abc-123",
        "execute task",
        &[msg], // non-ideation: this must be ignored
        0,
    );

    assert!(
        !prompt.contains("<session_history"),
        "non-Ideation context must never inject <session_history>"
    );
    assert!(
        prompt.contains("execute task"),
        "current user message must be present"
    );
}

#[test]
fn integration_ideation_spawn_truncated_history_uses_db_count_not_slice_len() {
    // Regression test: when a session has >SESSION_HISTORY_LIMIT messages,
    // total_available must come from count_by_session (the real DB count),
    // not from session_messages.len() (which is capped at the limit).
    // Bug: format_session_history(session_messages, session_messages.len()) would emit
    //   total_available="50" truncated="false" even when DB has 200 messages.
    // Fix: thread total_available through build_initial_prompt from send_message().
    let sid = IdeationSessionId::new();

    // Simulate fetching the last 2 messages from a session with 200 total.
    let msg1 = make_user_msg(&sid, "recent message 1");
    let msg2 = make_user_msg(&sid, "recent message 2");
    let session_messages = vec![msg1, msg2];
    let db_count: usize = 200; // real count from count_by_session

    let prompt = build_initial_prompt(
        ChatContextType::Ideation,
        sid.as_str(),
        "continue",
        &session_messages,
        db_count,
    );

    // Must use DB count, not slice length
    assert!(
        prompt.contains(&format!("total_available=\"{}\"", db_count)),
        "total_available must be the DB count ({}), not the slice length ({})",
        db_count,
        session_messages.len()
    );
    assert!(
        prompt.contains("truncated=\"true\""),
        "truncated must be true when DB count ({}) > slice len ({})",
        db_count,
        session_messages.len()
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests for resolve_working_directory — merge context worktree prefix filter
// Fix: commit cfb57e0e — accept both merge- and rebase- prefixes for merge worktrees
// ──────────────────────────────────────────────────────────────────────────────

/// Test 1: Merger agent spawn accepts rebase-{task_id} worktree path.
///
/// Regression test for commit cfb57e0e: before the fix, only merge- was accepted.
/// rebase- prefixed worktrees are created by the checkout-free rebase strategy and
/// must be valid merge agent working directories.
#[tokio::test]
async fn resolve_working_directory_merge_context_accepts_rebase_prefix() {
    let parent = tempfile::TempDir::new().unwrap();
    let wt = parent.path().join("rebase-abc123");
    std::fs::create_dir_all(&wt).unwrap();
    let wt_path = wt.to_str().unwrap().to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let project_dir = parent.path().join("main-repo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut project = Project::new(
        "test".to_string(),
        project_dir.to_str().unwrap().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "test task".to_string());
    task.worktree_path = Some(wt_path.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let result = resolve_working_directory(
        ChatContextType::Merge,
        task_id.as_str(),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::new(MockIdeationRepo::empty()) as Arc<dyn IdeationSessionRepository>,
        empty_delegated_session_repo(),
        std::path::Path::new("/tmp/default"),
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "rebase- prefixed worktree must be accepted for Merge context. Got: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        std::path::PathBuf::from(&wt_path),
        "Must return the rebase- worktree path as the working directory"
    );
}

/// Test 2: Merger agent spawn accepts merge-{task_id} worktree path (existing behavior not broken).
///
/// Confirms the original merge- prefix continues to work after the fix.
#[tokio::test]
async fn resolve_working_directory_merge_context_accepts_merge_prefix() {
    let parent = tempfile::TempDir::new().unwrap();
    let wt = parent.path().join("merge-abc123");
    std::fs::create_dir_all(&wt).unwrap();
    let wt_path = wt.to_str().unwrap().to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let project_dir = parent.path().join("main-repo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut project = Project::new(
        "test".to_string(),
        project_dir.to_str().unwrap().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "test task".to_string());
    task.worktree_path = Some(wt_path.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let result = resolve_working_directory(
        ChatContextType::Merge,
        task_id.as_str(),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::new(MockIdeationRepo::empty()) as Arc<dyn IdeationSessionRepository>,
        empty_delegated_session_repo(),
        std::path::Path::new("/tmp/default"),
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "merge- prefixed worktree must still be accepted for Merge context. Got: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        std::path::PathBuf::from(&wt_path),
        "Must return the merge- worktree path as the working directory"
    );
}

/// Test 3: Merger agent spawn rejects non-merge worktree paths (e.g., task-{task_id}).
///
/// A task worktree (task- prefix) must never be used as a merge agent working directory.
/// The guard must reject it with an error rather than silently falling back.
#[tokio::test]
async fn resolve_working_directory_merge_context_rejects_task_prefix() {
    let parent = tempfile::TempDir::new().unwrap();
    let wt = parent.path().join("task-abc123");
    std::fs::create_dir_all(&wt).unwrap();
    let wt_path = wt.to_str().unwrap().to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let project_dir = parent.path().join("main-repo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut project = Project::new(
        "test".to_string(),
        project_dir.to_str().unwrap().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "test task".to_string());
    task.worktree_path = Some(wt_path.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let result = resolve_working_directory(
        ChatContextType::Merge,
        task_id.as_str(),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::new(MockIdeationRepo::empty()) as Arc<dyn IdeationSessionRepository>,
        empty_delegated_session_repo(),
        std::path::Path::new("/tmp/default"),
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "task- prefixed worktree must be rejected for Merge context (not a merge worktree). \
         Got Ok instead of Err."
    );
}

#[tokio::test]
async fn resolve_working_directory_review_rejects_missing_worktree_path() {
    let parent = tempfile::TempDir::new().unwrap();
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let project_dir = parent.path().join("main-repo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut project = Project::new(
        "test".to_string(),
        project_dir.to_str().unwrap().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project_repo.create(project).await.unwrap();

    let task = Task::new(project_id, "test task".to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let result = resolve_working_directory(
        ChatContextType::Review,
        task_id.as_str(),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::new(MockIdeationRepo::empty()) as Arc<dyn IdeationSessionRepository>,
        empty_delegated_session_repo(),
        std::path::Path::new("/tmp/default"),
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "Review context in Worktree mode must fail when worktree_path is missing"
    );
}

#[tokio::test]
async fn resolve_working_directory_task_execution_rejects_missing_worktree_dir() {
    let parent = tempfile::TempDir::new().unwrap();
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let project_dir = parent.path().join("main-repo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut project = Project::new(
        "test".to_string(),
        project_dir.to_str().unwrap().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "test task".to_string());
    task.worktree_path = Some(
        parent
            .path()
            .join("task-missing")
            .to_string_lossy()
            .to_string(),
    );
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let result = resolve_working_directory(
        ChatContextType::TaskExecution,
        task_id.as_str(),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::new(MockIdeationRepo::empty()) as Arc<dyn IdeationSessionRepository>,
        empty_delegated_session_repo(),
        std::path::Path::new("/tmp/default"),
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "TaskExecution in Worktree mode must fail when worktree_path directory is missing"
    );
}

#[tokio::test]
async fn resolve_working_directory_review_rejects_merge_worktree_path() {
    let parent = tempfile::TempDir::new().unwrap();
    let wt = parent.path().join("merge-abc123");
    std::fs::create_dir_all(&wt).unwrap();
    let wt_path = wt.to_str().unwrap().to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let project_dir = parent.path().join("main-repo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut project = Project::new(
        "test".to_string(),
        project_dir.to_str().unwrap().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "test task".to_string());
    task.worktree_path = Some(wt_path);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let result = resolve_working_directory(
        ChatContextType::Review,
        task_id.as_str(),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::new(MockIdeationRepo::empty()) as Arc<dyn IdeationSessionRepository>,
        empty_delegated_session_repo(),
        std::path::Path::new("/tmp/default"),
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "Review context must reject merge-* worktree paths"
    );
}

#[tokio::test]
async fn resolve_working_directory_ideation_uses_session_workspace() {
    let parent = tempfile::TempDir::new().unwrap();
    let project_dir = parent.path().join("main-repo");
    let analysis_dir = parent.path().join("ideation-worktree");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::create_dir_all(&analysis_dir).unwrap();

    let project_repo = Arc::new(MemoryProjectRepository::new());
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_id = ProjectId::from_string("proj-ideation".to_string());
    let mut project = Project::new(
        "test".to_string(),
        project_dir.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    let mut session = IdeationSession::new(project_id);
    session.analysis.workspace_kind = IdeationAnalysisWorkspaceKind::IdeationWorktree;
    session.analysis.workspace_path = Some(analysis_dir.to_string_lossy().to_string());
    let session_id = session.id.clone();
    let ideation_repo = Arc::new(MockIdeationRepo::with_session(session));

    let result = resolve_working_directory(
        ChatContextType::Ideation,
        session_id.as_str(),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        ideation_repo,
        empty_delegated_session_repo(),
        std::path::Path::new("/tmp/default"),
        None,
    )
    .await
    .expect("ideation workspace should resolve");

    assert_eq!(result, analysis_dir);
}

#[tokio::test]
async fn resolve_working_directory_ideation_rejects_missing_session_workspace() {
    let parent = tempfile::TempDir::new().unwrap();
    let project_dir = parent.path().join("main-repo");
    std::fs::create_dir_all(&project_dir).unwrap();

    let project_repo = Arc::new(MemoryProjectRepository::new());
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_id = ProjectId::from_string("proj-ideation".to_string());
    let mut project = Project::new(
        "test".to_string(),
        project_dir.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    let mut session = IdeationSession::new(project_id);
    session.analysis.workspace_kind = IdeationAnalysisWorkspaceKind::IdeationWorktree;
    session.analysis.workspace_path = Some(
        parent
            .path()
            .join("missing-ideation-worktree")
            .to_string_lossy()
            .to_string(),
    );
    let session_id = session.id.clone();
    let ideation_repo = Arc::new(MockIdeationRepo::with_session(session));

    let result = resolve_working_directory(
        ChatContextType::Ideation,
        session_id.as_str(),
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        ideation_repo,
        empty_delegated_session_repo(),
        std::path::Path::new("/tmp/default"),
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "Ideation sessions requiring a dedicated workspace must not fall back to project root"
    );
}

// --- Verifier subagent cap injection tests ---
//
// These tests verify that build_command correctly resolves CLAUDE_CODE_SUBAGENT_MODEL
// from the verifier_subagent_model DB field for ralphx-plan-verifier, and that non-verifier
// agents use their own resolved model as the subagent cap instead.

#[tokio::test]
async fn test_plan_verifier_sets_subagent_cap_env_var() {
    // When build_command is called with entity_status="verification" (ralphx-plan-verifier),
    // and the DB has verifier_subagent_model=haiku, then CLAUDE_CODE_SUBAGENT_MODEL=haiku
    // must appear in the spawned command's environment variables.
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project("proj-1", "opus", "sonnet", "haiku", "inherit")
        .await
        .unwrap();

    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let artifact_repo = Arc::new(MemoryArtifactRepository::new());
    let attachment_repo = Arc::new(MemoryChatAttachmentRepository::new());
    let settings_repo: Arc<dyn IdeationModelSettingsRepository> = Arc::new(repo);

    let result = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conv,
            "continue",
            None,
            std::path::Path::new("/tmp"),
            Some("verification"),
            Some("proj-1"),
            &[],
            attachment_repo,
            artifact_repo,
            None,
            None,
            Some(settings_repo),
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await;

    assert!(result.is_ok(), "build_command failed: {:?}", result.err());
    let cmd = result.unwrap();
    let envs = cmd.get_envs_for_test();
    let subagent_model = envs
        .iter()
        .find(|(k, _)| k == "CLAUDE_CODE_SUBAGENT_MODEL")
        .map(|(_, v)| v.to_string_lossy().into_owned());

    assert_eq!(
        subagent_model.as_deref(),
        Some("haiku"),
        "CLAUDE_CODE_SUBAGENT_MODEL should be haiku for ralphx-plan-verifier with DB override"
    );
}

#[tokio::test]
async fn test_plan_verifier_subagent_cap_uses_haiku_default_when_no_db_rows() {
    // When the DB has no rows, the hardcoded "haiku" default must still appear
    // in CLAUDE_CODE_SUBAGENT_MODEL for ralphx-plan-verifier.
    let repo = MemoryIdeationModelSettingsRepository::new();
    // No rows seeded → falls back to "haiku"

    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let artifact_repo = Arc::new(MemoryArtifactRepository::new());
    let attachment_repo = Arc::new(MemoryChatAttachmentRepository::new());
    let settings_repo: Arc<dyn IdeationModelSettingsRepository> = Arc::new(repo);

    let result = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conv,
            "continue",
            None,
            std::path::Path::new("/tmp"),
            Some("verification"),
            None, // no project_id → no project row
            &[],
            attachment_repo,
            artifact_repo,
            None,
            None,
            Some(settings_repo),
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await;

    assert!(result.is_ok(), "build_command failed: {:?}", result.err());
    let cmd = result.unwrap();
    let envs = cmd.get_envs_for_test();
    let subagent_model = envs
        .iter()
        .find(|(k, _)| k == "CLAUDE_CODE_SUBAGENT_MODEL")
        .map(|(_, v)| v.to_string_lossy().into_owned());

    assert_eq!(
        subagent_model.as_deref(),
        Some("haiku"),
        "CLAUDE_CODE_SUBAGENT_MODEL should fall back to haiku when no DB rows exist"
    );
}

#[tokio::test]
async fn test_non_verifier_ideation_agent_subagent_cap_is_agent_own_model() {
    // For non-verifier ideation agents (ralphx-ideation), the subagent cap
    // must come from the IdeationSubagent lane row — NOT from the IdeationVerifierSubagent lane.
    use ralphx_lib::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings};

    let lane_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    // IdeationSubagent lane = "sonnet"; IdeationVerifierSubagent = "haiku" — must not bleed in
    lane_repo
        .upsert_global(
            AgentLane::IdeationSubagent,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Claude,
                model: Some("sonnet".to_string()),
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();
    lane_repo
        .upsert_global(
            AgentLane::IdeationVerifierSubagent,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Claude,
                model: Some("haiku".to_string()),
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();

    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let artifact_repo = Arc::new(MemoryArtifactRepository::new());
    let attachment_repo = Arc::new(MemoryChatAttachmentRepository::new());
    let lane_repo_arc: Arc<dyn ralphx_lib::domain::repositories::AgentLaneSettingsRepository> =
        lane_repo;

    // No entity_status → ralphx-ideation (default ideation agent)
    let result = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conv,
            "continue",
            None,
            std::path::Path::new("/tmp"),
            None, // no entity_status → ralphx-ideation
            Some("proj-1"),
            &[],
            attachment_repo,
            artifact_repo,
            Some(lane_repo_arc),
            None,
            None,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await;

    assert!(result.is_ok(), "build_command failed: {:?}", result.err());
    let cmd = result.unwrap();
    let envs = cmd.get_envs_for_test();
    let subagent_model = envs
        .iter()
        .find(|(k, _)| k == "CLAUDE_CODE_SUBAGENT_MODEL")
        .map(|(_, v)| v.to_string_lossy().into_owned());

    // The subagent cap for ralphx-ideation comes from IdeationSubagent lane row ("sonnet")
    assert_eq!(
        subagent_model.as_deref(),
        Some("sonnet"),
        "ralphx-ideation subagent cap should come from IdeationSubagent lane row"
    );
    assert_ne!(
        subagent_model.as_deref(),
        Some("haiku"),
        "IdeationVerifierSubagent lane must not bleed into non-verifier agents"
    );
}

#[tokio::test]
async fn test_orchestrator_ideation_uses_ideation_subagent_cap() {
    // build_command for ralphx-ideation must set CLAUDE_CODE_SUBAGENT_MODEL
    // to the IdeationSubagent lane row model ("sonnet"), NOT to the primary agent model ("opus").
    use ralphx_lib::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings};

    let lane_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    // IdeationPrimary=opus, IdeationSubagent=sonnet — they differ so we can distinguish
    lane_repo
        .upsert_for_project(
            "proj-1",
            AgentLane::IdeationPrimary,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Claude,
                model: Some("opus".to_string()),
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();
    lane_repo
        .upsert_for_project(
            "proj-1",
            AgentLane::IdeationSubagent,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Claude,
                model: Some("sonnet".to_string()),
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();

    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let artifact_repo = Arc::new(MemoryArtifactRepository::new());
    let attachment_repo = Arc::new(MemoryChatAttachmentRepository::new());
    let lane_repo_arc: Arc<dyn ralphx_lib::domain::repositories::AgentLaneSettingsRepository> =
        lane_repo;

    // entity_status=None → ralphx-ideation (non-verifier ideation agent)
    let result = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conv,
            "continue",
            None,
            std::path::Path::new("/tmp"),
            None, // no entity_status → ralphx-ideation
            Some("proj-1"),
            &[],
            attachment_repo,
            artifact_repo,
            Some(lane_repo_arc),
            None,
            None,
            &[],
            0,
            None,
            None, // model_override=None; primary model is "opus" from lane row
            None, // agent_runtime_context
            None, // attachment_context_override
        )
        .await
    })
    .await;

    assert!(result.is_ok(), "build_command failed: {:?}", result.err());
    let cmd = result.unwrap();
    let envs = cmd.get_envs_for_test();
    let subagent_model = envs
        .iter()
        .find(|(k, _)| k == "CLAUDE_CODE_SUBAGENT_MODEL")
        .map(|(_, v)| v.to_string_lossy().into_owned());

    // CLAUDE_CODE_SUBAGENT_MODEL must come from IdeationSubagent lane row ("sonnet"),
    // NOT from the agent's primary lane model ("opus").
    assert_eq!(
        subagent_model.as_deref(),
        Some("sonnet"),
        "CLAUDE_CODE_SUBAGENT_MODEL must equal IdeationSubagent lane model (sonnet), not primary model (opus)"
    );
    assert_ne!(
        subagent_model.as_deref(),
        Some("opus"),
        "primary lane model (opus) must NOT be used as CLAUDE_CODE_SUBAGENT_MODEL for ralphx-ideation"
    );
}

#[tokio::test]
async fn test_both_build_and_resume_use_ideation_subagent_cap() {
    // Both build_command AND build_resume_command must inject
    // CLAUDE_CODE_SUBAGENT_MODEL = IdeationSubagent lane row model for ralphx-ideation.
    use ralphx_lib::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings};
    use ralphx_lib::domain::repositories::AgentLaneSettingsRepository;

    let lane_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    // IdeationPrimary=opus, IdeationSubagent=sonnet — they differ so we can distinguish
    lane_repo
        .upsert_global(
            AgentLane::IdeationSubagent,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Claude,
                model: Some("sonnet".to_string()),
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();
    let lane_repo_arc: Arc<dyn AgentLaneSettingsRepository> = lane_repo;

    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id.clone());

    // --- Test build_command ---
    let build_result = with_claude_spawn_allowed_in_tests(|| async {
        build_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            &conv,
            "continue",
            None,
            std::path::Path::new("/tmp"),
            None,
            Some("proj-1"),
            &[],
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            Some(Arc::clone(&lane_repo_arc)),
            None,
            None,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await;

    assert!(
        build_result.is_ok(),
        "build_command failed: {:?}",
        build_result.err()
    );
    let build_cmd = build_result.unwrap();
    let build_envs = build_cmd.get_envs_for_test();
    let build_subagent = build_envs
        .iter()
        .find(|(k, _)| k == "CLAUDE_CODE_SUBAGENT_MODEL")
        .map(|(_, v)| v.to_string_lossy().into_owned());
    assert_eq!(
        build_subagent.as_deref(),
        Some("sonnet"),
        "build_command: CLAUDE_CODE_SUBAGENT_MODEL must be IdeationSubagent lane model (sonnet)"
    );

    // --- Test build_resume_command ---
    let resume_result = with_claude_spawn_allowed_in_tests(|| async {
        build_resume_command(
            std::path::Path::new("/fake/claude"),
            std::path::Path::new("/fake/plugin"),
            ChatContextType::Ideation,
            session_id.as_str(),
            CoordinationMode::Solo,
            "ideation-conversation-resume",
            None,
            None,
            "continue",
            None,
            None,
            None,
            std::path::Path::new("/tmp"),
            "fake-session-id",
            Some("proj-1"),
            &[],
            None,
            Arc::new(MemoryChatAttachmentRepository::new()),
            Arc::new(MemoryArtifactRepository::new()),
            Some(Arc::clone(&lane_repo_arc)),
            None,
            None,
            Arc::new(MemoryIdeationSessionRepository::new()),
            empty_delegated_session_repo(),
            Arc::new(MemoryTaskRepository::new()),
            &[],
            0,
            None,
            None,
            None,
            None,
        )
        .await
    })
    .await;

    assert!(
        resume_result.is_ok(),
        "build_resume_command failed: {:?}",
        resume_result.err()
    );
    let resume_cmd = resume_result.unwrap();
    let resume_envs = resume_cmd.get_envs_for_test();
    let resume_subagent = resume_envs
        .iter()
        .find(|(k, _)| k == "CLAUDE_CODE_SUBAGENT_MODEL")
        .map(|(_, v)| v.to_string_lossy().into_owned());
    assert_eq!(
        resume_subagent.as_deref(),
        Some("sonnet"),
        "build_resume_command: CLAUDE_CODE_SUBAGENT_MODEL must be IdeationSubagent lane model (sonnet)"
    );
}

#[tokio::test]
async fn test_build_command_resumes_from_provider_session_ref_without_legacy_alias() {
    let mut conversation = ChatConversation::new_ideation(IdeationSessionId::new());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "provider-only-session".to_string(),
    });
    conversation.claude_session_id = None;
    let home = make_claude_home_with_session("provider-only-session");

    let result = with_provider_state_home_override(home.path(), || async {
        with_claude_spawn_allowed_in_tests(|| async {
            build_command(
                std::path::Path::new("/fake/claude"),
                std::path::Path::new("/fake/plugin"),
                &conversation,
                "continue",
                None,
                std::path::Path::new("/tmp"),
                None,
                None,
                &[],
                Arc::new(MemoryChatAttachmentRepository::new()),
                Arc::new(MemoryArtifactRepository::new()),
                None,
                None,
                None,
                &[],
                0,
                None,
                None,
                None,
                None,
            )
            .await
        })
        .await
    })
    .await;

    assert!(result.is_ok(), "build_command failed: {:?}", result.err());
    let command = result.unwrap();
    let args = command.get_args_for_test();

    assert!(
        args.windows(2)
            .any(|window| window[0] == "--resume" && window[1] == "provider-only-session"),
        "build_command must resume from the canonical provider session reference",
    );

    let lead_session = command
        .get_envs_for_test()
        .iter()
        .find(|(key, _)| key == "RALPHX_LEAD_SESSION_ID")
        .map(|(_, value)| value.to_string_lossy().into_owned());

    assert_eq!(lead_session.as_deref(), Some("provider-only-session"));
}
