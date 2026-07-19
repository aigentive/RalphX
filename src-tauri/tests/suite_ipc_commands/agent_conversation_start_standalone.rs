use super::agent_conversation_start_support::*;

// ── Standalone (projectless) start arm — Phase 4a.3 ──────────────────────────

#[tokio::test]
async fn start_agent_conversation_standalone_flag_off_rejected() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(false));
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));

    let error = start_with_app(
        &app,
        standalone_start_input("hi", Some("chat"), None, None, None),
    )
    .await
    .expect_err("standalone start must be rejected while the flag is off");
    assert!(
        error.contains("standalone_conversations"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn personas_on_standalone_off_rejects_global_builder_at_start() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(false));
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));

    let error = start_with_app(
        &app,
        standalone_start_input(
            "Global builder must remain unavailable",
            Some("persona_builder"),
            None,
            None,
            None,
        ),
    )
    .await
    .expect_err("global builder must reject when standalone conversations are disabled");

    assert!(
        error.contains("standalone_conversations"),
        "unexpected error: {error}"
    );
    assert!(app
        .state::<AppState>()
        .chat_conversation_repo
        .list_by_context_type(ChatContextType::Standalone, true, 10)
        .await
        .expect("standalone conversation lookup should succeed")
        .is_empty());
}

#[tokio::test]
async fn start_agent_conversation_standalone_non_chat_mode_rejected() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));

    let error = start_with_app(
        &app,
        standalone_start_input("hi", Some("edit"), None, None, None),
    )
    .await
    .expect_err("non-chat modes must be rejected for standalone in this phase");
    assert!(error.contains("chat"), "unexpected error: {error}");
}

#[tokio::test]
async fn start_agent_conversation_standalone_absent_mode_rejected() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));

    // No mode supplied: for Project starts this silently defaults to "edit"
    // (parse_agent_workspace_mode). Standalone must NOT inherit that default —
    // an absent mode must be typed-rejected, not resolved to a non-chat mode.
    let error = start_with_app(&app, standalone_start_input("hi", None, None, None, None))
        .await
        .expect_err("an absent mode must not silently resolve to a non-chat mode for standalone");
    assert!(error.contains("chat"), "unexpected error: {error}");
}

#[tokio::test]
async fn start_agent_conversation_standalone_team_intent_rejected() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));

    let team_intent = TeamIntent {
        coordination_mode: CoordinationMode::RxNativeTeam,
        strategy: None,
    };
    let error = start_with_app(
        &app,
        standalone_start_input("hi", Some("chat"), None, Some(team_intent), None),
    )
    .await
    .expect_err("Team mode must be rejected for standalone conversations");
    assert!(error.contains("Team"), "unexpected error: {error}");
}

#[tokio::test]
async fn start_agent_conversation_standalone_solo_team_intent_is_allowed() {
    // Regression guard: the standalone Team rejection must only fire for a
    // genuinely non-solo intent, not for an explicit solo intent (which some
    // callers send even when no Team behavior is requested).
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));

    let team_intent = TeamIntent {
        coordination_mode: CoordinationMode::Solo,
        strategy: None,
    };
    let result = start_with_app(
        &app,
        standalone_start_input("hi", Some("chat"), None, Some(team_intent), None),
    )
    .await
    .expect("a solo team intent must not be rejected as Team mode");
    assert_eq!(
        result.conversation.context_type,
        ChatContextType::Standalone
    );
}

