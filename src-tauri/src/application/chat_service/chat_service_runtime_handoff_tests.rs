use super::chat_service_runtime_handoff::{
    activate_runtime_handoff_watchdog, cancel_armed_runtime_handoff_owner,
    capture_runtime_handoff_owner, finalize_idle_runtime_handoff,
    map_runtime_handoff_kick_send_result, release_no_owner_runtime_handoff,
    reserve_no_owner_runtime_handoff, stage_runtime_handoff, RuntimeHandoffCapture,
    RuntimeHandoffKickOutcome, RuntimeHandoffOutcome, RuntimeHandoffReleaseOutcome,
};
use super::chat_service_streaming::is_armed_mode_handoff_disposition;
use super::{ChatService, MockChatService, SendResult};
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata, InteractiveProcessRegistry,
    InteractiveProcessRetireAfterTurnDisposition,
};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentRun, AgentRunStatus, ChatContextType, ChatConversation, Project,
};
use crate::domain::repositories::QueuedMessageRepository;
use crate::domain::services::{
    MemoryRunningAgentRegistry, MessageQueue, QueueKey, QueuedMessage, RunningAgentKey,
    RunningAgentRegistry,
};
use crate::infrastructure::memory::MemoryQueuedMessageRepository;
use std::sync::Arc;
use tokio::process::ChildStdin;
use tokio_util::sync::CancellationToken;

async fn create_test_stdin() -> (ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin fixture");
    (child.stdin.take().expect("fixture stdin"), child)
}

async fn register_running(
    registry: &Arc<MemoryRunningAgentRegistry>,
    context_id: &str,
    run_id: &str,
    cancellation_token: Option<CancellationToken>,
) {
    registry
        .register(
            RunningAgentKey::new("project", context_id),
            0,
            "conversation".to_string(),
            run_id.to_string(),
            None,
            cancellation_token,
        )
        .await;
}

async fn register_interactive(
    registry: &InteractiveProcessRegistry,
    context_id: &str,
    run_id: &str,
) -> (
    crate::application::interactive_process_registry::InteractiveProcessToken,
    tokio::process::Child,
) {
    let (stdin, child) = create_test_stdin().await;
    let token = registry
        .register_with_metadata(
            InteractiveProcessKey::new("project", context_id),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some(run_id.to_string()),
                ..Default::default()
            },
        )
        .await;
    (token, child)
}

#[tokio::test]
async fn no_owner_reservation_excludes_competing_launch_and_releases_exact_slot() {
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    let running_trait: Arc<dyn RunningAgentRegistry> = running.clone();
    let key = RunningAgentKey::new("project", "handoff-reservation");

    let reservation = reserve_no_owner_runtime_handoff(
        &running_trait,
        ChatContextType::Project,
        "handoff-reservation",
        "request-a",
    )
    .await
    .expect("a stable no-owner slot should be reserved");

    let reserved = running
        .get(&key)
        .await
        .expect("the reservation must exclude competing launches");
    assert_eq!(reserved.pid, 0);
    assert_eq!(
        reserved.agent_run_id,
        "plan-mode-handoff-reservation:request-a"
    );

    let occupied = match reserve_no_owner_runtime_handoff(
        &running_trait,
        ChatContextType::Project,
        "handoff-reservation",
        "request-b",
    )
    .await
    {
        Ok(_) => panic!("a competing request must not replace the exact reservation"),
        Err(error) => error,
    };
    assert_eq!(
        occupied
            .occupied()
            .expect("the competing reservation should report its owner")
            .agent_run_id,
        "plan-mode-handoff-reservation:request-a"
    );

    assert_eq!(
        release_no_owner_runtime_handoff(&running_trait, &reservation).await,
        RuntimeHandoffReleaseOutcome::Released
    );
    assert!(
        running.get(&key).await.is_none(),
        "release must remove only the request-owned PID-0 row"
    );
}

