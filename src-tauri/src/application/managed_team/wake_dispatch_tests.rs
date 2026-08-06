use std::sync::Arc;

use ralphx_events::RecordingEventSink;

use crate::application::chat_service::{ChatService, MockChatService};
use crate::application::managed_team::wake_dispatch::ManagedTeamWakeDispatcher;
use crate::application::managed_team::{
    ManagedTeamMemberSpec, ManagedTeamMessageRequest, ManagedTeamMessageSender,
    ManagedTeamMessageTarget, ManagedTeamService,
};
use crate::domain::entities::{
    AgentRun, AgentTaskAssignmentId, ChatContextType, ChatConversation, CoordinationMode,
    ProjectId, TeamMessageDeliveryStatus, TeamMessageKind, TeamRunBindingStatus,
    TeamRunTriggerKind, TeamSession, TeamWakeBatchStatus,
};
use crate::domain::repositories::{
    AgentRunRepository, ChatConversationRepository, QueuedMessageRepository, TeamMessageRepository,
    TeamRunBindingRepository, TeamWakeBatchRepository, UiFeatureFlagOverridesRepository,
};
use crate::domain::services::QueueKey;
use crate::infrastructure::agents::claude::delegation_config;
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryChatConversationRepository, MemoryQueuedMessageRepository,
    MemoryTeamCoordinationTransitionRepository, MemoryTeamMessageRepository, MemoryTeamRepository,
    MemoryTeamRunBindingRepository, MemoryTeamWakeBatchRepository,
    MemoryTeamWorkspaceReservationRepository, MemoryUiFeatureFlagOverridesRepository,
};
use crate::testing::team_fixtures::team_conversation_id;

struct Harness {
    managed_team: Arc<ManagedTeamService>,
    run_binding_repo: Arc<MemoryTeamRunBindingRepository>,
    wake_batch_repo: Arc<MemoryTeamWakeBatchRepository>,
    agent_runs: Arc<MemoryAgentRunRepository>,
    conversations: Arc<MemoryChatConversationRepository>,
    queued_messages: Arc<MemoryQueuedMessageRepository>,
    overrides_repo: Arc<MemoryUiFeatureFlagOverridesRepository>,
    chat: Arc<MockChatService>,
    events: Arc<RecordingEventSink>,
}

impl Harness {
    fn dispatcher(&self) -> ManagedTeamWakeDispatcher {
        ManagedTeamWakeDispatcher::new(
            Arc::clone(&self.managed_team),
            Arc::clone(&self.chat) as Arc<dyn ChatService>,
            Arc::clone(&self.agent_runs) as Arc<dyn AgentRunRepository>,
            Arc::clone(&self.conversations) as Arc<dyn ChatConversationRepository>,
            Arc::clone(&self.events) as Arc<dyn ralphx_events::EventSink>,
        )
    }
}

fn build_harness() -> Harness {
    let run_binding_repo = Arc::new(MemoryTeamRunBindingRepository::new());
    let wake_batch_repo = Arc::new(MemoryTeamWakeBatchRepository::new());
    let message_repo = Arc::new(MemoryTeamMessageRepository::new());
    let conversations = Arc::new(MemoryChatConversationRepository::new());
    let agent_runs = Arc::new(MemoryAgentRunRepository::new());
    let queued_messages = Arc::new(MemoryQueuedMessageRepository::new());
    let overrides_repo = Arc::new(MemoryUiFeatureFlagOverridesRepository::new());
    let events = Arc::new(RecordingEventSink::new());
    let chat = Arc::new(MockChatService::with_agent_run_repo(
        Arc::clone(&agent_runs) as Arc<dyn AgentRunRepository>,
    ));
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let managed_team = Arc::new(ManagedTeamService::new_with_event_sink(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::clone(&run_binding_repo) as Arc<dyn TeamRunBindingRepository>,
        Arc::clone(&message_repo) as Arc<dyn TeamMessageRepository>,
        Arc::clone(&wake_batch_repo) as Arc<dyn TeamWakeBatchRepository>,
        Arc::clone(&queued_messages)
            as Arc<dyn crate::domain::repositories::QueuedMessageRepository>,
        Arc::clone(&conversations) as Arc<dyn ChatConversationRepository>,
        Arc::clone(&agent_runs) as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::clone(&overrides_repo) as Arc<dyn UiFeatureFlagOverridesRepository>,
        Arc::clone(&events) as Arc<dyn ralphx_events::EventSink>,
    ));
    Harness {
        managed_team,
        run_binding_repo,
        wake_batch_repo,
        agent_runs,
        conversations,
        queued_messages,
        overrides_repo,
        chat,
        events,
    }
}