#[tokio::test]
async fn start_agent_conversation_standalone_parent_conversation_id_rejected() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));

    let error = start_with_app(
        &app,
        standalone_start_input("hi", Some("chat"), None, None, Some("some-parent-id")),
    )
    .await
    .expect_err("parent_conversation_id must be rejected for standalone starts");
    assert!(
        error.contains("parent_conversation_id"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn start_agent_conversation_standalone_chat_mode_creates_self_keyed_conversation_and_resolves_workspace_cwd(
) {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));

    let result = start_with_app(
        &app,
        standalone_start_input("Quick standalone question", Some("chat"), None, None, None),
    )
    .await
    .expect("standalone chat start should succeed");

    assert_eq!(
        result.conversation.context_type,
        ChatContextType::Standalone
    );
    assert_eq!(
        result.conversation.context_id,
        result.conversation.id.as_str()
    );
    assert!(
        result.workspace.is_none(),
        "chat mode never creates an AgentConversationWorkspace, standalone included"
    );

    // Proves the 4a.2 private workspace (ensure_workspace) is actually reached
    // and created DURING start() via the live send path — not merely resolvable
    // in isolation.
    let app_data_dir = app
        .state::<AppState>()
        .app_paths
        .app_data_dir()
        .to_path_buf();
    let root = standalone_workspaces_root(&app_data_dir);
    let expected_path = standalone_workspace_path(&root, &result.conversation.id.as_str());
    assert!(
        expected_path.join("manifest.json").exists(),
        "the private standalone workspace must be created on disk during start(): {:?}",
        expected_path
    );

    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&result.conversation.id)
        .await
        .expect("stored conversation should load")
        .expect("stored conversation should exist");
    assert!(stored.is_valid_standalone_self_key());
}

#[tokio::test]
async fn ipc_contract_standalone_global_codex_role_defaults_are_validated_and_launched() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    let fake_codex = FakeCodex::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_codex
            .cli_path
            .to_str()
            .expect("fake Codex path should be UTF-8"),
    );

    let state = AppState::new_test();
    for role in [RoutingRole::WorkspaceChat, RoutingRole::UtilityLightweight] {
        state
            .manual_role_default_repo
            .upsert_global(role, &manual_role_default(AgentHarnessKind::Codex))
            .await
            .expect("global Codex role default should persist");
    }

    configure_provider_cli(
        &state,
        AgentHarnessKind::Claude,
        "/definitely/missing/ralphx-test-claude",
    )
    .await;
    configure_provider_cli(
        &state,
        AgentHarnessKind::Codex,
        fake_codex.cli_path.to_string_lossy().into_owned(),
    )
    .await;

    let app = build_app(state, Arc::new(ExecutionState::new()));
    for mode in ["chat", "persona_builder"] {
        let result = start_with_app(
            &app,
            standalone_start_input(
                &format!("Start standalone {mode} from its global Codex role default"),
                Some(mode),
                None,
                None,
                None,
            ),
        )
        .await
        .unwrap_or_else(|error| {
            panic!("standalone {mode} must validate and launch its Codex role default: {error}")
        });
        let runs = app
            .state::<AppState>()
            .agent_run_repo
            .get_by_conversation(&result.conversation.id)
            .await
            .expect("standalone role-default run lookup should succeed");
        assert!(
            runs.iter()
                .any(|run| run.harness == Some(AgentHarnessKind::Codex)),
            "standalone {mode} must persist the global Codex role default",
        );
    }
    assert!(
        fake_codex.was_invoked(),
        "standalone global Codex role defaults must invoke Codex",
    );
}

#[tokio::test]
async fn ipc_contract_start_agent_conversation_standalone_chat_accepts_codex() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let fake_codex = FakeCodex::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_codex
            .cli_path
            .to_str()
            .expect("fake Codex path should be UTF-8"),
    );
    let state = AppState::new_test();
    state
        .manual_role_default_repo
        .upsert_global(
            RoutingRole::WorkspaceChat,
            &manual_role_default(AgentHarnessKind::Claude),
        )
        .await
        .expect("global Claude chat default should persist");
    let app = build_app(state, Arc::new(ExecutionState::new()));
    let mut input = standalone_start_input(
        "Start a projectless Codex conversation",
        Some("chat"),
        None,
        None,
        None,
    );
    input.provider_harness = Some("codex".to_string());

    let result = start_with_app(&app, input)
        .await
        .expect("standalone Codex start should be accepted");

    assert_eq!(
        result.conversation.context_type,
        ChatContextType::Standalone
    );
    let runs = app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&result.conversation.id)
        .await
        .expect("standalone Codex start run lookup should succeed");
    assert!(
        runs.iter()
            .any(|run| run.harness == Some(AgentHarnessKind::Codex)),
        "start must preserve the explicit Codex harness selection"
    );
    assert!(
        fake_codex.was_invoked(),
        "Standalone Chat start must invoke Codex"
    );
}

