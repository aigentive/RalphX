use super::*;

fn assert_standalone_codex_security_boundary(spawnable: &SpawnableCommand) {
    let args = spawnable.get_args_for_test();
    assert!(
        args.windows(2).any(|window| {
            (window[0] == "-s" && window[1] == "workspace-write")
                || (window[0] == "-c" && window[1] == "sandbox_mode=\"workspace-write\"")
        }),
        "standalone Codex must use the workspace-write sandbox: {args:?}"
    );
    assert!(
        args.windows(2)
            .any(|window| window[0] == "-c" && window[1] == "approval_policy=\"on-request\""),
        "standalone Codex must fail closed when non-interactive execution needs approval: {args:?}"
    );
    assert!(
        !args.iter().any(|argument| {
            argument == "danger-full-access" || argument == "approval_policy=\"never\""
        }),
        "standalone Codex must not inherit the unrestricted MCP compatibility policy: {args:?}"
    );
    for disabled_override in [
        "features.apply_patch_freeform=false",
        "features.apply_patch_streaming_events=false",
        "include_apply_patch_tool=false",
    ] {
        assert!(
            args.windows(2)
                .any(|window| window[0] == "-c" && window[1] == disabled_override),
            "read-only standalone Codex must disable {disabled_override}: {args:?}"
        );
    }

    let mcp_overrides = args
        .windows(2)
        .filter(|window| window[0] == "-c")
        .map(|window| window[1].as_str())
        .filter(|override_value| override_value.starts_with("mcp_servers."))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        mcp_overrides.contains("--filesystem-enforced")
            && mcp_overrides.contains("--filesystem-read-root"),
        "standalone Codex must retain RalphX MCP root enforcement: {mcp_overrides}"
    );
}

fn assert_codex_mcp_compatibility_security(spawnable: &SpawnableCommand) {
    let args = spawnable.get_args_for_test();
    assert!(
        args.windows(2).any(|window| {
            (window[0] == "-s" && window[1] == "danger-full-access")
                || (window[0] == "-c" && window[1] == "sandbox_mode=\"danger-full-access\"")
        }),
        "non-standalone Codex launches must retain the MCP compatibility sandbox: {args:?}"
    );
    assert!(
        args.windows(2)
            .any(|window| window[0] == "-c" && window[1] == "approval_policy=\"never\""),
        "non-standalone Codex launches must retain the MCP compatibility approval policy: {args:?}"
    );
}

