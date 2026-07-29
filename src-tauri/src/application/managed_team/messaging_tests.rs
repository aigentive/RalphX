use std::sync::Arc;

use async_trait::async_trait;

use crate::application::managed_team::{
    ManagedTeamMemberSpec, ManagedTeamMessageRequest, ManagedTeamMessageSender,
    ManagedTeamMessageTarget, ManagedTeamService,
};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRun, AgentRunId, ChatConversation, CoordinationMode, DelegatedSessionId, ProjectId,
    TeamMessageKind,
};
use crate::domain::repositories::{
    AgentRunRepository, ChatConversationRepository, QueuedMessageRepository, TeamMessageRepository,
    TeamRepository, TeamRunBindingRepository, TeamWakeBatchRepository,
    UiFeatureFlagOverridesRepository,
};
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryChatConversationRepository, MemoryQueuedMessageRepository,
    MemoryTeamCoordinationTransitionRepository, MemoryTeamMessageRepository, MemoryTeamRepository,
    MemoryTeamRunBindingRepository, MemoryTeamWakeBatchRepository,
    MemoryTeamWorkspaceReservationRepository, MemoryUiFeatureFlagOverridesRepository,
};
use crate::testing::team_fixtures::{team_agent_run_id, team_conversation_id};

struct Parts {
    service: ManagedTeamService,
    team_repo: Arc<MemoryTeamRepository>,
    run_repo: Arc<MemoryAgentRunRepository>,
    binding_repo: Arc<MemoryTeamRunBindingRepository>,
    message_repo: Arc<MemoryTeamMessageRepository>,
    wake_repo: Arc<MemoryTeamWakeBatchRepository>,
    queue_repo: Arc<MemoryQueuedMessageRepository>,
    conversation_repo: Arc<MemoryChatConversationRepository>,
}

struct FailingQueuedMessageRepository;

#[async_trait]
impl QueuedMessageRepository for FailingQueuedMessageRepository {
    async fn enqueue_back(&self, _key: &QueueKey, _message: &QueuedMessage) -> AppResult<()> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }

    async fn enqueue_front(&self, _key: &QueueKey, _message: &QueuedMessage) -> AppResult<()> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }

    async fn list(&self, _key: &QueueKey) -> AppResult<Vec<QueuedMessage>> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }

    async fn list_keys(&self) -> AppResult<Vec<QueueKey>> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }

    async fn delete(&self, _key: &QueueKey, _message_id: &str) -> AppResult<bool> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }

    async fn delete_by_id(&self, _message_id: &str) -> AppResult<bool> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }

    async fn clear(&self, _key: &QueueKey) -> AppResult<()> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }

    async fn pop_front(&self, _key: &QueueKey) -> AppResult<Option<QueuedMessage>> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }

    async fn remove_stale(
        &self,
        _key: &QueueKey,
        _threshold_secs: u64,
    ) -> AppResult<Vec<QueuedMessage>> {
        Err(AppError::Infrastructure(
            "injected queue projection failure".to_string(),
        ))
    }
}

fn build_parts() -> Parts {
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let team_repo = Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions)));
    let message_repo = Arc::new(MemoryTeamMessageRepository::new());
    let wake_repo = Arc::new(MemoryTeamWakeBatchRepository::new());
    let queue_repo = Arc::new(MemoryQueuedMessageRepository::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let binding_repo = Arc::new(MemoryTeamRunBindingRepository::new());
    let service = ManagedTeamService::new(
        Arc::clone(&team_repo) as Arc<_>,
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::clone(&binding_repo) as Arc<_>,
        Arc::clone(&message_repo) as Arc<dyn TeamMessageRepository>,
        Arc::clone(&wake_repo) as Arc<dyn TeamWakeBatchRepository>,
        Arc::clone(&queue_repo) as Arc<dyn QueuedMessageRepository>,
        Arc::clone(&conversation_repo) as Arc<dyn ChatConversationRepository>,
        Arc::clone(&run_repo) as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new())
            as Arc<dyn UiFeatureFlagOverridesRepository>,
    );
    Parts {
        service,
        team_repo,
        run_repo,
        binding_repo,
        message_repo,
        wake_repo,
        queue_repo,
        conversation_repo,
    }
}

