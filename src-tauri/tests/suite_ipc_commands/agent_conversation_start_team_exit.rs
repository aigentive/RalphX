use super::agent_conversation_start_support::*;
use ralphx_lib::application::managed_team::ManagedTeamService;
use ralphx_lib::infrastructure::memory::{
    MemoryQueuedMessageRepository, MemoryTeamCoordinationTransitionRepository,
    MemoryTeamMessageRepository, MemoryTeamRepository, MemoryTeamRunBindingRepository,
    MemoryTeamWakeBatchRepository, MemoryTeamWorkspaceReservationRepository,
};

#[tokio::test]
async fn restarting_team_conversation_with_solo_intent_performs_staged_exit() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut state = AppState::new_sqlite_test();
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
    let project = seed_project(
        &state,
        "project-start-service-team-exit",
        &repo_path,
        &worktree_parent,
    )
    .await;
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

    start_with_app(&app, input)
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
