use std::sync::Arc;

use crate::application::managed_team::{ManagedTeamMemberSpec, ManagedTeamService, TeamExitAction};
use crate::application::AgentTaskService;
use crate::domain::entities::{
    ChatConversation, CoordinationMode, ProjectId, TeamMemberStatus, TeamRunBindingStatus,
    TeamSessionStatus,
};
use crate::domain::repositories::{
    AgentTaskRepository, ChatConversationRepository, TeamCoordinationTransitionRepository,
    TeamExitMarker, TeamRepository, TeamRunBindingRepository, UiFeatureFlagOverridesRepository,
};
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryAgentTaskRepository, MemoryChatConversationRepository,
    MemoryQueuedMessageRepository, MemoryTeamCoordinationTransitionRepository,
    MemoryTeamMessageRepository, MemoryTeamRepository, MemoryTeamRunBindingRepository,
    MemoryTeamWakeBatchRepository, MemoryTeamWorkspaceReservationRepository,
    MemoryUiFeatureFlagOverridesRepository,
};
use crate::testing::team_fixtures::{member_run_binding, team_conversation_id};

struct Parts {
    service: ManagedTeamService,
    teams: Arc<MemoryTeamRepository>,
    transitions: Arc<MemoryTeamCoordinationTransitionRepository>,
    conversations: Arc<MemoryChatConversationRepository>,
    bindings: Arc<MemoryTeamRunBindingRepository>,
    tasks: AgentTaskService,
}

async fn parts() -> Parts {
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let teams = Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions)));
    let transitions = Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
        sessions,
    ));
    let conversations = Arc::new(MemoryChatConversationRepository::new());
    let bindings = Arc::new(MemoryTeamRunBindingRepository::new());
    let service = ManagedTeamService::new(
        Arc::clone(&teams) as Arc<_>,
        Arc::clone(&transitions) as Arc<_>,
        Arc::clone(&bindings) as Arc<_>,
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::clone(&conversations) as Arc<_>,
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new())
            as Arc<dyn UiFeatureFlagOverridesRepository>,
    );
    Parts {
        service,
        teams,
        transitions,
        conversations,
        bindings,
        tasks: AgentTaskService::new(
            Arc::new(MemoryAgentTaskRepository::new()) as Arc<dyn AgentTaskRepository>
        ),
    }
}

async fn team(parts: &Parts) -> crate::domain::entities::TeamSession {
    let conversation_id = team_conversation_id(1);
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.id = conversation_id;
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    parts.conversations.create(conversation).await.unwrap();
    parts
        .service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation_id,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn suspend_exit_marks_then_cleans_before_switching_conversation_to_solo() {
    let parts = parts().await;
    let team = team(&parts).await;
    let member = parts
        .service
        .add_member(
            &team.id,
            ManagedTeamMemberSpec {
                name: "Idle member".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "idle test member".to_string(),
                harness: None,
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();

    let exited = parts
        .service
        .exit_team(&parts.tasks, &team.id, TeamExitAction::Suspend)
        .await
        .unwrap();
    assert_eq!(exited.status, TeamSessionStatus::Suspended);
    assert_eq!(exited.pending_exit_action.as_deref(), Some("suspend"));
    assert_eq!(
        parts
            .teams
            .get_member(&member.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TeamMemberStatus::Suspended
    );
    assert_eq!(
        parts
            .conversations
            .get_by_id(&team.coordinator_conversation_id)
            .await
            .unwrap()
            .unwrap()
            .coordination_mode,
        CoordinationMode::Solo
    );
}

#[tokio::test]
async fn drain_exit_cancels_active_binding_and_is_idempotent_after_final_mode_write() {
    let parts = parts().await;
    let team = team(&parts).await;
    let member = parts
        .service
        .add_member(
            &team.id,
            ManagedTeamMemberSpec {
                name: "Busy member".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "busy test member".to_string(),
                harness: None,
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();
    let mut binding = member_run_binding(
        "binding-1",
        team.id.as_str(),
        7,
        member.id.as_str(),
        member.generation,
    );
    binding.status = TeamRunBindingStatus::Running;
    parts.bindings.create(binding.clone()).await.unwrap();
    let mut working = member.clone();
    working.status = TeamMemberStatus::Working;
    working.current_run_id = Some(binding.agent_run_id);
    assert!(parts
        .teams
        .update_member(working, member.generation)
        .await
        .unwrap());

    let exited = parts
        .service
        .exit_team(&parts.tasks, &team.id, TeamExitAction::DrainAndClose)
        .await
        .unwrap();
    assert_eq!(exited.status, TeamSessionStatus::Closed);
    assert_eq!(
        parts
            .bindings
            .get_by_id(&binding.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TeamRunBindingStatus::Cancelled
    );
    assert_eq!(
        parts
            .teams
            .get_member(&member.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TeamMemberStatus::Stopped
    );
    assert_eq!(
        parts
            .service
            .exit_team(&parts.tasks, &team.id, TeamExitAction::DrainAndClose)
            .await
            .unwrap()
            .status,
        TeamSessionStatus::Closed
    );
}

#[tokio::test]
async fn pending_exit_recovery_keeps_team_mode_until_the_final_conversation_write() {
    let parts = parts().await;
    let team = team(&parts).await;
    assert!(parts
        .transitions
        .mark_pending_exit(
            &team.id,
            team.version,
            TeamExitMarker {
                coordination_mode: CoordinationMode::Solo,
                exit_action: "drain_and_close".to_string()
            },
        )
        .await
        .unwrap());
    assert_eq!(
        parts
            .teams
            .get_session(&team.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TeamSessionStatus::Active,
        "a crash before cleanup retains the Team session and its exit marker"
    );
    assert_eq!(
        parts
            .conversations
            .get_by_id(&team.coordinator_conversation_id)
            .await
            .unwrap()
            .unwrap()
            .coordination_mode,
        CoordinationMode::RxNativeTeam
    );
    let mut draining = parts.teams.get_session(&team.id).await.unwrap().unwrap();
    draining
        .transition_to(TeamSessionStatus::Draining, chrono::Utc::now())
        .unwrap();
    draining.version += 1;
    assert!(parts
        .teams
        .update_session(draining.clone(), 1)
        .await
        .unwrap());
    draining
        .transition_to(TeamSessionStatus::Closed, chrono::Utc::now())
        .unwrap();
    draining.version += 1;
    assert!(parts
        .teams
        .update_session(draining.clone(), 2)
        .await
        .unwrap());

    assert_eq!(
        parts
            .conversations
            .get_by_id(&team.coordinator_conversation_id)
            .await
            .unwrap()
            .unwrap()
            .coordination_mode,
        CoordinationMode::RxNativeTeam,
        "a crash after cleanup leaves the durable exit marker but never a false Solo mode"
    );
    assert!(parts
        .transitions
        .commit_exit(
            &team.coordinator_conversation_id,
            &team.id,
            draining.version
        )
        .await
        .unwrap());
    assert_eq!(
        parts.conversations.get_by_id(&team.coordinator_conversation_id).await.unwrap().unwrap().coordination_mode,
        CoordinationMode::RxNativeTeam,
        "a crash after the Team commit still leaves the conversation visibly Team until its final write"
    );
    parts
        .service
        .exit_team(&parts.tasks, &team.id, TeamExitAction::DrainAndClose)
        .await
        .unwrap();
    assert_eq!(
        parts
            .conversations
            .get_by_id(&team.coordinator_conversation_id)
            .await
            .unwrap()
            .unwrap()
            .coordination_mode,
        CoordinationMode::Solo
    );
}
