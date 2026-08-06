//! Wake-budget decay coverage for `schedule_coordinator_wakes`.
//!
//! `TeamRunBindingRepository` has no delete method, so `WakeBatch`-triggered
//! bindings persist for the life of the Team row. These tests pin the fix
//! that counts only live (`Planned`/`Launching`/`Running`) wake bindings
//! against `automatic_wake_limit`, instead of every wake binding ever
//! created.

use std::sync::Arc;

use ralphx_events::{EventSink, RecordingEventSink};

use crate::application::managed_team::ManagedTeamService;
use crate::domain::entities::{
    AgentRun, AgentRunStatus, ChatConversation, CoordinationMode, ProjectId, TeamRunBinding,
    TeamRunBindingStatus, TeamRunTriggerKind, TeamSession, TeamSessionId,
};
use crate::domain::repositories::{
    AgentRunRepository, ChatConversationRepository, QueuedMessageRepository, TeamMessageRepository,
    TeamRepository, TeamRunBindingRepository, TeamWakeBatchRepository,
    UiFeatureFlagOverridesRepository,
};
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryChatConversationRepository, MemoryQueuedMessageRepository,
    MemoryTeamCoordinationTransitionRepository, MemoryTeamMessageRepository, MemoryTeamRepository,
    MemoryTeamRunBindingRepository, MemoryTeamWakeBatchRepository,
    MemoryTeamWorkspaceReservationRepository, MemoryUiFeatureFlagOverridesRepository,
};
use crate::testing::team_fixtures::{
    team_agent_run_id, team_conversation_id, team_delivery, team_message, team_run_binding,
};

struct Parts {
    service: ManagedTeamService,
    binding_repo: Arc<dyn TeamRunBindingRepository>,
    wake_repo: Arc<dyn TeamWakeBatchRepository>,
    agent_runs: Arc<MemoryAgentRunRepository>,
    events: RecordingEventSink,
}

async fn ready_team_service() -> (Parts, TeamSession) {
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let team_repo = Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions)));
    let binding_repo = Arc::new(MemoryTeamRunBindingRepository::new());
    let wake_repo = Arc::new(MemoryTeamWakeBatchRepository::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let agent_runs = Arc::new(MemoryAgentRunRepository::new());
    let events = RecordingEventSink::new();
    let service = ManagedTeamService::new_with_event_sink(
        Arc::clone(&team_repo) as Arc<dyn TeamRepository>,
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::clone(&binding_repo) as Arc<dyn TeamRunBindingRepository>,
        Arc::new(MemoryTeamMessageRepository::new()) as Arc<dyn TeamMessageRepository>,
        Arc::clone(&wake_repo) as Arc<dyn TeamWakeBatchRepository>,
        Arc::new(MemoryQueuedMessageRepository::new()) as Arc<dyn QueuedMessageRepository>,
        Arc::clone(&conversation_repo) as Arc<dyn ChatConversationRepository>,
        Arc::clone(&agent_runs) as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new())
            as Arc<dyn UiFeatureFlagOverridesRepository>,
        Arc::new(events.clone()) as Arc<dyn EventSink>,
    );
    let conversation_id = team_conversation_id(1);
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.id = conversation_id;
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    conversation_repo.create(conversation).await.unwrap();
    let team = service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &conversation_id,
        )
        .await
        .unwrap();
    service.startup_barrier().run(&service.team_repo()).await;
    service
        .release_delivery_projection_after_recovery()
        .await
        .unwrap();
    (
        Parts {
            service,
            binding_repo,
            wake_repo,
            agent_runs,
            events,
        },
        team,
    )
}

fn wake_binding(
    id: &str,
    team_id: &TeamSessionId,
    run_index: u64,
    status: TeamRunBindingStatus,
) -> TeamRunBinding {
    let mut binding = team_run_binding(id, team_id.as_str(), run_index);
    binding.trigger_kind = TeamRunTriggerKind::WakeBatch;
    binding.status = status;
    binding
}

fn coordinator_queued_delivery(
    team_id: &TeamSessionId,
) -> Vec<(
    crate::domain::entities::TeamMessage,
    crate::domain::entities::TeamMessageDelivery,
)> {
    let message = team_message("msg-1", team_id.as_str(), 1);
    let delivery = team_delivery("delivery-1", "msg-1", None);
    vec![(message, delivery)]
}

