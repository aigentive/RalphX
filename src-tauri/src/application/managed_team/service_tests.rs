use std::sync::Arc;

use async_trait::async_trait;
use ralphx_events::{EventSink, RecordingEventSink};

use crate::application::managed_team::ManagedTeamService;
use crate::domain::entities::{ChatConversation, ProjectId, UiFeatureFlagOverrides};
use crate::domain::repositories::{
    ChatConversationRepository, TeamRunBindingRepository, UiFeatureFlagOverridesRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryChatConversationRepository, MemoryQueuedMessageRepository,
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
    conversations: Arc<MemoryChatConversationRepository>,
}

fn build_service() -> ServiceParts {
    let run_binding_repo = Arc::new(MemoryTeamRunBindingRepository::new());
    let overrides_repo = Arc::new(MemoryUiFeatureFlagOverridesRepository::new());
    let conversations = Arc::new(MemoryChatConversationRepository::new());
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let service = ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::clone(&run_binding_repo) as Arc<dyn TeamRunBindingRepository>,
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::clone(&conversations) as Arc<dyn ChatConversationRepository>,
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::clone(&overrides_repo) as Arc<dyn UiFeatureFlagOverridesRepository>,
    );
    ServiceParts {
        service,
        run_binding_repo,
        overrides_repo,
        conversations,
    }
}

async fn create_project_conversation(
    conversations: &MemoryChatConversationRepository,
    project_id: ProjectId,
    conversation_id: crate::domain::entities::ChatConversationId,
) {
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.id = conversation_id;
    conversations.create(conversation).await.unwrap();
}

fn build_service_with_failing_overrides() -> ManagedTeamService {
    ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::new()),
        Arc::new(MemoryTeamCoordinationTransitionRepository::new()),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::new(MemoryChatConversationRepository::new()),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(FailingOverridesRepo),
    )
}

fn build_service_with_event_sink(sink: Arc<dyn EventSink>) -> ServiceParts {
    let run_binding_repo = Arc::new(MemoryTeamRunBindingRepository::new());
    let overrides_repo = Arc::new(MemoryUiFeatureFlagOverridesRepository::new());
    let conversations = Arc::new(MemoryChatConversationRepository::new());
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let service = ManagedTeamService::new_with_event_sink(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::clone(&run_binding_repo) as Arc<dyn TeamRunBindingRepository>,
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::clone(&conversations) as Arc<dyn ChatConversationRepository>,
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::clone(&overrides_repo) as Arc<dyn UiFeatureFlagOverridesRepository>,
        sink,
    );
    ServiceParts {
        service,
        run_binding_repo,
        overrides_repo,
        conversations,
    }
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
    create_project_conversation(
        &parts.conversations,
        ProjectId::from_string("project-1".to_string()),
        conversation,
    )
    .await;

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
async fn test_ensure_team_rejects_unknown_coordinator_conversation() {
    let parts = build_service();

    let result = parts
        .service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &team_conversation_id(99),
        )
        .await;

    assert!(
        matches!(result, Err(AppError::NotFound(_))),
        "unknown coordinator conversations must not create durable Teams"
    );
}

#[tokio::test]
async fn test_ensure_team_rejects_cross_project_coordinator_conversation() {
    let parts = build_service();
    let conversation = team_conversation_id(1);
    create_project_conversation(
        &parts.conversations,
        ProjectId::from_string("project-a".to_string()),
        conversation,
    )
    .await;

    let result = parts
        .service
        .ensure_team(
            ProjectId::from_string("project-b".to_string()),
            &conversation,
        )
        .await;

    assert!(
        matches!(result, Err(AppError::Conflict(_))),
        "a coordinator conversation must remain scoped to its owning project"
    );
}