#[tokio::test]
async fn capture_runtime_handoff_owner_requires_two_registry_agreement() {
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    let interactive = InteractiveProcessRegistry::new();
    let (_token, _child) = register_interactive(&interactive, "handoff-capture", "run-a").await;
    register_running(&running, "handoff-capture", "run-a", None).await;
    let running_trait: Arc<dyn RunningAgentRegistry> = running.clone();

    let owner = capture_runtime_handoff_owner(
        &running_trait,
        &interactive,
        ChatContextType::Project,
        "handoff-capture",
    )
    .await;
    let RuntimeHandoffCapture::Captured(owner) = owner else {
        panic!("matching registries must produce exact owner authority");
    };
    assert_eq!(owner.agent_run_id, "run-a");

    register_running(&running, "handoff-capture", "foreign-run", None).await;
    assert!(matches!(
        capture_runtime_handoff_owner(
            &running_trait,
            &interactive,
            ChatContextType::Project,
            "handoff-capture",
        )
        .await,
        RuntimeHandoffCapture::FailedOrUncertain
    ));
}

#[tokio::test]
async fn capture_runtime_handoff_owner_reports_no_owner_only_for_stable_absence() {
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    let interactive = InteractiveProcessRegistry::new();
    let running_trait: Arc<dyn RunningAgentRegistry> = running.clone();

    assert!(matches!(
        capture_runtime_handoff_owner(
            &running_trait,
            &interactive,
            ChatContextType::Project,
            "handoff-absent",
        )
        .await,
        RuntimeHandoffCapture::NoOwner
    ));

    register_running(&running, "handoff-absent", "running-only", None).await;
    assert!(matches!(
        capture_runtime_handoff_owner(
            &running_trait,
            &interactive,
            ChatContextType::Project,
            "handoff-absent",
        )
        .await,
        RuntimeHandoffCapture::FailedOrUncertain
    ));
    running
        .unregister(
            &RunningAgentKey::new("project", "handoff-absent"),
            "running-only",
        )
        .await;

    let (_token, _child) = register_interactive(&interactive, "handoff-absent", "ipr-only").await;
    assert!(matches!(
        capture_runtime_handoff_owner(
            &running_trait,
            &interactive,
            ChatContextType::Project,
            "handoff-absent",
        )
        .await,
        RuntimeHandoffCapture::FailedOrUncertain
    ));
}

#[tokio::test]
async fn idle_staging_is_awaiting_retirement_not_started() {
    let interactive = InteractiveProcessRegistry::new();
    let (token, _child) = register_interactive(&interactive, "handoff-idle", "run-idle").await;
    assert!(
        interactive
            .mark_idle_if_token(
                &InteractiveProcessKey::new("project", "handoff-idle"),
                token
            )
            .await
    );
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-idle".to_string(),
        agent_run_id: "run-idle".to_string(),
        interactive_process_token: token,
    };
    let durable = Arc::new(MemoryQueuedMessageRepository::new());
    let queue = MessageQueue::new();
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    register_running(&running, "handoff-idle", "run-idle", None).await;
    let running: Arc<dyn RunningAgentRegistry> = running;

    let outcome = stage_runtime_handoff(
        Some(&(durable.clone() as Arc<dyn QueuedMessageRepository>)),
        &queue,
        &running,
        &interactive,
        &owner,
        QueuedMessage::new("continue".to_string()),
    )
    .await;

    assert_eq!(outcome, RuntimeHandoffOutcome::AwaitingRetirement);
    assert!(matches!(
        interactive
            .retire_after_turn_disposition_if_owner(
                &InteractiveProcessKey::new("project", "handoff-idle"),
                token,
                "run-idle",
            )
            .await,
        InteractiveProcessRetireAfterTurnDisposition::Idle { is_armed: true }
    ));
}

#[tokio::test]
async fn stale_foreign_owner_fails_and_compensates_only_its_stable_row() {
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    let interactive = InteractiveProcessRegistry::new();
    let (stale_token, _stale_child) =
        register_interactive(&interactive, "handoff-stale", "stale-run").await;
    register_running(&running, "handoff-stale", "foreign-run", None).await;
    let running_trait: Arc<dyn RunningAgentRegistry> = running.clone();
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-stale".to_string(),
        agent_run_id: "stale-run".to_string(),
        interactive_process_token: stale_token,
    };
    let durable = Arc::new(MemoryQueuedMessageRepository::new());
    let queue = MessageQueue::new();
    let continuation = QueuedMessage::new("continue".to_string());
    let continuation_id = continuation.id.clone();

    assert_eq!(
        stage_runtime_handoff(
            Some(&(durable.clone() as Arc<dyn QueuedMessageRepository>)),
            &queue,
            &running_trait,
            &interactive,
            &owner,
            continuation,
        )
        .await,
        RuntimeHandoffOutcome::Failed
    );
    assert!(durable
        .list(&QueueKey::new(ChatContextType::Project, "handoff-stale"))
        .await
        .expect("list durable queue")
        .iter()
        .all(|message| message.id != continuation_id));
    assert_eq!(
        interactive
            .capture_owner(&InteractiveProcessKey::new("project", "handoff-stale"))
            .await
            .expect("exact IPR owner is preserved")
            .agent_run_id,
        "stale-run"
    );
}

