use std::sync::Arc;

use async_trait::async_trait;

use crate::application::managed_team::ManagedTeamService;
use crate::domain::entities::{ProjectId, UiFeatureFlagOverrides};
use crate::domain::repositories::{TeamRunBindingRepository, UiFeatureFlagOverridesRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryTeamCoordinationTransitionRepository, MemoryTeamMessageRepository, MemoryTeamRepository,
    MemoryTeamRunBindingRepository, MemoryTeamWakeBatchRepository,
    MemoryTeamWorkspaceReservationRepository, MemoryUiFeatureFlagOverridesRepository,
};
use crate::testing::team_fixtures::{team_agent_run_id, team_conversation_id};

struct FailingOverridesRepo;

#[async_trait]
impl UiFeatureFlagOverridesRepository for FailingOverridesRepo {
    async fn get(&self) -> AppResult<UiFeatureFlagOverrides> {
        Err(AppError::Database("overrides unavailable".to_string()))
    }
    async fn set_agent_personas(&self, _value: Option<bool>) -> AppResult<()> {
        Err(AppError::Database("overrides unavailable".to_string()))
    }
    async fn update_agent_capabilities(
        &self,
        _team: Option<bool>,
        _workflows: Option<bool>,
        _autopilot: Option<bool>,
    ) -> AppResult<UiFeatureFlagOverrides> {
        Err(AppError::Database("overrides unavailable".to_string()))
    }
}

struct ServiceParts {
    service: ManagedTeamService,
    run_binding_repo: Arc<MemoryTeamRunBindingRepository>,
    overrides_repo: Arc<MemoryUiFeatureFlagOverridesRepository>,
}

fn build_service() -> ServiceParts {
    let run_binding_repo = Arc::new(MemoryTeamRunBindingRepository::new());
    let overrides_repo = Arc::new(MemoryUiFeatureFlagOverridesRepository::new());
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let service = ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::clone(&run_binding_repo) as Arc<dyn TeamRunBindingRepository>,
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::clone(&overrides_repo) as Arc<dyn UiFeatureFlagOverridesRepository>,
    );
    ServiceParts {
        service,
        run_binding_repo,
        overrides_repo,
    }
}

fn build_service_with_failing_overrides() -> ManagedTeamService {
    ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::new()),
        Arc::new(MemoryTeamCoordinationTransitionRepository::new()),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(FailingOverridesRepo),
    )
}

async fn enable_team(overrides_repo: &MemoryUiFeatureFlagOverridesRepository) {
    overrides_repo
        .update_agent_capabilities(Some(true), None, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_ensure_team_is_idempotent_per_conversation() {
    let parts = build_service();
    let conversation = team_conversation_id(1);

    let first = parts
        .service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
        )
        .await
        .unwrap();
    let second = parts
        .service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
        )
        .await
        .unwrap();
    assert_eq!(first.id, second.id);

    let status = parts
        .service
        .team_status(&conversation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.session.id, first.id);
    assert!(status.members.is_empty());
}

#[tokio::test]
async fn test_team_status_none_without_open_team() {
    let parts = build_service();
    assert!(parts
        .service
        .team_status(&team_conversation_id(7))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_preallocate_skips_when_capability_disabled() {
    let parts = build_service();
    let conversation = team_conversation_id(1);

    let binding = parts
        .service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
            &team_agent_run_id(1),
        )
        .await
        .unwrap();
    assert!(binding.is_none(), "disabled capability must not bind runs");
    assert!(
        parts
            .service
            .team_status(&conversation)
            .await
            .unwrap()
            .is_none(),
        "disabled capability must not create a team"
    );
}

#[tokio::test]
async fn test_preallocate_creates_team_and_member_null_binding() {
    let parts = build_service();
    enable_team(&parts.overrides_repo).await;
    let conversation = team_conversation_id(1);

    let binding = parts
        .service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
            &team_agent_run_id(1),
        )
        .await
        .unwrap()
        .expect("enabled capability must record a binding");

    assert!(binding.team_member_id.is_none());
    assert!(binding.team_member_generation.is_none());
    assert_eq!(binding.agent_run_id, team_agent_run_id(1));

    let status = parts
        .service
        .team_status(&conversation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.session.id, binding.team_id);

    // A second coordinator send reuses the same team with a distinct run.
    let second = parts
        .service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
            &team_agent_run_id(2),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.team_id, binding.team_id);
    assert_ne!(second.agent_run_id, binding.agent_run_id);

    let bindings = parts
        .run_binding_repo
        .list_for_team(&binding.team_id)
        .await
        .unwrap();
    assert_eq!(bindings.len(), 2);
}

#[tokio::test]
async fn test_preallocate_fails_closed_on_override_read_error() {
    let service = build_service_with_failing_overrides();

    let result = service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-1".to_string()),
            &team_conversation_id(1),
            &team_agent_run_id(1),
        )
        .await;
    assert!(
        result.is_err(),
        "override read errors must fail the send, not silently skip Team binding"
    );
}

#[tokio::test]
async fn test_duplicate_run_binding_rejected() {
    let parts = build_service();
    enable_team(&parts.overrides_repo).await;
    let conversation = team_conversation_id(1);

    parts
        .service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
            &team_agent_run_id(1),
        )
        .await
        .unwrap();
    assert!(
        parts
            .service
            .preallocate_coordinator_run_binding(
                ProjectId::from_string("project-1".to_string()),
                &conversation,
                &team_agent_run_id(1),
            )
            .await
            .is_err(),
        "one agent run must not bind twice"
    );
}