#[tokio::test]
async fn member_transitions_emit_frontend_roster_payload_after_the_write_succeeds() {
    let sink = RecordingEventSink::new();
    let parts = build_service_with_event_sink(Arc::new(sink.clone()));
    let conversation = team_conversation_id(1);
    create_project_conversation(
        &parts.conversations,
        ProjectId::from_string("project-1".to_string()),
        conversation,
    )
    .await;
    let team = parts
        .service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
        )
        .await
        .unwrap();

    let member = parts
        .service
        .add_member(
            &team.id,
            crate::application::managed_team::ManagedTeamMemberSpec {
                name: "Event member".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "event test member".to_string(),
                harness: None,
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        sink.events(),
        vec![ralphx_events::RecordedEvent {
            event: "team:member_updated".to_string(),
            payload: serde_json::json!({
                "conversation_id": conversation.as_str(),
                "member": {
                    "id": member.id.as_str(),
                    "teamId": member.team_id.as_str(),
                    "name": "Event member",
                    "normalizedName": "event member",
                    "canonicalAgentName": "ralphx-general-worker",
                    "roleSummary": "event test member",
                    "status": "idle",
                    "generation": 0,
                }
            }),
        }]
    );
}

#[tokio::test]
async fn failed_member_cas_does_not_emit_a_member_updated_event() {
    let sink = RecordingEventSink::new();
    let parts = build_service_with_event_sink(Arc::new(sink.clone()));
    let conversation = team_conversation_id(1);
    create_project_conversation(
        &parts.conversations,
        ProjectId::from_string("project-1".to_string()),
        conversation,
    )
    .await;
    let team = parts
        .service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
        )
        .await
        .unwrap();
    let member = parts
        .service
        .add_member(
            &team.id,
            crate::application::managed_team::ManagedTeamMemberSpec {
                name: "CAS member".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "cas test member".to_string(),
                harness: None,
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();
    let recorded_after_create = sink.events();

    assert!(matches!(
        parts
            .service
            .stop_member(&team.id, &member.normalized_name, member.generation + 1)
            .await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(sink.events(), recorded_after_create);
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
    create_project_conversation(
        &parts.conversations,
        ProjectId::from_string("project-1".to_string()),
        conversation,
    )
    .await;

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
async fn preallocate_coordinator_run_binding_is_idempotent_for_wake_runs() {
    // A wake-driven coordinator send preallocates the run binding twice for the
    // same run id: once when `claim_next_actionable_wake_batch` creates the
    // member-null wake binding, and again when the coordinator send itself
    // calls `preallocate_coordinator_run_binding`. The second call must return
    // the original binding unchanged rather than erroring or creating a
    // duplicate that would make `get_by_agent_run_id` ambiguous.
    let parts = build_service();
    enable_team(&parts.overrides_repo).await;
    let conversation = team_conversation_id(1);
    create_project_conversation(
        &parts.conversations,
        ProjectId::from_string("project-1".to_string()),
        conversation,
    )
    .await;

    let first = parts
        .service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
            &team_agent_run_id(1),
        )
        .await
        .unwrap()
        .expect("enabled capability must record a binding");
    let second = parts
        .service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-1".to_string()),
            &conversation,
            &team_agent_run_id(1),
        )
        .await
        .unwrap()
        .expect("idempotent replay must return the existing binding");

    assert_eq!(first.id, second.id, "one agent run must not bind twice");
    let bindings = parts
        .run_binding_repo
        .list_for_team(&first.team_id)
        .await
        .unwrap();
    assert_eq!(
        bindings.len(),
        1,
        "idempotent replay must not create a second binding for the same run id"
    );
}

#[tokio::test]
async fn preallocate_rejects_cross_team_binding_for_same_run() {
    let parts = build_service();
    enable_team(&parts.overrides_repo).await;
    let conversation_a = team_conversation_id(1);
    let conversation_b = team_conversation_id(2);
    create_project_conversation(
        &parts.conversations,
        ProjectId::from_string("project-1".to_string()),
        conversation_a,
    )
    .await;
    create_project_conversation(
        &parts.conversations,
        ProjectId::from_string("project-2".to_string()),
        conversation_b,
    )
    .await;

    parts
        .service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-1".to_string()),
            &conversation_a,
            &team_agent_run_id(1),
        )
        .await
        .unwrap();

    let result = parts
        .service
        .preallocate_coordinator_run_binding(
            ProjectId::from_string("project-2".to_string()),
            &conversation_b,
            &team_agent_run_id(1),
        )
        .await;
    assert!(
        matches!(result, Err(AppError::Conflict(_))),
        "a run id already bound to a different Team/conversation must never be silently adopted"
    );
}