#[tokio::test]
async fn stable_continuation_staging_is_idempotent() {
    let interactive = InteractiveProcessRegistry::new();
    let (token, _child) = register_interactive(&interactive, "handoff-duplicate", "run-a").await;
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-duplicate".to_string(),
        agent_run_id: "run-a".to_string(),
        interactive_process_token: token,
    };
    let durable = Arc::new(MemoryQueuedMessageRepository::new());
    let queue = MessageQueue::new();
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    register_running(&running, "handoff-duplicate", "run-a", None).await;
    let running: Arc<dyn RunningAgentRegistry> = running;
    let continuation = QueuedMessage::new("continue".to_string());

    for _ in 0..2 {
        assert_eq!(
            stage_runtime_handoff(
                Some(&(durable.clone() as Arc<dyn QueuedMessageRepository>)),
                &queue,
                &running,
                &interactive,
                &owner,
                continuation.clone(),
            )
            .await,
            RuntimeHandoffOutcome::AwaitingRetirement
        );
    }
    assert_eq!(
        durable
            .list(&QueueKey::new(
                ChatContextType::Project,
                "handoff-duplicate"
            ))
            .await
            .expect("list durable queue")
            .len(),
        1
    );
}

#[tokio::test]
async fn same_run_registry_with_replaced_ipr_fails_closed_and_compensates() {
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    let interactive = InteractiveProcessRegistry::new();
    let (stale_token, _stale_child) =
        register_interactive(&interactive, "handoff-split-authority", "run-a").await;
    let (_replacement_token, _replacement_child) =
        register_interactive(&interactive, "handoff-split-authority", "run-b").await;
    register_running(&running, "handoff-split-authority", "run-a", None).await;
    let running_trait: Arc<dyn RunningAgentRegistry> = running.clone();
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-split-authority".to_string(),
        agent_run_id: "run-a".to_string(),
        interactive_process_token: stale_token,
    };
    let durable = Arc::new(MemoryQueuedMessageRepository::new());
    let queue = MessageQueue::new();
    let continuation = QueuedMessage::new("continue".to_string());
    let continuation_id = continuation.id.clone();

    assert_eq!(
        stage_runtime_handoff(
            Some(&(durable.clone() as Arc<dyn QueuedMessageRepository>)),
            &queue,
            &running_trait,
            &interactive,
            &owner,
            continuation,
        )
        .await,
        RuntimeHandoffOutcome::Failed
    );
    assert!(durable
        .list(&QueueKey::new(
            ChatContextType::Project,
            "handoff-split-authority"
        ))
        .await
        .expect("list durable queue")
        .iter()
        .all(|message| message.id != continuation_id));
}

#[tokio::test]
async fn missing_both_runtime_authorities_preserves_durable_recovery() {
    let interactive = InteractiveProcessRegistry::new();
    let (token, _child) =
        register_interactive(&interactive, "handoff-authorities-gone", "run-a").await;
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-authorities-gone".to_string(),
        agent_run_id: "run-a".to_string(),
        interactive_process_token: token,
    };
    interactive
        .remove(&InteractiveProcessKey::new(
            "project",
            "handoff-authorities-gone",
        ))
        .await;
    let durable = Arc::new(MemoryQueuedMessageRepository::new());
    let queue = MessageQueue::new();
    let running: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let continuation = QueuedMessage::new("continue".to_string());
    let continuation_id = continuation.id.clone();

    assert_eq!(
        stage_runtime_handoff(
            Some(&(durable.clone() as Arc<dyn QueuedMessageRepository>)),
            &queue,
            &running,
            &interactive,
            &owner,
            continuation,
        )
        .await,
        RuntimeHandoffOutcome::DurablyRecoverable
    );
    assert!(durable
        .list(&QueueKey::new(
            ChatContextType::Project,
            "handoff-authorities-gone"
        ))
        .await
        .expect("list durable queue")
        .iter()
        .any(|message| message.id == continuation_id));
}

