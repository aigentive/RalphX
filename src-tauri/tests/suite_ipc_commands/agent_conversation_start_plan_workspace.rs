use super::agent_conversation_start_support::*;

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
        authoring_state_json: None,
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
    let starter = Arc::new(AgentConversationAutomationRunStarter::new(
        app_state.clone(),
        Arc::clone(&execution_state),
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