#[tokio::test]
async fn wake_budget_counts_only_live_wake_bindings() {
    let (parts, team) = ready_team_service().await;
    // Seed `automatic_wake_limit` (5) SETTLED wake bindings. Before the fix,
    // `schedule_coordinator_wakes` counted every `WakeBatch`-trigger binding
    // regardless of status, so this alone would already saturate the budget
    // and permanently suppress new wakes (the latent bug this test pins).
    for (index, status) in [
        TeamRunBindingStatus::Terminal,
        TeamRunBindingStatus::Failed,
        TeamRunBindingStatus::Cancelled,
        TeamRunBindingStatus::Terminal,
        TeamRunBindingStatus::Failed,
    ]
    .into_iter()
    .enumerate()
    {
        parts
            .binding_repo
            .create(wake_binding(
                &format!("settled-{index}"),
                &team.id,
                index as u64,
                status,
            ))
            .await
            .unwrap();
    }

    parts
        .service
        .schedule_coordinator_wakes(&coordinator_queued_delivery(&team.id))
        .await
        .unwrap();

    let batches = parts
        .wake_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    assert_eq!(
        batches.len(),
        1,
        "settled wake bindings must not consume automatic wake budget"
    );
}

#[tokio::test]
async fn wake_budget_blocks_on_concurrent_live_wakes() {
    let (parts, team) = ready_team_service().await;
    // Seed `automatic_wake_limit` (5) LIVE wake bindings; these must suppress
    // a new automatic wake batch. Only one binding is `Running` (the others
    // are `Planned`/`Launching`) so `running_count` (1) stays below
    // `effective_concurrency` (2) and the suppression is attributable to the
    // wake budget specifically, not the separate concurrency guard.
    for (index, status) in [
        TeamRunBindingStatus::Planned,
        TeamRunBindingStatus::Planned,
        TeamRunBindingStatus::Launching,
        TeamRunBindingStatus::Launching,
        TeamRunBindingStatus::Running,
    ]
    .into_iter()
    .enumerate()
    {
        parts
            .binding_repo
            .create(wake_binding(
                &format!("live-{index}"),
                &team.id,
                index as u64,
                status,
            ))
            .await
            .unwrap();
    }

    parts
        .service
        .schedule_coordinator_wakes(&coordinator_queued_delivery(&team.id))
        .await
        .unwrap();

    let batches = parts
        .wake_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    assert!(
        batches.is_empty(),
        "concurrent live wake bindings at the budget limit must suppress a new batch"
    );
}

/// Budget exhaustion must never be silent: it emits a needs-attention event so
/// a Team that stopped waking is diagnosable from the UI rather than only from
/// a queue inspection.
///
/// This asserts the emitted event rather than captured `tracing` output. The
/// suppression warning is logged too, but `tracing`'s global max-level hint and
/// callsite interest cache are process-wide, so a scoped `set_default`
/// subscriber can silently capture nothing when other tests in the binary
/// install their own subscribers concurrently.
#[tokio::test]
async fn suppressed_wake_emits_observability_event() {
    let (parts, team) = ready_team_service().await;
    for index in 0..5u64 {
        parts
            .binding_repo
            .create(wake_binding(
                &format!("live-{index}"),
                &team.id,
                index,
                TeamRunBindingStatus::Running,
            ))
            .await
            .unwrap();
    }

    parts
        .service
        .schedule_coordinator_wakes(&coordinator_queued_delivery(&team.id))
        .await
        .unwrap();

    let events = parts.events.events();
    let suppressed = events
        .iter()
        .find(|recorded| recorded.event == "team:needs_attention")
        .expect("budget exhaustion must emit a needs-attention event");
    assert_eq!(
        suppressed.payload["reason"],
        serde_json::json!("automatic_wake_budget_exhausted")
    );
    assert_eq!(
        suppressed.payload["team_id"],
        serde_json::json!(team.id.as_str())
    );
    assert_eq!(suppressed.payload["wake_count"], serde_json::json!(5));
    assert_eq!(
        suppressed.payload["automatic_wake_limit"],
        serde_json::json!(5)
    );
}