/// Enables the Team capability, ensures a Team for a fresh coordinator
/// conversation, and releases the delivery-projection startup barrier so
/// `claim_next_actionable_wake_batch` can run.
async fn ready_team(harness: &Harness, conversation_index: u64) -> TeamSession {
    harness
        .overrides_repo
        .update_agent_capabilities(Some(true), None, None)
        .await
        .unwrap();
    let conversation_id = team_conversation_id(conversation_index);
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.id = conversation_id;
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    harness.conversations.create(conversation).await.unwrap();
    let team = harness
        .managed_team
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation_id,
        )
        .await
        .unwrap();
    harness
        .managed_team
        .startup_barrier()
        .run(&harness.managed_team.team_repo())
        .await;
    harness
        .managed_team
        .release_delivery_projection_after_recovery()
        .await
        .unwrap();
    team
}

/// Queues exactly one coordinator wake batch the same way production does:
/// a `System`-sender settlement message targeting the coordinator.
async fn queue_one_coordinator_wake(harness: &Harness, team: &TeamSession) {
    let member = harness
        .managed_team
        .add_member(
            &team.id,
            ManagedTeamMemberSpec {
                name: "Wake source member".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "test member".to_string(),
                harness: None,
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();
    let assignment_id = AgentTaskAssignmentId("assignment-1".to_string());
    let mut binding = crate::testing::team_fixtures::member_run_binding(
        "binding-source",
        team.id.as_str(),
        900,
        member.id.as_str(),
        member.generation,
    );
    binding.assignment_id = Some(assignment_id.clone());
    binding.conversation_id = team.coordinator_conversation_id;
    harness.run_binding_repo.create(binding).await.unwrap();

    harness
        .managed_team
        .send_team_message(ManagedTeamMessageRequest {
            team_id: team.id.clone(),
            sender: ManagedTeamMessageSender::System { assignment_id },
            target: ManagedTeamMessageTarget::Coordinator,
            kind: TeamMessageKind::Result,
            content: "assignment settled".to_string(),
            idempotency_key: "settle-1".to_string(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn lost_claim_cas_is_a_no_op() {
    let harness = build_harness();
    let team = ready_team(&harness, 1).await;
    queue_one_coordinator_wake(&harness, &team).await;

    // Simulate a concurrent dispatcher winning the claim first.
    let claimed = harness
        .managed_team
        .claim_next_actionable_wake_batch(&team.id)
        .await
        .unwrap();
    assert!(
        claimed.is_some(),
        "the batch must be claimable exactly once"
    );

    // No queued batches remain, so this dispatcher's claim is a no-op.
    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    assert!(harness.chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn active_coordinator_run_suppresses_dispatch() {
    let harness = build_harness();
    let team = ready_team(&harness, 2).await;
    queue_one_coordinator_wake(&harness, &team).await;
    let queued = harness
        .wake_batch_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    let batch_id = queued[0].id.clone();

    let mut active_run = AgentRun::new(team.coordinator_conversation_id);
    active_run.id = crate::domain::entities::AgentRunId::new();
    harness.agent_runs.create(active_run).await.unwrap();

    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    assert!(
        harness.chat.get_sent_messages().await.is_empty(),
        "an already-active coordinator run must suppress dispatch"
    );
    let batches = harness
        .wake_batch_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    assert!(
        batches.is_empty(),
        "the batch was claimed (Queued -> Launching) before suppression was detected"
    );
    let resolved = harness
        .wake_batch_repo
        .get_by_id(&batch_id)
        .await
        .unwrap()
        .expect("the claimed batch must still exist");
    assert_eq!(
        resolved.status,
        TeamWakeBatchStatus::Cancelled,
        "a suppressed batch must not stay stuck at Launching forever"
    );
    let run_bindings = harness
        .run_binding_repo
        .list_for_team(&team.id)
        .await
        .unwrap();
    let wake_binding = run_bindings
        .iter()
        .find(|binding| {
            binding.trigger_kind == crate::domain::entities::TeamRunTriggerKind::WakeBatch
        })
        .expect("claim must record a coordinator wake run binding");
    assert_eq!(wake_binding.status, TeamRunBindingStatus::Cancelled);
}

#[tokio::test]
async fn successful_dispatch_settles_the_batch_exactly_once() {
    let harness = build_harness();
    let team = ready_team(&harness, 3).await;
    queue_one_coordinator_wake(&harness, &team).await;
    let queued = harness
        .wake_batch_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    assert_eq!(
        queued.len(),
        1,
        "exactly one batch must be queued before dispatch"
    );
    let batch_id = queued[0].id.clone();

    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    let sent = harness.chat.get_sent_messages().await;
    assert_eq!(
        sent.len(),
        1,
        "exactly one hidden resume message must be sent"
    );
    assert!(sent[0].contains("<team_message"));

    let options = harness.chat.get_sent_options().await;
    let metadata: serde_json::Value =
        serde_json::from_str(options[0].metadata.as_ref().unwrap()).unwrap();
    assert_eq!(metadata["ralphx_action_kind"], "managed_team_wake");
    assert_eq!(metadata["hidden_from_ui"], true);

    let settled = harness
        .wake_batch_repo
        .get_by_id(&batch_id)
        .await
        .unwrap()
        .expect("the claimed batch must still exist");
    assert_eq!(settled.status, TeamWakeBatchStatus::Settled);

    let run_bindings = harness
        .run_binding_repo
        .list_for_team(&team.id)
        .await
        .unwrap();
    assert!(
        run_bindings.iter().any(|binding| {
            binding.trigger_kind == crate::domain::entities::TeamRunTriggerKind::WakeBatch
        }),
        "claim must record a coordinator wake run binding"
    );

    // Re-dispatching must find nothing queued: the batch settled exactly once.
    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();
    assert_eq!(
        harness.chat.get_sent_messages().await.len(),
        1,
        "a settled batch must never be dispatched twice"
    );
}

#[tokio::test(start_paused = true)]
async fn send_failure_retries_then_durably_fails_with_event() {
    let harness = build_harness();
    let team = ready_team(&harness, 4).await;
    queue_one_coordinator_wake(&harness, &team).await;
    harness.chat.set_available(false).await;

    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    let retry_max = delegation_config().park_wake_retry_max.max(1);
    assert_eq!(
        harness.chat.call_count(),
        retry_max,
        "must retry exactly the configured bounded number of attempts"
    );
    let attention_events: Vec<_> = harness
        .events
        .events()
        .into_iter()
        .filter(|recorded| recorded.event == "team:needs_attention")
        .collect();
    assert_eq!(
        attention_events.len(),
        1,
        "a durable failure must emit exactly one needs_attention event"
    );

    let run_bindings = harness
        .run_binding_repo
        .list_for_team(&team.id)
        .await
        .unwrap();
    let wake_binding = run_bindings
        .iter()
        .find(|binding| {
            binding.trigger_kind == crate::domain::entities::TeamRunTriggerKind::WakeBatch
        })
        .expect("claim must record a coordinator wake run binding");
    assert_eq!(
        wake_binding.status,
        TeamRunBindingStatus::Failed,
        "the wake-triggered run binding must fail too, so it stops consuming wake budget"
    );
}

#[tokio::test]
async fn startup_drain_dispatches_batches_left_queued_by_a_crash() {
    let harness = build_harness();
    let team = ready_team(&harness, 6).await;
    // A batch that was queued but never dispatched: the process died between
    // `schedule_coordinator_wakes` and the settlement-path dispatch, so no
    // further settlement will ever re-trigger it.
    queue_one_coordinator_wake(&harness, &team).await;
    assert_eq!(
        harness
            .wake_batch_repo
            .list_queued_for_team(&team.id, 10)
            .await
            .unwrap()
            .len(),
        1,
        "the crash leaves exactly one orphaned queued batch"
    );

    let drained = harness.dispatcher().drain_open_team_wakes().await.unwrap();

    assert_eq!(drained, 1, "startup recovery must claim the orphaned batch");
    assert_eq!(
        harness.chat.get_sent_messages().await.len(),
        1,
        "the orphaned batch must produce exactly one coordinator turn"
    );
    assert!(
        harness
            .wake_batch_repo
            .list_queued_for_team(&team.id, 10)
            .await
            .unwrap()
            .is_empty(),
        "no batch may remain queued after the drain"
    );
}

#[tokio::test]
async fn startup_drain_is_a_no_op_without_queued_batches() {
    let harness = build_harness();
    let team = ready_team(&harness, 7).await;

    let drained = harness.dispatcher().drain_open_team_wakes().await.unwrap();

    assert_eq!(drained, 0);
    assert!(harness.chat.get_sent_messages().await.is_empty());
    assert!(harness
        .run_binding_repo
        .list_for_team(&team.id)
        .await
        .unwrap()
        .iter()
        .all(|binding| {
            binding.trigger_kind != crate::domain::entities::TeamRunTriggerKind::WakeBatch
        }));
}

#[tokio::test]
async fn two_concurrent_dispatchers_produce_exactly_one_coordinator_turn() {
    let harness = build_harness();
    let team = ready_team(&harness, 5).await;
    queue_one_coordinator_wake(&harness, &team).await;

    let dispatcher_a = harness.dispatcher();
    let dispatcher_b = harness.dispatcher();
    let team_id_a = team.id.clone();
    let team_id_b = team.id.clone();
    let (result_a, result_b) = tokio::join!(
        dispatcher_a.dispatch_pending_wakes(&team_id_a),
        dispatcher_b.dispatch_pending_wakes(&team_id_b)
    );
    result_a.unwrap();
    result_b.unwrap();

    assert_eq!(
        harness.chat.get_sent_messages().await.len(),
        1,
        "exactly one coordinator turn must be produced per wake batch under concurrent dispatch"
    );
}

#[tokio::test]
async fn coordinator_delivery_queues_under_the_conversation_scoped_key() {
    // A Project conversation's `context_id` is the *project* id, but the
    // coordinator's turn-end drain keys its queue by `runtime_context_id`,
    // which is the *conversation* id for Project sends carrying
    // `conversation_id_override`. Keying the delivery by `context_id` would
    // park the settlement message on a queue nothing ever drains, so a
    // mid-turn coordinator would silently lose it.
    let harness = build_harness();
    let team = ready_team(&harness, 20).await;
    queue_one_coordinator_wake(&harness, &team).await;

    let conversation_key = QueueKey::new(
        ChatContextType::Project,
        team.coordinator_conversation_id.as_str(),
    );
    let project_key = QueueKey::new(ChatContextType::Project, "project-1");

    let on_conversation = harness
        .queued_messages
        .list(&conversation_key)
        .await
        .unwrap();
    assert_eq!(
        on_conversation.len(),
        1,
        "the coordinator delivery must land on the queue the turn-end drain reads"
    );
    assert!(on_conversation[0].content.contains("<team_message"));

    assert!(
        harness
            .queued_messages
            .list(&project_key)
            .await
            .unwrap()
            .is_empty(),
        "nothing drains the project-scoped queue for a coordinator conversation"
    );
}

#[tokio::test]
async fn successful_dispatch_marks_the_wake_binding_running() {
    // The wake binding is created at `Planned`. Nothing terminalizes a
    // coordinator binding on the success path, and only `Running -> Terminal`
    // is a legal transition, so leaving it at `Planned` is what let a
    // successful wake permanently consume automatic-wake budget.
    let harness = build_harness();
    let team = ready_team(&harness, 21).await;
    queue_one_coordinator_wake(&harness, &team).await;

    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    let wake_binding = harness
        .run_binding_repo
        .list_for_team(&team.id)
        .await
        .unwrap()
        .into_iter()
        .find(|binding| binding.trigger_kind == TeamRunTriggerKind::WakeBatch)
        .expect("claim must record a coordinator wake run binding");
    assert_eq!(
        wake_binding.status,
        TeamRunBindingStatus::Running,
        "an accepted wake send must move its binding off Planned"
    );
    assert!(
        wake_binding.launched_at.is_some(),
        "laddering through Launching must stamp launched_at"
    );
}

#[tokio::test]
async fn wake_message_only_carries_messages_addressed_to_the_coordinator() {
    // A wake batch EXTENDS across coordinator deliveries, so its
    // `first..=last` window is a Team-wide sequence range that also spans
    // member-targeted traffic and the coordinator's own outbound messages.
    // `list_messages_after` returns all of them; re-injecting them would feed
    // the coordinator its own instructions back as new input.
    let harness = build_harness();
    let team = ready_team(&harness, 22).await;

    let member = harness
        .managed_team
        .add_member(
            &team.id,
            ManagedTeamMemberSpec {
                name: "Worker".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "test member".to_string(),
                harness: None,
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();

    let assignment_id = AgentTaskAssignmentId("assignment-fold".to_string());
    let mut binding = crate::testing::team_fixtures::member_run_binding(
        "binding-fold",
        team.id.as_str(),
        910,
        member.id.as_str(),
        member.generation,
    );
    binding.assignment_id = Some(assignment_id.clone());
    binding.conversation_id = team.coordinator_conversation_id;
    harness.run_binding_repo.create(binding).await.unwrap();

    let member_result = |content: &str, key: &str| ManagedTeamMessageRequest {
        team_id: team.id.clone(),
        sender: ManagedTeamMessageSender::System {
            assignment_id: assignment_id.clone(),
        },
        target: ManagedTeamMessageTarget::Coordinator,
        kind: TeamMessageKind::Result,
        content: content.to_string(),
        idempotency_key: key.to_string(),
    };

    // Opens the wake batch at this sequence.
    harness
        .managed_team
        .send_team_message(member_result("first member result", "result-1"))
        .await
        .unwrap();

    // Coordinator -> member instruction. Lands INSIDE the batch window below,
    // but must never be echoed back to its own author.
    harness
        .managed_team
        .send_team_message(ManagedTeamMessageRequest {
            team_id: team.id.clone(),
            sender: ManagedTeamMessageSender::Coordinator {
                conversation_id: team.coordinator_conversation_id,
                source_run_id: None,
            },
            target: ManagedTeamMessageTarget::MemberName(member.name.clone()),
            kind: TeamMessageKind::Instruction,
            content: "coordinator instruction to member".to_string(),
            idempotency_key: "instruction-1".to_string(),
        })
        .await
        .unwrap();

    // Extends the batch across the instruction above.
    harness
        .managed_team
        .send_team_message(member_result("second member result", "result-2"))
        .await
        .unwrap();

    let queued = harness
        .wake_batch_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1, "the deliveries must share one wake batch");
    assert!(
        queued[0].last_message_sequence - queued[0].first_message_sequence >= 2,
        "the batch window must span the member-targeted instruction for this test to bite"
    );

    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    let sent = harness.chat.get_sent_messages().await;
    assert_eq!(sent.len(), 1, "exactly one wake must be dispatched");
    assert!(sent[0].contains("first member result"));
    assert!(sent[0].contains("second member result"));
    assert!(
        !sent[0].contains("coordinator instruction to member"),
        "member-targeted, coordinator-authored traffic must not be re-injected"
    );
    assert!(
        sent[0].contains("Team update (2 messages)."),
        "the wake header must count only coordinator-addressed messages, got: {}",
        sent[0]
    );
}

#[tokio::test]
async fn successful_dispatch_retires_the_delivered_queue_entry() {
    // `project_delivery` enqueues the coordinator delivery under the
    // conversation-scoped `QueueKey` keyed by delivery id. If a successfully
    // dispatched wake leaves that entry queued, the turn-end drain
    // (`process_queued_messages`) starts a second, redundant coordinator turn
    // with identical content.
    let harness = build_harness();
    let team = ready_team(&harness, 30).await;
    queue_one_coordinator_wake(&harness, &team).await;

    let queued_batches = harness
        .wake_batch_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    assert_eq!(queued_batches.len(), 1);
    let delivery_id = queued_batches[0].delivery_ids[0].clone();

    let conversation_key = QueueKey::new(
        ChatContextType::Project,
        team.coordinator_conversation_id.as_str(),
    );
    assert_eq!(
        harness
            .queued_messages
            .list(&conversation_key)
            .await
            .unwrap()
            .len(),
        1,
        "the settlement message must be queued before dispatch"
    );

    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    assert!(
        harness
            .queued_messages
            .list(&conversation_key)
            .await
            .unwrap()
            .is_empty(),
        "a successfully dispatched wake must retire the queue entry it just delivered"
    );

    let delivery = harness
        .managed_team
        .message_repo
        .get_delivery(&delivery_id)
        .await
        .unwrap()
        .expect("the delivery row must still exist");
    assert_eq!(
        delivery.status,
        TeamMessageDeliveryStatus::Delivered,
        "the retired delivery must be marked Delivered, the first production writer of that status"
    );
}

#[tokio::test(start_paused = true)]
async fn failed_dispatch_leaves_the_queue_entry_intact() {
    // A failed or retried send must leave the durable queue untouched: the
    // user-message delivery contract lets a gate refuse to resume a session,
    // never to discard a user message.
    let harness = build_harness();
    let team = ready_team(&harness, 31).await;
    queue_one_coordinator_wake(&harness, &team).await;
    harness.chat.set_available(false).await;

    let conversation_key = QueueKey::new(
        ChatContextType::Project,
        team.coordinator_conversation_id.as_str(),
    );

    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    assert!(
        !harness
            .queued_messages
            .list(&conversation_key)
            .await
            .unwrap()
            .is_empty(),
        "a durably failed wake must not retire the queue entry; it is still deliverable"
    );
}

#[tokio::test]
async fn mid_turn_cancellation_leaves_the_queue_entry_for_the_normal_drain() {
    // When the coordinator is already mid-turn, the settlement message is
    // exactly what the turn-end drain must find on the queue; the cancelled
    // wake batch must not retire it.
    let harness = build_harness();
    let team = ready_team(&harness, 32).await;
    queue_one_coordinator_wake(&harness, &team).await;

    let mut active_run = AgentRun::new(team.coordinator_conversation_id);
    active_run.id = crate::domain::entities::AgentRunId::new();
    harness.agent_runs.create(active_run).await.unwrap();

    let conversation_key = QueueKey::new(
        ChatContextType::Project,
        team.coordinator_conversation_id.as_str(),
    );

    harness
        .dispatcher()
        .dispatch_pending_wakes(&team.id)
        .await
        .unwrap();

    assert!(
        harness.chat.get_sent_messages().await.is_empty(),
        "an already-active coordinator run must suppress dispatch"
    );
    assert!(
        !harness
            .queued_messages
            .list(&conversation_key)
            .await
            .unwrap()
            .is_empty(),
        "the mid-turn cancel path must leave the entry for the normal turn-end drain"
    );
}
