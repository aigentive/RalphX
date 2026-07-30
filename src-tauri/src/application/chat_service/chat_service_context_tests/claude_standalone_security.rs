use super::*;

fn assert_standalone_claude_permission_boundary(spawnable: &SpawnableCommand) {
    let args = spawnable.get_args_for_test();
    let permission_mode = args
        .windows(2)
        .find_map(|window| (window[0] == "--permission-mode").then(|| window[1].as_str()))
        .expect("standalone Claude launch should declare a permission mode");
    assert_eq!(
        permission_mode, CLAUDE_PROMPT_PERMISSION_MODE,
        "standalone Claude must prompt at the native out-of-root permission boundary"
    );
    assert!(
        !args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--dangerously-skip-permissions" | "--allow-dangerously-skip-permissions"
            )
        }),
        "standalone Claude must not inherit global permission-bypass flags: {args:?}"
    );

    let native_tools = args
        .windows(2)
        .find_map(|window| (window[0] == "--tools").then(|| window[1].as_str()))
        .expect("standalone Claude should retain its bounded native tool surface");
    for required in ["Read", "Grep", "Glob"] {
        assert!(
            native_tools.split(',').any(|tool| tool == required),
            "standalone Claude should retain {required}: {native_tools}"
        );
    }

    let preapproved_tools = args
        .windows(2)
        .find_map(|window| (window[0] == "--allowedTools").then(|| window[1].as_str()))
        .expect("standalone Claude should retain MCP preapprovals");
    let preapproved = preapproved_tools.split(',').collect::<Vec<_>>();
    for prompt_gated in ["Read", "Grep", "Glob"] {
        assert!(
            !preapproved.contains(&prompt_gated),
            "standalone Claude must not preapprove native {prompt_gated}: {preapproved_tools}"
        );
    }
    assert!(
        preapproved
            .iter()
            .any(|tool| tool.contains("permission_request")),
        "standalone Claude must retain the permission bridge preapproval: {preapproved_tools}"
    );
    assert!(
        preapproved.iter().any(|tool| tool.starts_with("mcp__")),
        "standalone Claude must retain MCP preapprovals: {preapproved_tools}"
    );
}

fn claude_permission_projection(
    spawnable: &SpawnableCommand,
) -> (Option<String>, bool, bool, Option<String>) {
    let args = spawnable.get_args_for_test();
    let value_after = |flag: &str| {
        args.windows(2)
            .find_map(|window| (window[0] == flag).then(|| window[1].clone()))
    };
    (
        value_after("--permission-mode"),
        args.iter()
            .any(|arg| arg == "--dangerously-skip-permissions"),
        args.iter()
            .any(|arg| arg == "--allow-dangerously-skip-permissions"),
        value_after("--allowedTools"),
    )
}

#[test]
fn standalone_claude_project_and_persona_builder_keep_configured_permission_behavior() {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = repo_plugin_dir();
    let cli_path = make_fake_claude_cli(&temp);

    for (context_type, effective_mode, agent) in [
        (
            ChatContextType::Project,
            AgentConversationWorkspaceMode::Chat,
            agent_names::AGENT_GENERAL_EXPLORER,
        ),
        (
            ChatContextType::Standalone,
            AgentConversationWorkspaceMode::PersonaBuilder,
            agent_names::AGENT_PERSONA_EXTRACTOR,
        ),
    ] {
        let configured = build_spawnable_interactive_command_for_test(
            &cli_path,
            &plugin_dir,
            "Inspect the workspace.",
            Some(agent),
            None,
            temp.path(),
            false,
            None,
            None,
        )
        .expect("configured Claude command should build");
        let scoped = build_claude_spawnable_interactive_command(
            &cli_path,
            &plugin_dir,
            "Inspect the workspace.",
            Some(agent),
            None,
            None,
            None,
            temp.path(),
            false,
            None,
            None,
            None,
            false,
            context_type,
            Some(effective_mode),
        )
        .expect("context-scoped Claude command should build");

        assert_eq!(
            claude_permission_projection(&scoped),
            claude_permission_projection(&configured),
            "{context_type:?}/{effective_mode:?} must retain its configured Claude permissions"
        );
    }
}

#[tokio::test]
async fn standalone_claude_fresh_interactive_launch_enforces_permission_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = repo_plugin_dir();
    let cli_path = make_fake_claude_cli(&temp);
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    let context_id = conversation.id.as_str();
    let resolved_spawn_settings =
        crate::application::agent_lane_resolution::resolve_agent_spawn_settings(
            agent_names::AGENT_GENERAL_EXPLORER,
            None,
            ChatContextType::Standalone,
            None,
            Some(AgentHarnessKind::Claude),
            None,
            None,
        )
        .await;

    let launch_plan = build_launch_plan_for_harness_for_test(
        AgentHarnessKind::Claude,
        &cli_path,
        &plugin_dir,
        &conversation,
        "Inspect this standalone workspace safely.",
        Some(agent_names::AGENT_GENERAL_EXPLORER),
        None,
        ChatContextType::Standalone,
        &context_id,
        Some(context_id.clone()),
        None,
        temp.path(),
        None,
        None,
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
    .expect("fresh standalone Claude launch should build");

    assert_standalone_claude_permission_boundary(launch_spawnable(&launch_plan));
}

async fn build_standalone_claude_noninteractive_resume_command(
    temp: &TempDir,
    session_id: &str,
    provider_session_exists: bool,
) -> ProviderSpawnableCommand {
    let plugin_dir = repo_plugin_dir();
    let cli_path = make_fake_claude_cli(temp);
    if provider_session_exists {
        let session_file = temp
            .path()
            .join(".claude/projects/standalone")
            .join(format!("{session_id}.jsonl"));
        write_test_file(&session_file, "{}\n");
    }
    let _provider_home = EnvGuard::set_os(
        "RALPHX_PROVIDER_STATE_HOME_OVERRIDE",
        temp.path().as_os_str(),
    );
    let conversation = ChatConversation::new_standalone();
    let context_id = conversation.id.as_str();

    build_resume_command_for_harness_with_folder_refs(
        AgentHarnessKind::Claude,
        &cli_path,
        &plugin_dir,
        ChatContextType::Standalone,
        &context_id,
        CoordinationMode::Solo,
        &context_id,
        Some(AgentConversationWorkspaceMode::Chat),
        None,
        "Continue the queued standalone chat safely.",
        None,
        None,
        Some(agent_names::AGENT_GENERAL_EXPLORER),
        None,
        temp.path(),
        session_id,
        None,
        &[],
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
    )
    .await
    .expect("standalone Claude noninteractive resume command should build")
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn standalone_claude_queued_resume_enforces_same_permission_boundary() {
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let session_id = "standalone-queued-resume-session";
    let command =
        build_standalone_claude_noninteractive_resume_command(&temp, session_id, true).await;

    assert!(
        command
            .spawnable
            .get_args_for_test()
            .windows(2)
            .any(|window| window == ["--resume", session_id]),
        "fixture should exercise the queue's true provider-resume path"
    );
    assert_standalone_claude_permission_boundary(&command.spawnable);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn standalone_claude_restart_recovery_enforces_same_permission_boundary() {
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let session_id = "standalone-missing-provider-session";
    let command =
        build_standalone_claude_noninteractive_resume_command(&temp, session_id, false).await;

    assert!(
        !command
            .spawnable
            .get_args_for_test()
            .iter()
            .any(|argument| argument == "--resume"),
        "missing provider state must exercise ProviderResumeMode::Recovery"
    );
    assert_standalone_claude_permission_boundary(&command.spawnable);
}