/// Seeds an `AgentRun` under `run_index` (matching `wake_binding`'s
/// `team_agent_run_id`) on a NON-coordinator conversation, so the seeded run
/// never trips `schedule_coordinator_wakes`' separate "coordinator already has
/// an active run" guard and the assertion stays attributable to the budget.
async fn seed_binding_run(parts: &Parts, run_index: u64, terminal: Option<AgentRunStatus>) {
    let mut run = AgentRun::new(team_conversation_id(900 + run_index));
    run.id = team_agent_run_id(run_index);
    let run_id = run.id.clone();
    parts.agent_runs.create(run).await.unwrap();
    match terminal {
        None | Some(AgentRunStatus::Running) => {}
        Some(AgentRunStatus::Completed) => parts.agent_runs.complete(&run_id).await.unwrap(),
        Some(AgentRunStatus::Failed) => parts.agent_runs.fail(&run_id, "boom").await.unwrap(),
        Some(AgentRunStatus::Cancelled) => parts.agent_runs.cancel(&run_id).await.unwrap(),
    }
}

#[tokio::test]
async fn wake_budget_recovers_after_coordinator_wake_runs_end() {
    // Nothing terminalizes a coordinator wake binding when its run finishes:
    // the coordinator's run completes through the generic chat-service
    // finalization path, and the member settlement hook only fires for member
    // runs. Without run-ended reconciliation the budget would latch off for
    // good after `automatic_wake_limit` (5) SUCCESSFUL wakes.
    let (parts, team) = ready_team_service().await;
    for (index, status) in [
        AgentRunStatus::Completed,
        AgentRunStatus::Completed,
        AgentRunStatus::Failed,
        AgentRunStatus::Cancelled,
        AgentRunStatus::Completed,
    ]
    .into_iter()
    .enumerate()
    {
        let run_index = index as u64;
        parts
            .binding_repo
            .create(wake_binding(
                &format!("ended-{index}"),
                &team.id,
                run_index,
                TeamRunBindingStatus::Running,
            ))
            .await
            .unwrap();
        seed_binding_run(&parts, run_index, Some(status)).await;
    }

    parts
        .service
        .schedule_coordinator_wakes(&coordinator_queued_delivery(&team.id))
        .await
        .unwrap();

    let batches = parts
        .wake_repo
        .list_queued_for_team(&team.id, 10)
        .await
        .unwrap();
    assert_eq!(
        batches.len(),
        1,
        "wake bindings whose runs already ended must not hold automatic wake budget"
    );

    let bindings = parts.binding_repo.list_for_team(&team.id).await.unwrap();
    for binding in bindings
        .iter()
        .filter(|binding| binding.id.0.starts_with("ended-"))
    {
        assert!(
            matches!(
                binding.status,
                TeamRunBindingStatus::Terminal
                    | TeamRunBindingStatus::Failed
                    | TeamRunBindingStatus::Cancelled
            ),
            "binding {} must be terminalized, got {:?}",
            binding.id.0,
            binding.status
        );
        assert!(binding.terminal_at.is_some());
    }
}

#[tokio::test]
async fn wake_budget_holds_while_wake_runs_are_still_live() {
    // The run row is the authority: reconciliation must never manufacture
    // budget by terminalizing a binding whose run is still executing.
    let (parts, team) = ready_team_service().await;
    for index in 0..5u64 {
        parts
            .binding_repo
            .create(wake_binding(
                &format!("live-{index}"),
                &team.id,
                index,
                TeamRunBindingStatus::Running,
            ))
            .await
            .unwrap();
        seed_binding_run(&parts, index, Some(AgentRunStatus::Running)).await;
    }

    parts
        .service
        .schedule_coordinator_wakes(&coordinator_queued_delivery(&team.id))
        .await
        .unwrap();

    assert!(
        parts
            .wake_repo
            .list_queued_for_team(&team.id, 10)
            .await
            .unwrap()
            .is_empty(),
        "live wake runs at the budget limit must still suppress a new batch"
    );
    let bindings = parts.binding_repo.list_for_team(&team.id).await.unwrap();
    assert!(
        bindings
            .iter()
            .all(|binding| binding.status == TeamRunBindingStatus::Running),
        "a still-running run must leave its binding untouched"
    );
}