fn service_with_queue(
    parts: &Parts,
    queue_repo: Arc<dyn QueuedMessageRepository>,
) -> ManagedTeamService {
    ManagedTeamService::new(
        Arc::clone(&parts.team_repo) as Arc<dyn ralphx_domain::repositories::TeamRepository>,
        Arc::new(MemoryTeamCoordinationTransitionRepository::new()),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::clone(&parts.message_repo)
            as Arc<dyn ralphx_domain::repositories::TeamMessageRepository>,
        Arc::clone(&parts.wake_repo)
            as Arc<dyn ralphx_domain::repositories::TeamWakeBatchRepository>,
        queue_repo,
        Arc::clone(&parts.conversation_repo)
            as Arc<dyn ralphx_domain::repositories::ChatConversationRepository>,
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new()),
    )
}

async fn ready_team(parts: &Parts) -> crate::domain::entities::TeamSession {
    let conversation_id = team_conversation_id(1);
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.id = conversation_id;
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    parts.conversation_repo.create(conversation).await.unwrap();
    let team = parts
        .service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation_id,
        )
        .await
        .unwrap();
    parts
        .service
        .startup_barrier()
        .run(&parts.service.team_repo())
        .await;
    parts
        .service
        .release_delivery_projection_after_recovery()
        .await
        .unwrap();
    team
}

async fn add_delivery_member(
    parts: &Parts,
    team: &crate::domain::entities::TeamSession,
    name: &str,
) -> crate::domain::entities::TeamMember {
    let member = parts
        .service
        .add_member(
            &team.id,
            ManagedTeamMemberSpec {
                name: name.to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "test recipient".to_string(),
                harness: None,
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();
    let delegated_session_id = DelegatedSessionId::new();
    let conversation = ChatConversation::new_delegation(delegated_session_id.clone());
    parts.conversation_repo.create(conversation).await.unwrap();
    let mut bound = member.clone();
    bound.delegated_session_id = Some(delegated_session_id);
    assert!(parts
        .team_repo
        .update_member(bound.clone(), member.generation)
        .await
        .unwrap());
    bound
}

fn coordinator_request(
    team_id: crate::domain::entities::TeamSessionId,
    key: &str,
    target: ManagedTeamMessageTarget,
) -> ManagedTeamMessageRequest {
    ManagedTeamMessageRequest {
        team_id,
        sender: ManagedTeamMessageSender::Coordinator {
            conversation_id: team_conversation_id(1),
            source_run_id: None,
        },
        target,
        kind: TeamMessageKind::Instruction,
        content: "inspect <this>".to_string(),
        idempotency_key: key.to_string(),
    }
}

#[tokio::test]
async fn idle_member_delivery_projects_once_with_typed_prompt() {
    let parts = build_parts();
    let team = ready_team(&parts).await;
    let member = add_delivery_member(&parts, &team, "Writer One").await;

    let (message, deliveries) = parts
        .service
        .send_team_message(coordinator_request(
            team.id,
            "composer-run-1",
            ManagedTeamMessageTarget::MemberName(member.normalized_name.clone()),
        ))
        .await
        .unwrap();

    assert_eq!(deliveries.len(), 1);
    let key = QueueKey::new(
        crate::domain::entities::ChatContextType::Delegation,
        member.delegated_session_id.unwrap().as_str(),
    );
    let queued = parts.queue_repo.list(&key).await.unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].id, deliveries[0].id.0);
    assert!(queued[0].content.contains("<team_message"));
    assert!(queued[0].content.contains("&lt;this&gt;"));
    assert_eq!(message.sequence, 1);
}