#[tokio::test]
async fn ipc_contract_standalone_seeded_ownership_accepts_valid_self_keyed_draft() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let state = app.state::<AppState>();
    let seeded = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .expect("seeded standalone draft should persist");

    let result = start_with_app(
        &app,
        standalone_start_input(
            "Continue from the draft",
            Some("chat"),
            Some(&seeded.id),
            None,
            None,
        ),
    )
    .await
    .expect("a valid self-keyed standalone seed must be accepted");

    assert_eq!(result.conversation.id, seeded.id);
}

#[tokio::test]
async fn start_agent_conversation_standalone_seeded_ownership_rejects_team_coordination() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let state = app.state::<AppState>();
    let mut seed = ChatConversation::new_standalone();
    seed.set_coordination_mode(CoordinationMode::RxNativeTeam);
    let seeded = state
        .chat_conversation_repo
        .create(seed)
        .await
        .expect("corrupt team standalone seed should persist for the ownership regression");

    let error = start_with_app(
        &app,
        standalone_start_input(
            "Reject corrupt standalone seed",
            Some("chat"),
            Some(&seeded.id),
            None,
            None,
        ),
    )
    .await
    .expect_err("team-coordination standalone seed must be rejected");

    assert!(
        error.contains("valid standalone seed"),
        "unexpected error: {error}"
    );
    let stored = state
        .chat_conversation_repo
        .get_by_id(&seeded.id)
        .await
        .expect("seed lookup should succeed")
        .expect("rejected seed should remain persisted");
    assert_eq!(stored.coordination_mode, CoordinationMode::RxNativeTeam);
}

#[tokio::test]
async fn start_agent_conversation_standalone_context_id_mismatch_cannot_be_seeded() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let state = app.state::<AppState>();
    let mut corrupted = ChatConversation::new_standalone();
    corrupted.context_id = "not-my-own-id".to_string();
    let error = state
        .chat_conversation_repo
        .create(corrupted)
        .await
        .expect_err("repository must reject a standalone row whose context_id != id");
    assert!(error.to_string().contains("context_id"));
}

#[tokio::test]
async fn ipc_contract_standalone_seeded_ownership_rejects_wrong_context_type() {
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let state = app.state::<AppState>();
    let project_id = ProjectId::from_string("project-standalone-ownership-mismatch".to_string());
    let project_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id))
        .await
        .expect("project conversation should persist");

    let error = start_with_app(
        &app,
        standalone_start_input(
            "Should not be accepted",
            Some("chat"),
            Some(&project_conversation.id),
            None,
            None,
        ),
    )
    .await
    .expect_err("a Project-context conversation must be rejected as a standalone seed");
    assert!(
        error.contains("not a valid standalone seed"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn ipc_contract_standalone_seeded_ownership_rejects_when_project_id_is_set() {
    // D3.6: valid iff context_type == Standalone && context_id == id &&
    // input.project_id == None. Supplying a project_id alongside a standalone
    // seed must be rejected (it routes into the Project ownership branch,
    // which also rejects since the seed's context_type is Standalone, not
    // Project — still a correct rejection of the invalid combination).
    let _reset = StandaloneConversationsFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let state = app.state::<AppState>();
    let seeded = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .expect("seeded standalone draft should persist");
    let project_id = ProjectId::from_string("project-standalone-project-id-set".to_string());
    let mut project = Project::new(
        "Standalone project_id set".to_string(),
        "/tmp/project-standalone-project-id-set".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");

    let mut input = standalone_start_input(
        "Should not be accepted",
        Some("chat"),
        Some(&seeded.id),
        None,
        None,
    );
    input.project_id = Some(project_id.as_str().to_string());

    start_with_app(&app, input)
        .await
        .expect_err("a standalone seed must be rejected when project_id is also supplied");
}
