use std::sync::Arc;

use crate::application::managed_team::{
    ManagedTeamMemberUsage, ManagedTeamService, ManagedTeamUsage,
};
use crate::domain::entities::{
    AgentRun, AgentRunUsage, ChatConversation, ProjectId, TeamBudgetPolicy,
};
use crate::domain::repositories::{
    AgentRunRepository, ChatConversationRepository, TeamRepository,
    UiFeatureFlagOverridesRepository,
};
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryChatConversationRepository, MemoryQueuedMessageRepository,
    MemoryTeamCoordinationTransitionRepository, MemoryTeamMessageRepository, MemoryTeamRepository,
    MemoryTeamRunBindingRepository, MemoryTeamWakeBatchRepository,
    MemoryTeamWorkspaceReservationRepository, MemoryUiFeatureFlagOverridesRepository,
};
use crate::testing::team_fixtures::{team_agent_run_id, team_conversation_id};

fn service() -> (
    ManagedTeamService,
    Arc<MemoryAgentRunRepository>,
    Arc<MemoryTeamRepository>,
    Arc<MemoryChatConversationRepository>,
) {
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let teams = Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions)));
    let runs = Arc::new(MemoryAgentRunRepository::new());
    let conversations = Arc::new(MemoryChatConversationRepository::new());
    let service = ManagedTeamService::new(
        Arc::clone(&teams) as Arc<_>,
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::clone(&conversations) as Arc<dyn ChatConversationRepository>,
        Arc::clone(&runs) as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new())
            as Arc<dyn UiFeatureFlagOverridesRepository>,
    );
    (service, runs, teams, conversations)
}

#[tokio::test]
async fn team_usage_is_derived_from_run_bindings_and_agent_runs() {
    let (service, runs, teams, conversations) = service();
    let conversation = team_conversation_id(1);
    let mut coordinator =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    coordinator.id = conversation;
    conversations.create(coordinator).await.unwrap();
    let team = service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
        )
        .await
        .unwrap();
    let mut run = AgentRun::new(conversation);
    run.id = team_agent_run_id(1);
    run.harness = Some(crate::domain::agents::AgentHarnessKind::Codex);
    runs.create(run).await.unwrap();
    runs.update_usage(
        &team_agent_run_id(1),
        &AgentRunUsage {
            input_tokens: Some(9),
            output_tokens: Some(3),
            estimated_usd: Some(0.000_002),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    service
        .run_binding_repo()
        .create(
            crate::application::managed_team::new_coordinator_run_binding(
                team.id.clone(),
                conversation,
                team_agent_run_id(1),
            ),
        )
        .await
        .unwrap();

    assert_eq!(
        service.team_usage(&team.id).await.unwrap(),
        ManagedTeamUsage {
            tokens: 12,
            cost_micros: 2,
            members: vec![ManagedTeamMemberUsage {
                member_id: None,
                tokens: 12,
                cost_micros: 2,
            }]
        }
    );
    let mut limited = teams.get_session(&team.id).await.unwrap().unwrap();
    limited.budget_policy = Some(TeamBudgetPolicy {
        token_limit: Some(12),
        cost_limit_micros: None,
    });
    limited.version += 1;
    assert!(teams.update_session(limited, 0).await.unwrap());
    assert!(service
        .admit_dispatch(&team.id)
        .await
        .unwrap_err()
        .to_string()
        .contains("token budget"));
}