#[tokio::test]
async fn mixed_claude_codex_leads_and_members_share_the_same_message_service_path() {
    for (lead_harness, member_harness, run_index) in [
        (AgentHarnessKind::Claude, AgentHarnessKind::Codex, 31),
        (AgentHarnessKind::Codex, AgentHarnessKind::Claude, 32),
    ] {
        let parts = build_parts();
        let team = ready_team(&parts).await;
        let member = add_delivery_member(&parts, &team, "Mixed member").await;
        let mut provider_member = member.clone();
        provider_member.harness = Some(member_harness);
        assert!(parts
            .team_repo
            .update_member(provider_member.clone(), member.generation)
            .await
            .unwrap());
        let mut lead = AgentRun::new(team_conversation_id(1));
        lead.id = team_agent_run_id(run_index);
        lead.harness = Some(lead_harness);
        parts.run_repo.create(lead.clone()).await.unwrap();
        parts
            .binding_repo
            .create(
                crate::application::managed_team::new_coordinator_run_binding(
                    team.id.clone(),
                    team_conversation_id(1),
                    lead.id,
                ),
            )
            .await
            .unwrap();
        let request = ManagedTeamMessageRequest {
            team_id: team.id,
            sender: ManagedTeamMessageSender::Coordinator {
                conversation_id: team_conversation_id(1),
                source_run_id: Some(lead.id),
            },
            target: ManagedTeamMessageTarget::MemberName(provider_member.normalized_name),
            kind: TeamMessageKind::Instruction,
            content: "mixed-provider delivery".to_string(),
            idempotency_key: format!("mixed-{run_index}"),
        };
        let (message, deliveries) = parts.service.send_team_message(request).await.unwrap();
        assert_eq!(
            message.sender_kind,
            crate::domain::entities::TeamMessageActorKind::Coordinator
        );
        assert_eq!(deliveries.len(), 1);
    }
}

#[tokio::test]
async fn duplicate_idempotency_key_replays_envelope_without_duplicate_delivery() {
    let parts = build_parts();
    let team = ready_team(&parts).await;
    let member = add_delivery_member(&parts, &team, "Writer One").await;
    let request = coordinator_request(
        team.id,
        "composer-run-replay",
        ManagedTeamMessageTarget::MemberName(member.normalized_name.clone()),
    );

    let first = parts
        .service
        .send_team_message(request.clone())
        .await
        .unwrap();
    let second = parts.service.send_team_message(request).await.unwrap();

    assert_eq!(first.0.id, second.0.id);
    assert_eq!(first.1[0].id, second.1[0].id);
    let key = QueueKey::new(
        crate::domain::entities::ChatContextType::Delegation,
        member.delegated_session_id.unwrap().as_str(),
    );
    assert_eq!(parts.queue_repo.list(&key).await.unwrap().len(), 1);
}

#[tokio::test]
async fn queue_projection_failure_keeps_delivery_replayable_without_duplicate_envelope() {
    let parts = build_parts();
    let team = ready_team(&parts).await;
    let member = add_delivery_member(&parts, &team, "Writer One").await;
    let request = coordinator_request(
        team.id.clone(),
        "composer-retry-after-queue-failure",
        ManagedTeamMessageTarget::MemberName(member.normalized_name.clone()),
    );
    let failing = service_with_queue(&parts, Arc::new(FailingQueuedMessageRepository));
    failing.startup_barrier().run(&failing.team_repo()).await;
    failing
        .release_delivery_projection_after_recovery()
        .await
        .unwrap();

    let error = failing
        .send_team_message(request.clone())
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Infrastructure(_)));
    let stored = parts
        .message_repo
        .get_envelope_by_idempotency_key(&team.id, &request.idempotency_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.1[0].status,
        crate::domain::entities::TeamMessageDeliveryStatus::Failed
    );

    let recovered = service_with_queue(
        &parts,
        Arc::clone(&parts.queue_repo) as Arc<dyn QueuedMessageRepository>,
    );
    recovered
        .startup_barrier()
        .run(&recovered.team_repo())
        .await;
    recovered
        .release_delivery_projection_after_recovery()
        .await
        .unwrap();
    let replayed = recovered.send_team_message(request).await.unwrap();

    assert_eq!(replayed.0.id, stored.0.id);
    assert_eq!(replayed.1.len(), 1);
    let key = QueueKey::new(
        crate::domain::entities::ChatContextType::Delegation,
        member.delegated_session_id.unwrap().as_str(),
    );
    assert_eq!(parts.queue_repo.list(&key).await.unwrap().len(), 1);
}