#[tokio::test]
async fn standalone_codex_fresh_launch_is_contained_and_keeps_mcp_root_enforcement() {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = repo_plugin_dir();
    let cli_path = make_fake_codex_cli(&temp);
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    let context_id = conversation.id.as_str();
    let filesystem_read_roots = vec![temp.path().to_path_buf()];
    let resolved_spawn_settings =
        crate::application::agent_lane_resolution::resolve_agent_spawn_settings(
            agent_names::AGENT_GENERAL_EXPLORER,
            None,
            ChatContextType::Standalone,
            None,
            Some(AgentHarnessKind::Codex),
            None,
            None,
        )
        .await;

    let launch_plan = build_launch_plan_for_harness_for_test(
        AgentHarnessKind::Codex,
        &cli_path,
        &plugin_dir,
        &conversation,
        "Inspect this standalone workspace safely with Codex.",
        Some(agent_names::AGENT_GENERAL_EXPLORER),
        None,
        ChatContextType::Standalone,
        &context_id,
        Some(context_id.clone()),
        None,
        temp.path(),
        None,
        None,
        &filesystem_read_roots,
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
    .expect("fresh standalone Codex launch should build");

    let spawnable = launch_spawnable(&launch_plan);
    assert_eq!(
        spawnable.get_args_for_test().first().map(String::as_str),
        Some("exec")
    );
    assert_standalone_codex_security_boundary(spawnable);
}

async fn build_standalone_codex_noninteractive_resume_command(
    temp: &TempDir,
    session_id: &str,
    provider_session_exists: bool,
) -> ProviderSpawnableCommand {
    let plugin_dir = repo_plugin_dir();
    let cli_path = make_fake_codex_cli(temp);
    if provider_session_exists {
        write_test_file(
            &temp.path().join(".codex/session_index.jsonl"),
            &format!(r#"{{"id":"{session_id}"}}"#),
        );
    }
    let _provider_home = EnvGuard::set_os(
        "RALPHX_PROVIDER_STATE_HOME_OVERRIDE",
        temp.path().as_os_str(),
    );
    let conversation = ChatConversation::new_standalone();
    let context_id = conversation.id.as_str();
    let filesystem_read_roots = vec![temp.path().to_path_buf()];

    build_resume_command_for_harness_with_folder_refs(
        AgentHarnessKind::Codex,
        &cli_path,
        &plugin_dir,
        ChatContextType::Standalone,
        &context_id,
        CoordinationMode::Solo,
        &context_id,
        Some(AgentConversationWorkspaceMode::Chat),
        "Continue the queued standalone Codex chat safely.",
        None,
        None,
        Some(agent_names::AGENT_GENERAL_EXPLORER),
        None,
        temp.path(),
        session_id,
        None,
        &filesystem_read_roots,
        None,
        Arc::new(MemoryChatAttachmentRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        None,
        None,
        None,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryDelegatedSessionRepository::new()),
        Arc::new(MemoryTaskRepository::new()),
        &[],
        0,
        None,
        None,
        false,
        None,
        None,
    )
    .await
    .expect("standalone Codex noninteractive resume command should build")
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn standalone_codex_true_resume_enforces_same_security_boundary() {
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let session_id = "standalone-codex-resume-session";
    let command =
        build_standalone_codex_noninteractive_resume_command(&temp, session_id, true).await;
    let args = command.spawnable.get_args_for_test();

    assert_eq!(&args[..3], ["exec", "resume", session_id]);
    assert_standalone_codex_security_boundary(&command.spawnable);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn standalone_codex_restart_recovery_enforces_same_security_boundary() {
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let session_id = "standalone-codex-missing-session";
    let command =
        build_standalone_codex_noninteractive_resume_command(&temp, session_id, false).await;
    let args = command.spawnable.get_args_for_test();

    assert_eq!(args.first().map(String::as_str), Some("exec"));
    assert_ne!(args.get(1).map(String::as_str), Some("resume"));
    assert_standalone_codex_security_boundary(&command.spawnable);
}

#[tokio::test]
async fn project_codex_launch_retains_mcp_compatibility_security() {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = repo_plugin_dir();
    let cli_path = make_fake_codex_cli(&temp);
    let project_id = ProjectId::from_string("codex-security-project".to_string());
    let conversation = ChatConversation::new_project(project_id.clone());
    let resolved_spawn_settings =
        crate::application::agent_lane_resolution::resolve_agent_spawn_settings(
            agent_names::AGENT_GENERAL_WORKER,
            Some(project_id.as_str()),
            ChatContextType::Project,
            None,
            Some(AgentHarnessKind::Codex),
            None,
            None,
        )
        .await;

    let launch_plan = build_launch_plan_for_harness_for_test(
        AgentHarnessKind::Codex,
        &cli_path,
        &plugin_dir,
        &conversation,
        "Continue using the existing Project Codex policy.",
        Some(agent_names::AGENT_GENERAL_WORKER),
        None,
        ChatContextType::Project,
        project_id.as_str(),
        Some(conversation.id.as_str()),
        None,
        temp.path(),
        None,
        Some(project_id.as_str()),
        &[],
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
    .expect("Project Codex launch should build");

    assert_codex_mcp_compatibility_security(launch_spawnable(&launch_plan));
}

#[tokio::test]
async fn standalone_persona_builder_codex_retains_mcp_compatibility_security() {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = repo_plugin_dir();
    let cli_path = make_fake_codex_cli(&temp);
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let context_id = conversation.id.as_str();
    let resolved_spawn_settings =
        crate::application::agent_lane_resolution::resolve_agent_spawn_settings(
            agent_names::AGENT_PERSONA_EXTRACTOR,
            None,
            ChatContextType::Standalone,
            None,
            Some(AgentHarnessKind::Codex),
            None,
            None,
        )
        .await;

    let filesystem_read_roots = vec![temp.path().to_path_buf()];
    let user_prompt = "Build a global persona from bounded source material.";
    let launch_plan = build_launch_plan_for_harness_for_test(
        AgentHarnessKind::Codex,
        &cli_path,
        &plugin_dir,
        &conversation,
        user_prompt,
        Some(agent_names::AGENT_PERSONA_EXTRACTOR),
        None,
        ChatContextType::Standalone,
        &context_id,
        Some(context_id.clone()),
        None,
        temp.path(),
        None,
        None,
        &filesystem_read_roots,
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
    .expect("Standalone PersonaBuilder Codex launch should build");

    let spawnable = launch_spawnable(&launch_plan);
    assert_codex_mcp_compatibility_security(spawnable);
    let args = spawnable.get_args_for_test();

    for expected in [
        "features.shell_tool=false",
        "features.apply_patch_freeform=false",
        "features.apply_patch_streaming_events=false",
        "include_apply_patch_tool=false",
    ] {
        assert!(
            args.windows(2)
                .any(|window| window[0] == "-c" && window[1] == expected),
            "Standalone Codex PersonaBuilder must disable {expected}: {args:?}"
        );
    }

    let mcp_overrides = args
        .windows(2)
        .filter(|window| window[0] == "-c")
        .map(|window| window[1].as_str())
        .filter(|override_value| override_value.starts_with("mcp_servers."))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        mcp_overrides.contains(
            "enabled_tools=[\"fs_read_file\",\"fs_list_dir\",\"fs_grep\",\"fs_glob\",\"ask_user_question\",\"save_persona_draft\",\"get_persona_draft\"]"
        ),
        "Standalone Codex PersonaBuilder must expose exactly its canonical MCP surface: {mcp_overrides}"
    );
    assert!(
        mcp_overrides.contains("--filesystem-enforced")
            && mcp_overrides.contains("--filesystem-read-root")
            && mcp_overrides.contains(temp.path().to_string_lossy().as_ref()),
        "Standalone Codex PersonaBuilder must preserve its exact enforced read root: {mcp_overrides}"
    );

    let composed_prompt = args.last().expect("Codex prompt argument");
    for required in [
        "ralphx-persona-extractor",
        "save_persona_draft",
        "get_persona_draft",
        user_prompt,
    ] {
        assert!(
            composed_prompt.contains(required),
            "Standalone Codex PersonaBuilder prompt must contain {required}: {composed_prompt}"
        );
    }
}
