use super::agent_conversation_start_support::*;
use ralphx_lib::application::managed_team::ManagedTeamService;
use ralphx_lib::infrastructure::memory::{
    MemoryQueuedMessageRepository, MemoryTeamCoordinationTransitionRepository,
    MemoryTeamMessageRepository, MemoryTeamRepository, MemoryTeamRunBindingRepository,
    MemoryTeamWakeBatchRepository, MemoryTeamWorkspaceReservationRepository,
};

fn share_managed_team_repositories(state: &mut AppState) {
    let sessions = MemoryTeamRepository::new_shared_sessions();
    state.managed_team = Arc::new(ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::clone(&state.ui_feature_flag_overrides_repo),
    ));
}

async fn team_start_fixture(
    project_id: &str,
) -> (
    tempfile::TempDir,
    tauri::App<tauri::test::MockRuntime>,
    Project,
    ChatConversation,
    ralphx_lib::domain::entities::TeamSession,
) {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut state = AppState::new_sqlite_test();
    share_managed_team_repositories(&mut state);
    let project = seed_project(&state, project_id, &repo_path, &worktree_parent).await;
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Edit);
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("Team conversation should persist");
    let session = state
        .managed_team
        .ensure_team(project.id.clone(), &conversation.id)
        .await
        .expect("open Team session should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);
    (temp, app, project, conversation, session)
}

fn solo_restart_input(
    project: &Project,
    conversation: &ChatConversation,
) -> StartAgentConversationInput {
    let mut input = service_start_input(
        &project.id,
        "Restart as Solo",
        "edit",
        Some("main"),
        Some("isolated"),
        Some(&conversation.id),
        None,
    );
    input.team_intent = Some(TeamIntent {
        coordination_mode: CoordinationMode::Solo,
        strategy: None,
    });
    input
}

#[tokio::test]
async fn restarting_team_conversation_with_solo_intent_performs_staged_exit() {
    let (_temp, app, project, conversation, session) =
        team_start_fixture("project-start-service-team-exit").await;

    start_with_app(&app, solo_restart_input(&project, &conversation))
        .await
        .expect("Solo restart should queue after staged Team exit");

    let state = app.state::<AppState>();
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation query should succeed")
        .expect("conversation should remain persisted");
    assert_eq!(stored.coordination_mode, CoordinationMode::Solo);
    let exited = state
        .managed_team
        .team_repo()
        .get_session(&session.id)
        .await
        .expect("Team session query should succeed")
        .expect("Team session should remain as durable history");
    assert_eq!(
        exited.status,
        ralphx_lib::domain::entities::TeamSessionStatus::Closed
    );
    assert_eq!(
        exited.pending_exit_action.as_deref(),
        Some("drain_and_close")
    );
}

#[tokio::test]
async fn corrupt_team_exit_marker_blocks_solo_restart_before_mode_write() {
    let (_temp, app, project, conversation, session) =
        team_start_fixture("project-start-service-corrupt-team-exit").await;
    let state = app.state::<AppState>();
    let mut corrupt = session.clone();
    corrupt.pending_exit_action = Some("corrupt".to_string());
    state
        .managed_team
        .team_repo()
        .update_session(corrupt, session.version)
        .await
        .expect("corrupt marker fixture should persist");

    let error = start_with_app(&app, solo_restart_input(&project, &conversation))
        .await
        .expect_err("corrupt durable exit state must block the restart");

    assert!(
        error.contains("must be suspend or drain_and_close"),
        "unexpected error: {error}"
    );
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation query should succeed")
        .expect("conversation should remain persisted");
    assert_eq!(stored.coordination_mode, CoordinationMode::RxNativeTeam);
    assert!(state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation.id.as_str())
        .is_empty());
}