#[tokio::test]
async fn broadcast_resolves_current_generation_recipients_as_one_envelope() {
    let parts = build_parts();
    let team = ready_team(&parts).await;
    add_delivery_member(&parts, &team, "Writer One").await;
    add_delivery_member(&parts, &team, "Reviewer Two").await;

    let (message, deliveries) = parts
        .service
        .send_team_message(coordinator_request(
            team.id.clone(),
            "broadcast-1",
            ManagedTeamMessageTarget::Broadcast,
        ))
        .await
        .unwrap();

    assert_eq!(deliveries.len(), 2);
    let stored = parts
        .message_repo
        .get_envelope_by_idempotency_key(&team.id, "broadcast-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.0.id, message.id);
    assert_eq!(stored.1.len(), 2);
}

#[tokio::test]
async fn cross_team_coordinator_spoof_is_rejected_before_envelope_write() {
    let parts = build_parts();
    let _team = ready_team(&parts).await;
    let other_conversation_id = team_conversation_id(2);
    let mut other_conversation =
        ChatConversation::new_project(ProjectId::from_string("project-2".to_string()));
    other_conversation.id = other_conversation_id;
    parts
        .conversation_repo
        .create(other_conversation)
        .await
        .unwrap();
    let other = parts
        .service
        .ensure_team(
            ProjectId::from_string("project-2".to_string()),
            &other_conversation_id,
        )
        .await
        .unwrap();

    let result = parts
        .service
        .send_team_message(coordinator_request(
            other.id.clone(),
            "cross-team",
            ManagedTeamMessageTarget::Broadcast,
        ))
        .await;

    assert!(matches!(result, Err(AppError::Conflict(_))));
    assert!(parts
        .message_repo
        .list_messages_after(&other.id, 0, 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn coordinator_wakes_coalesce_sequence_range_when_idle() {
    let parts = build_parts();
    let team = ready_team(&parts).await;
    let member = add_delivery_member(&parts, &team, "Writer One").await;
    let source_run_id = team_agent_run_id(7);
    let mut sending = member.clone();
    sending.current_run_id = Some(source_run_id.clone());
    sending.status = crate::domain::entities::TeamMemberStatus::Working;
    assert!(parts
        .team_repo
        .update_member(sending.clone(), member.generation)
        .await
        .unwrap());
    let binding = crate::application::managed_team::lifecycle::new_member_assignment_run_binding(
        &sending,
        sending.delegated_session_id.clone().unwrap(),
        team_conversation_id(3),
        source_run_id.clone(),
        crate::domain::entities::AgentTaskAssignmentId::new(),
        crate::domain::entities::TeamWorkClassification::ReadOnly,
    );
    parts
        .service
        .run_binding_repo()
        .create(binding)
        .await
        .unwrap();

    for key in ["wake-1", "wake-2"] {
        parts
            .service
            .send_team_message(ManagedTeamMessageRequest {
                team_id: team.id.clone(),
                sender: ManagedTeamMessageSender::Member {
                    member_id: sending.id.clone(),
                    generation: sending.generation,
                    source_run_id: source_run_id.clone(),
                },
                target: ManagedTeamMessageTarget::Coordinator,
                kind: TeamMessageKind::Result,
                content: key.to_string(),
                idempotency_key: key.to_string(),
            })
            .await
            .unwrap();
    }

    let batches = parts
        .wake_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].first_message_sequence, 1);
    assert_eq!(batches[0].last_message_sequence, 2);
    assert_eq!(batches[0].delivery_ids.len(), 2);
}

#[tokio::test]
async fn stale_member_generation_rejects_message_authority() {
    let parts = build_parts();
    let team = ready_team(&parts).await;
    let member = add_delivery_member(&parts, &team, "Writer One").await;
    let result = parts
        .service
        .send_team_message(ManagedTeamMessageRequest {
            team_id: team.id,
            sender: ManagedTeamMessageSender::Member {
                member_id: member.id,
                generation: member.generation + 1,
                source_run_id: AgentRunId::new(),
            },
            target: ManagedTeamMessageTarget::Coordinator,
            kind: TeamMessageKind::Result,
            content: "late".to_string(),
            idempotency_key: "late-1".to_string(),
        })
        .await;
    assert!(matches!(result, Err(AppError::Conflict(_))));
}