#[tokio::test]
async fn kick_runtime_handoff_launches_once_through_the_queued_send_seam() {
    let queue = Arc::new(MessageQueue::new());
    let service = MockChatService::with_queue(Arc::clone(&queue));
    let conversation_id = crate::domain::entities::ChatConversationId::new();
    let queued = queue.queue(
        ChatContextType::Project,
        conversation_id.as_str(),
        "resume the accepted handoff".to_string(),
    );

    let started = service
        .kick_runtime_handoff(&conversation_id, &queued.id)
        .await;
    assert!(matches!(
        started,
        RuntimeHandoffKickOutcome::Started { ref agent_run_id } if !agent_run_id.is_empty()
    ));
    assert_eq!(
        service.get_sent_messages().await,
        vec!["resume the accepted handoff".to_string()],
        "the stable queue row must enter the normal send path exactly once"
    );

    assert_eq!(
        service
            .kick_runtime_handoff(&conversation_id, &queued.id)
            .await,
        RuntimeHandoffKickOutcome::Failed,
        "a duplicate kick cannot create a second run after the stable row was consumed"
    );
    assert_eq!(service.get_sent_messages().await.len(), 1);
}

#[tokio::test]
async fn runtime_handoff_kick_preserves_active_pid_zero_reservation_and_stable_row() {
    let state = AppState::new_test();
    let project_dir = tempfile::tempdir().expect("project directory should be created");
    let project = state
        .project_repo
        .create(Project::new(
            "Runtime handoff reservation".to_string(),
            project_dir.path().to_string_lossy().to_string(),
        ))
        .await
        .expect("project should persist");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id))
        .await
        .expect("conversation should persist");
    let mut owner_run = AgentRun::new(conversation.id);
    owner_run.status = crate::domain::entities::AgentRunStatus::Running;
    state
        .agent_run_repo
        .create(owner_run.clone())
        .await
        .expect("active owner run should persist");
    let owner_key = RunningAgentKey::new("project", conversation.id.as_str());
    let owner_cancellation = CancellationToken::new();
    state
        .running_agent_registry
        .register(
            owner_key.clone(),
            0,
            conversation.id.as_str().to_string(),
            owner_run.id.as_str().to_string(),
            None,
            Some(owner_cancellation.clone()),
        )
        .await;
    let queued = state.message_queue.queue(
        ChatContextType::Project,
        conversation.id.as_str(),
        "resume after retained owner".to_string(),
    );

    let service = state.build_chat_service();
    assert_eq!(
        service
            .kick_runtime_handoff(&conversation.id, &queued.id)
            .await,
        RuntimeHandoffKickOutcome::DurablyRecoverable,
        "an occupied immediate-start reservation must leave the handoff recoverable"
    );

    let owner_after = state
        .running_agent_registry
        .get(&owner_key)
        .await
        .expect("runtime-handoff kick must retain the active owner reservation");
    assert_eq!(owner_after.pid, 0);
    assert_eq!(owner_after.agent_run_id, owner_run.id.as_str());
    assert!(
        !owner_cancellation.is_cancelled(),
        "runtime-handoff must not cancel or stop a competing owner"
    );
    let persisted_owner = state
        .agent_run_repo
        .get_by_id(&owner_run.id)
        .await
        .expect("active owner run reload should succeed")
        .expect("active owner run should remain persisted");
    assert_eq!(persisted_owner.status, AgentRunStatus::Running);

    let memory_rows = state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation.id.as_str());
    assert_eq!(memory_rows.len(), 1);
    assert_eq!(memory_rows[0].id, queued.id);
    let durable_rows = state
        .queued_message_repo
        .list(&QueueKey::new(
            ChatContextType::Project,
            conversation.id.as_str(),
        ))
        .await
        .expect("recovered stable row should be durable");
    assert_eq!(durable_rows.len(), 1);
    assert_eq!(durable_rows[0].id, queued.id);
}

