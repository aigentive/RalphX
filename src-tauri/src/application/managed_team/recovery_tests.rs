use std::sync::Arc;

use async_trait::async_trait;

use crate::application::managed_team::{ManagedTeamService, ManagedTeamStartupBarrier};
use crate::application::AgentTaskService;
use crate::domain::entities::{
    ChatConversationId, CoordinationMode, TeamMember, TeamMemberId, TeamSession, TeamSessionId,
};
use crate::domain::repositories::{
    AgentTaskRepository, TeamRepository, UiFeatureFlagOverridesRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryAgentTaskRepository, MemoryChatConversationRepository,
    MemoryQueuedMessageRepository, MemoryTeamCoordinationTransitionRepository,
    MemoryTeamMessageRepository, MemoryTeamRepository, MemoryTeamRunBindingRepository,
    MemoryTeamWakeBatchRepository, MemoryTeamWorkspaceReservationRepository,
    MemoryUiFeatureFlagOverridesRepository,
};
use crate::testing::team_fixtures::{team_conversation_id, team_session};

struct FailingTeamRepo;

#[async_trait]
impl TeamRepository for FailingTeamRepo {
    async fn ensure_session(&self, _session: TeamSession) -> AppResult<TeamSession> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
    async fn get_session(&self, _id: &TeamSessionId) -> AppResult<Option<TeamSession>> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
    async fn get_open_session_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<TeamSession>> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
    async fn list_open_sessions(&self) -> AppResult<Vec<TeamSession>> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
    async fn update_session(
        &self,
        _session: TeamSession,
        _expected_version: i64,
    ) -> AppResult<bool> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
    async fn create_member(&self, _member: TeamMember) -> AppResult<TeamMember> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
    async fn get_member(&self, _id: &TeamMemberId) -> AppResult<Option<TeamMember>> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
    async fn list_members(&self, _team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
    async fn update_member(
        &self,
        _member: TeamMember,
        _expected_generation: i64,
    ) -> AppResult<bool> {
        Err(AppError::Database("team storage unavailable".to_string()))
    }
}

#[tokio::test]
async fn test_barrier_fences_team_conversations_before_running() {
    let barrier = ManagedTeamStartupBarrier::new();

    assert!(
        barrier
            .should_fence_resumption(CoordinationMode::RxNativeTeam, &team_conversation_id(1))
            .await,
        "barrier that has not run must fence Team conversations"
    );
    assert!(
        !barrier
            .should_fence_resumption(CoordinationMode::Solo, &team_conversation_id(1))
            .await,
        "non-Team conversations are never fenced"
    );
}

#[tokio::test]
async fn test_barrier_failure_keeps_team_conversations_fenced() {
    let barrier = ManagedTeamStartupBarrier::new();
    let repo: Arc<dyn TeamRepository> = Arc::new(FailingTeamRepo);
    barrier.run(&repo).await;

    assert!(
        barrier
            .should_fence_resumption(CoordinationMode::RxNativeTeam, &team_conversation_id(1))
            .await,
        "failed barrier must keep Team conversations fenced"
    );
    assert!(
        !barrier
            .should_fence_resumption(CoordinationMode::Solo, &team_conversation_id(1))
            .await
    );
}

#[tokio::test]
async fn test_ready_barrier_fences_only_open_team_conversations() {
    let barrier = ManagedTeamStartupBarrier::new();
    let repo = Arc::new(MemoryTeamRepository::new());
    repo.ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    let repo: Arc<dyn TeamRepository> = repo;
    barrier.run(&repo).await;

    assert!(
        barrier
            .should_fence_resumption(CoordinationMode::RxNativeTeam, &team_conversation_id(1))
            .await,
        "conversation with an open team session stays fenced until full recovery lands"
    );
    assert!(
        !barrier
            .should_fence_resumption(CoordinationMode::RxNativeTeam, &team_conversation_id(2))
            .await,
        "Team conversation without an open session may resume normally"
    );
    assert!(
        !barrier
            .should_fence_resumption(CoordinationMode::Solo, &team_conversation_id(1))
            .await
    );
}

#[tokio::test]
async fn pending_exit_recovery_propagates_session_scan_errors() {
    let service = ManagedTeamService::new(
        Arc::new(FailingTeamRepo),
        Arc::new(MemoryTeamCoordinationTransitionRepository::new()),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::new(MemoryChatConversationRepository::new()),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new())
            as Arc<dyn UiFeatureFlagOverridesRepository>,
    );
    let task_service = AgentTaskService::new(
        Arc::new(MemoryAgentTaskRepository::new()) as Arc<dyn AgentTaskRepository>
    );

    assert!(matches!(
        service.recover_pending_exits(&task_service).await,
        Err(AppError::Database(_))
    ));
}