#[test]
fn kick_mapping_requires_an_immediate_nonblank_run_and_keeps_durable_rows_recoverable() {
    let started = SendResult {
        agent_run_id: "replacement-run".to_string(),
        ..Default::default()
    };
    assert_eq!(
        map_runtime_handoff_kick_send_result(Some(&started), false),
        RuntimeHandoffKickOutcome::Started {
            agent_run_id: "replacement-run".to_string(),
        }
    );

    let queued = SendResult {
        was_queued: true,
        ..Default::default()
    };
    assert_eq!(
        map_runtime_handoff_kick_send_result(Some(&queued), false),
        RuntimeHandoffKickOutcome::DurablyRecoverable
    );
    assert_eq!(
        map_runtime_handoff_kick_send_result(None, true),
        RuntimeHandoffKickOutcome::DurablyRecoverable
    );
    assert_eq!(
        map_runtime_handoff_kick_send_result(Some(&SendResult::default()), false),
        RuntimeHandoffKickOutcome::Failed
    );
}

#[tokio::test]
async fn watchdog_cancels_only_an_armed_exact_owner() {
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    let interactive = InteractiveProcessRegistry::new();
    let (token, _child) = register_interactive(&interactive, "handoff-watchdog", "run-a").await;
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-watchdog".to_string(),
        agent_run_id: "run-a".to_string(),
        interactive_process_token: token,
    };
    let cancellation = CancellationToken::new();
    register_running(
        &running,
        "handoff-watchdog",
        "run-a",
        Some(cancellation.clone()),
    )
    .await;
    let running_trait: Arc<dyn RunningAgentRegistry> = running.clone();

    assert!(!cancel_armed_runtime_handoff_owner(&running_trait, &interactive, &owner,).await);
    assert!(
        !cancellation.is_cancelled(),
        "unarmed user runtime must not be classified as handoff"
    );

    assert!(matches!(
        interactive
            .arm_retire_after_turn_if_owner(
                &InteractiveProcessKey::new("project", "handoff-watchdog"),
                token,
                "run-a",
            )
            .await,
        crate::application::interactive_process_registry::InteractiveProcessRetireArmDisposition::AwaitingTurn
    ));
    assert!(cancel_armed_runtime_handoff_owner(&running_trait, &interactive, &owner,).await);
    assert!(cancellation.is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn activated_watchdog_cancels_the_exact_armed_owner_after_configured_grace() {
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    let interactive = Arc::new(InteractiveProcessRegistry::new());
    let (token, _child) =
        register_interactive(&interactive, "handoff-activated-watchdog", "run-a").await;
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-activated-watchdog".to_string(),
        agent_run_id: "run-a".to_string(),
        interactive_process_token: token,
    };
    let cancellation = CancellationToken::new();
    register_running(
        &running,
        "handoff-activated-watchdog",
        "run-a",
        Some(cancellation.clone()),
    )
    .await;
    assert!(matches!(
        interactive
            .arm_retire_after_turn_if_owner(
                &InteractiveProcessKey::new("project", "handoff-activated-watchdog"),
                token,
                "run-a",
            )
            .await,
        crate::application::interactive_process_registry::InteractiveProcessRetireArmDisposition::AwaitingTurn
    ));

    activate_runtime_handoff_watchdog(
        running.clone() as Arc<dyn RunningAgentRegistry>,
        Arc::clone(&interactive),
        owner,
    );
    tokio::task::yield_now().await;
    let grace = std::time::Duration::from_secs(
        crate::infrastructure::agents::claude::stream_timeouts().completion_grace_secs,
    );
    tokio::time::advance(grace).await;
    tokio::task::yield_now().await;

    assert!(
        cancellation.is_cancelled(),
        "the configured watchdog must cancel the exact still-armed source runtime"
    );
}

#[tokio::test]
async fn idle_finalization_requires_armed_idle_exact_owner_and_removes_it_after_commit() {
    let interactive = InteractiveProcessRegistry::new();
    let (token, _child) =
        register_interactive(&interactive, "handoff-idle-finalize", "run-a").await;
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-idle-finalize".to_string(),
        agent_run_id: "run-a".to_string(),
        interactive_process_token: token,
    };
    let key = InteractiveProcessKey::new("project", "handoff-idle-finalize");
    assert!(matches!(
        interactive
            .arm_retire_after_turn_if_owner(&key, token, "run-a")
            .await,
        crate::application::interactive_process_registry::InteractiveProcessRetireArmDisposition::AwaitingTurn
    ));

    assert!(
        finalize_idle_runtime_handoff(&interactive, &owner)
            .await
            .is_none(),
        "an active owner must remain available to finish its current turn"
    );
    assert!(interactive.has_process(&key).await);

    assert!(interactive.mark_idle_if_token(&key, token).await);
    assert!(
        finalize_idle_runtime_handoff(&interactive, &owner)
            .await
            .is_some(),
        "the exact armed idle owner must retire after answer commit"
    );
    assert!(
        !interactive.has_process(&key).await,
        "post-commit finalization must remove the retired owner"
    );
}

#[tokio::test]
async fn armed_watchdog_rejects_missing_foreign_and_uncancellable_running_owners() {
    let running = Arc::new(MemoryRunningAgentRegistry::new());
    let running_trait: Arc<dyn RunningAgentRegistry> = running.clone();
    let interactive = InteractiveProcessRegistry::new();
    let (token, _child) =
        register_interactive(&interactive, "handoff-watchdog-guards", "run-a").await;
    let owner = super::RuntimeHandoffOwner {
        context_type: ChatContextType::Project,
        runtime_context_id: "handoff-watchdog-guards".to_string(),
        agent_run_id: "run-a".to_string(),
        interactive_process_token: token,
    };
    assert!(matches!(
        interactive
            .arm_retire_after_turn_if_owner(
                &InteractiveProcessKey::new("project", "handoff-watchdog-guards"),
                token,
                "run-a",
            )
            .await,
        crate::application::interactive_process_registry::InteractiveProcessRetireArmDisposition::AwaitingTurn
    ));

    assert!(
        !cancel_armed_runtime_handoff_owner(&running_trait, &interactive, &owner).await,
        "a missing running owner must fail closed"
    );

    register_running(
        &running,
        "handoff-watchdog-guards",
        "foreign-run",
        Some(CancellationToken::new()),
    )
    .await;
    assert!(
        !cancel_armed_runtime_handoff_owner(&running_trait, &interactive, &owner).await,
        "a foreign running owner must not be cancelled"
    );

    register_running(&running, "handoff-watchdog-guards", "run-a", None).await;
    assert!(
        !cancel_armed_runtime_handoff_owner(&running_trait, &interactive, &owner).await,
        "an exact owner without a cancellation token must remain untouched"
    );
}

#[test]
fn unarmed_user_cancellation_is_not_a_mode_handoff_exit() {
    assert!(!is_armed_mode_handoff_disposition(
        InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: false }
    ));
    assert!(!is_armed_mode_handoff_disposition(
        InteractiveProcessRetireAfterTurnDisposition::Idle { is_armed: false }
    ));
    assert!(is_armed_mode_handoff_disposition(
        InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: true }
    ));
}
#[tokio::test]
async fn runtime_handoff_send_queued_message_now_stops_project_runtime_and_restores_on_launch_failure(
) {
    let state = AppState::new_test();
    let project_dir = tempfile::tempdir().expect("project dir should be created");
    let project = Project::new(
        "Queued Send Now Project".to_string(),
        project_dir.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "provider-session-1".to_string(),
    });
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");

    let first = state.message_queue.queue(
        ChatContextType::Project,
        conversation_id.as_str(),
        "wait first".to_string(),
    );
    let selected = state.message_queue.queue(
        ChatContextType::Project,
        conversation_id.as_str(),
        "send me now".to_string(),
    );
    let third = state.message_queue.queue(
        ChatContextType::Project,
        conversation_id.as_str(),
        "wait third".to_string(),
    );
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", conversation_id.as_str()),
            0,
            conversation_id.as_str().to_string(),
            "active-run".to_string(),
            None,
            None,
        )
        .await;

    let service = state
        .build_chat_service()
        .with_cli_path(project_dir.path().join("missing-claude-cli"))
        .with_working_directory(project_dir.path());

    let error = service
        .send_queued_message_now(
            ChatContextType::Project,
            &conversation_id.as_str(),
            &selected.id,
        )
        .await
        .expect_err("missing CLI should restore the selected queued prompt");

    assert!(
        error.to_string().contains("Claude CLI not found"),
        "send-now should resolve the project conversation before attempting launch: {error}"
    );
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("project", conversation_id.as_str()))
            .await,
        "send-now should stop the active runtime key before relaunch"
    );

    let queued = state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str());
    assert_eq!(
        queued
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec![selected.id.as_str(), first.id.as_str(), third.id.as_str()],
        "failed immediate launch should restore the selected prompt at the front"
    );
}
