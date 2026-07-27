use std::path::PathBuf;
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_recovery::{
    recover_agent_workspace_repair_after_terminal_run,
    recover_stale_agent_workspace_publish_repairs,
    recover_stale_agent_workspace_publish_repairs_for_state,
    recover_stale_agent_workspace_publish_repairs_on_startup,
    recover_stale_agent_workspace_publish_repairs_on_startup_for_state,
    recover_stale_publish_repair_for_workspace,
    recover_stale_publish_repair_for_workspace_and_reload,
    recover_stale_publish_repair_for_workspace_and_reload_with_review_target,
    recover_stale_publish_repair_for_workspace_in_state,
    recover_stale_publish_repair_for_workspace_with_project_repo_outcome,
    recover_stale_transient_publish_statuses, StalePublishRepairRecoveryOutcome,
    STALE_NEEDS_AGENT_CLASSIFICATION, STALE_REPAIR_BLOCKED_SUMMARY, STALE_REPAIR_RECOVERED_STEP,
    STALE_TRANSIENT_CLASSIFICATION, STALE_TRANSIENT_RECOVERED_STEP,
};
use crate::application::agent_workspace_publish_repair_state::{
    reserve_agent_workspace_repair_dispatch, AgentWorkspaceRepairDispatchOutcome,
};
use crate::application::agent_workspace_review::{
    AgentWorkspaceReviewPacket, AgentWorkspaceReviewTarget,
};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, AgentRunActionKind, AgentRunId,
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairContinuation, AgentWorkspaceRepairEffect,
    AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversation,
    ChatConversationId, GitTargetIdentity, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository,
    AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
    CreateAgentWorkspaceRepairEffect, CreateAgentWorkspaceRepairEffectOutcome,
    StartOrJoinAgentWorkspaceRepairAttempt, StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentProviderSettingsRepository,
    MemoryAgentRunRepository,
};

fn conversation_id(suffix: u8) -> ChatConversationId {
    ChatConversationId::from_string(format!("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbb{suffix:02}"))
}

fn project_id() -> ProjectId {
    ProjectId::from_string("project-publish-recovery".to_string())
}

#[cfg(unix)]
struct TestEnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

#[cfg(unix)]
impl TestEnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

#[cfg(unix)]
impl Drop for TestEnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn needs_agent_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/publish-recovery".to_string(),
        "/tmp/ralphx-test-publish-recovery".to_string(),
    );
    workspace.publication_pr_number = Some(684);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/684".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace
}

fn review_target() -> AgentWorkspaceReviewTarget {
    AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        base_ref: "main".to_string(),
        base_sha: Some("base-sha".to_string()),
        head_ref: "ralphx/test/publish-recovery".to_string(),
        head_sha: Some("head-current".to_string()),
        diff_fingerprint: "diff-current".to_string(),
        working_directory: PathBuf::from("/tmp/ralphx-test-publish-recovery"),
        source_pull_request_number: None,
        review_packet: AgentWorkspaceReviewPacket::default(),
    }
}

fn reviewing_monitor(
    conversation_id: ChatConversationId,
    target: &AgentWorkspaceReviewTarget,
) -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor
}

fn stale_passed_monitor(
    conversation_id: ChatConversationId,
    target: &AgentWorkspaceReviewTarget,
) -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-stale"));
    monitor.reviewed_target_scope = Some(target.scope);
    monitor.reviewed_head_sha = Some("old-head".to_string());
    monitor.reviewed_diff_fingerprint = Some("old-diff".to_string());
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor
}

async fn seed_terminal_run(
    agent_run_repo: &dyn AgentRunRepository,
    conversation_id: ChatConversationId,
) {
    let run = agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed run");
    agent_run_repo
        .fail(&run.id, "agent repair exited")
        .await
        .expect("mark run failed");
}

async fn seed_failed_pr_autofix_run(
    agent_run_repo: &dyn AgentRunRepository,
    conversation_id: ChatConversationId,
    fingerprint: &str,
) {
    let mut run = AgentRun::new(conversation_id);
    run.action_kind = Some(AgentRunActionKind::PrAutofix);
    run.action_context_id = Some("684".to_string());
    run.action_target_id = Some(fingerprint.to_string());
    let run = agent_run_repo.create(run).await.expect("seed autofix run");
    agent_run_repo
        .fail(&run.id, "autofix interrupted")
        .await
        .expect("mark autofix failed");
}

#[tokio::test]
async fn startup_recovery_wrappers_finish_on_empty_repositories() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());

    recover_stale_agent_workspace_publish_repairs_on_startup(
        workspace_repo as Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo as Arc<dyn AgentRunRepository>,
    )
    .await;
    recover_stale_agent_workspace_publish_repairs_on_startup_for_state(&AppState::new_test()).await;
}

#[tokio::test]
async fn startup_recovery_schedules_one_due_retry_for_an_interrupted_repair_delivery() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(93);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed durable repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "interrupted delivery".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair attempt must start");
    };
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-recovery-dispatch"),
        "refs/heads/ralphx/test/publish-recovery",
    )
    .expect("valid canonical target identity");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        started,
        AgentRunId::from_string("interrupted-repair-delivery-run"),
        "dispatch durable repair",
        None,
    )
    .await
    .expect("reserve interrupted repair delivery");
    assert!(matches!(
        dispatch,
        AgentWorkspaceRepairDispatchOutcome::Reserved(_)
    ));

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("startup recovery schedules due repair retry"),
        1
    );
    let scheduled = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load scheduled repair retry")
        .expect("repair remains current");
    assert_eq!(scheduled.phase, AgentWorkspaceRepairPhase::Requested);
    assert_eq!(scheduled.dispatch_count, 1);
    assert!(scheduled.next_dispatch_at.is_some());
    assert!(scheduled.reserved_agent_run_id.is_none());
    let retry_events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load recovery retry events");
    assert_eq!(
        retry_events
            .iter()
            .filter(|event| event.step == "repair_sent" && event.status == "retrying")
            .count(),
        1
    );

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("not-due startup replay is harmless"),
        0
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("reload retry events"),
        retry_events,
        "not-due restart recovery must not dispatch or emit a duplicate repair message"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn due_startup_recovery_redelivers_once_and_binds_the_replacement_run() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let mut state = AppState::new_test();
    let worktree_parent = tempfile::tempdir().expect("create repair worktree parent");
    let project_dir = tempfile::tempdir().expect("create repair project directory");
    let cli_path = project_dir.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"durable-retry-session"}'
printf '%s\n' '{"type":"result","session_id":"durable-retry-session","is_error":false,"result":"repair started","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .expect("write fake repair CLI");
    std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))
        .expect("make fake repair CLI executable");
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut provider = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    provider.enabled = true;
    provider.is_default = true;
    provider.custom_binary_enabled = true;
    provider.custom_binary_path = Some(cli_path.display().to_string());
    state
        .agent_provider_settings_repo
        .upsert(&provider)
        .await
        .expect("enable fake Claude provider");
    let conversation_id = conversation_id(95);
    let mut project = Project::new(
        "repair recovery project".to_string(),
        project_dir.path().display().to_string(),
    );
    project.id = project_id();
    project.worktree_parent_directory = Some(worktree_parent.path().display().to_string());
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("derive exact workspace path");
    std::fs::create_dir_all(workspace_path.join(".git")).expect("seed test workspace marker");
    state
        .project_repo
        .create(project)
        .await
        .expect("seed retry project");
    let mut conversation = ChatConversation::new_project(project_id());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed retry conversation");
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.worktree_path = workspace_path.display().to_string();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed durable retry workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "retry delivery".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair attempt must start");
    };
    let target_identity =
        GitTargetIdentity::new(workspace_path, "refs/heads/ralphx/test/publish-recovery")
            .expect("valid canonical target identity");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        started,
        AgentRunId::from_string("due-retry-initial-run"),
        "reserve retry delivery",
        None,
    )
    .await
    .expect("reserve retry delivery");
    let AgentWorkspaceRepairDispatchOutcome::Reserved(dispatch) = dispatch else {
        panic!("first delivery must reserve its run");
    };
    let scheduled = crate::application::agent_workspace_publish_repair_state::settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        dispatch,
        crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        "retryable delivery failure",
        None,
    )
    .await
    .expect("schedule durable retry");
    let crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairTransitionOutcome::Applied(mut scheduled) = scheduled else {
        panic!("first delivery failure must schedule a retry");
    };
    let expected_updated_at = scheduled.updated_at;
    scheduled.next_dispatch_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    scheduled.updated_at += chrono::Duration::microseconds(1);
    let due = state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: scheduled,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Requested,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("make durable retry due");
    assert!(matches!(
        due,
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("due recovery redelivers repair"),
        1
    );
    let delivered = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load repaired attempt")
        .expect("repair remains current");
    assert_eq!(
        delivered.phase,
        AgentWorkspaceRepairPhase::Repairing,
        "due retry should bind a replacement run instead of rescheduling: {delivered:?}"
    );
    assert_eq!(delivered.dispatch_count, 1);
    assert!(delivered.next_dispatch_at.is_none());
    let replacement_run = delivered
        .reserved_agent_run_id
        .clone()
        .expect("successful due delivery binds exactly one replacement run");
    assert!(
        state
            .agent_run_repo
            .get_by_id(&replacement_run)
            .await
            .expect("load replacement run")
            .is_some(),
        "due recovery must create the bound replacement run through the chat service"
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load retry events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "repair_sent" && event.status == "succeeded")
            .count(),
        1,
        "due recovery must settle exactly one delivery event"
    );

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("duplicate recovery is suppressed"),
        0
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("reload retry events"),
        events,
        "duplicate recovery must not send another repair message or append another event"
    );
}

#[tokio::test]
async fn due_recovery_with_an_open_repair_effect_does_not_dispatch_or_append_events() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(96);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed durable retry workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "effect-owned retry".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start durable repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair attempt must start");
    };
    let target_identity = GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repair-recovery-effect"),
        "refs/heads/ralphx/test/publish-recovery",
    )
    .expect("valid canonical target identity");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        started,
        AgentRunId::from_string("effect-owned-retry-initial-run"),
        "reserve retry delivery",
        None,
    )
    .await
    .expect("reserve retry delivery");
    let AgentWorkspaceRepairDispatchOutcome::Reserved(dispatch) = dispatch else {
        panic!("first delivery must reserve its run");
    };
    let scheduled = crate::application::agent_workspace_publish_repair_state::settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        dispatch,
        crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        "retryable delivery failure",
        None,
    )
    .await
    .expect("schedule durable retry");
    let crate::application::agent_workspace_publish_repair_state::AgentWorkspaceRepairTransitionOutcome::Applied(mut scheduled) = scheduled else {
        panic!("first delivery failure must schedule a retry");
    };
    let expected_updated_at = scheduled.updated_at;
    scheduled.next_dispatch_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    scheduled.updated_at += chrono::Duration::microseconds(1);
    let due = state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: scheduled,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Requested,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("make retry due");
    let AgentWorkspaceRepairAttemptTransitionOutcome::Applied(due) = due else {
        panic!("due checkpoint must preserve retry authority");
    };
    let effect = AgentWorkspaceRepairEffect::new(
        due.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "effect-owned-retry",
        chrono::Utc::now(),
    );
    assert!(matches!(
        state
            .agent_workspace_repair_repo
            .create_repair_effect(CreateAgentWorkspaceRepairEffect {
                attempt_id: due.id.clone(),
                generation: due.generation,
                expected_phase: AgentWorkspaceRepairPhase::Requested,
                effect,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("record active repair effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));
    let events_before = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load events before suppressed retry");

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("effect-owned retry recovery is harmless"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load suppressed retry")
        .expect("repair remains current");
    assert_eq!(current.id, due.id);
    assert_eq!(current.updated_at, due.updated_at);
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Requested);
    assert!(current.reserved_agent_run_id.is_none());
    assert!(
        state
            .agent_run_repo
            .get_latest_for_conversation(&conversation_id)
            .await
            .expect("load replacement run")
            .is_none(),
        "effect ownership must suppress replacement agent-run creation"
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("reload events after suppressed retry"),
        events_before,
        "effect ownership must not append retry delivery events"
    );
}

#[tokio::test]
async fn startup_recovery_keeps_a_live_reserved_repair_run_authoritative() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(94);
    state
        .agent_conversation_workspace_repo
        .create_or_update(needs_agent_workspace(conversation_id.clone()))
        .await
        .expect("seed durable repair workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "live delivery".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair attempt must start");
    };
    let run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed live reserved repair run");
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        GitTargetIdentity::new(
            PathBuf::from("/tmp/ralphx-repair-recovery-live-run"),
            "refs/heads/ralphx/test/publish-recovery",
        )
        .expect("valid canonical target identity"),
        started,
        run.id,
        "dispatch durable repair",
        None,
    )
    .await
    .expect("reserve live repair delivery");
    assert!(matches!(
        dispatch,
        AgentWorkspaceRepairDispatchOutcome::Reserved(_)
    ));

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("live repair recovery is a no-op"),
        0
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load live repair")
        .expect("repair remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Dispatching);
    assert_eq!(current.dispatch_count, 0);
    assert_eq!(current.reserved_agent_run_id, Some(run.id));
}

#[tokio::test]
async fn failed_exact_pr_autofix_is_classified_as_retry_eligible() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(31);
    let workspace = needs_agent_workspace(conversation_id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let fingerprint = "github_pr_autofix:684:head:checks";
    seed_failed_pr_autofix_run(state.agent_run_repo.as_ref(), conversation_id, fingerprint).await;

    let (_workspace, outcome) =
        recover_stale_publish_repair_for_workspace_with_project_repo_outcome(
            Arc::clone(&state.agent_conversation_workspace_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.project_repo),
            workspace,
        )
        .await
        .expect("recover retry-eligible repair");

    assert_eq!(outcome, StalePublishRepairRecoveryOutcome::RetryEligible);
}

#[tokio::test]
async fn state_recovery_recovers_terminal_needs_agent_workspace_and_reloads_it() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(1);
    let workspace = needs_agent_workspace(conversation_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_terminal_run(state.agent_run_repo.as_ref(), conversation_id).await;

    let recovered = recover_stale_agent_workspace_publish_repairs_for_state(&state)
        .await
        .expect("recover stale publish repair");

    assert_eq!(recovered, 1);
    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(events.iter().any(|event| {
        event.step == "legacy_repair_import_blocked"
            && event.classification.as_deref() == Some("legacy_repair_import_ambiguous")
    }));
}

#[tokio::test]
async fn recovery_correlates_the_exact_pr_autofix_attempt_not_a_newer_unrelated_run() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(11);
    let workspace = needs_agent_workspace(conversation_id);
    let fingerprint = "github_pr_autofix:684:head:failing-check";
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix",
            "needs_agent",
            "PR autofix started.",
            Some(fingerprint.to_string()),
        ))
        .await
        .expect("seed autofix event");
    seed_failed_pr_autofix_run(agent_run_repo.as_ref(), conversation_id, fingerprint).await;
    let unrelated = agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed unrelated run");
    agent_run_repo
        .complete(&unrelated.id)
        .await
        .expect("complete unrelated run");

    let (updated, outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
            None,
        )
        .await
        .expect("recover exact autofix attempt");

    assert_eq!(outcome, StalePublishRepairRecoveryOutcome::RetryEligible);
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("single retry is eligible"));
}

#[tokio::test]
async fn recovery_with_review_target_preserves_current_reviewing_handoff() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(2);
    let workspace = needs_agent_workspace(conversation_id);
    let target = review_target();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");
    workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor(conversation_id, &target))
        .await
        .expect("seed review monitor");
    seed_terminal_run(agent_run_repo.as_ref(), conversation_id).await;

    let (refreshed, outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
            Some(&target),
        )
        .await
        .expect("check stale publish repair");

    assert_eq!(outcome, StalePublishRepairRecoveryOutcome::HandoffPreserved);
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(
        !events
            .iter()
            .any(|event| event.step == "stale_repair_recovered"),
        "current Workspace Review handoff must not be downgraded as stale"
    );
}

#[tokio::test]
async fn stale_review_handoff_without_matching_target_is_recovered_and_reloaded() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(3);
    let workspace = needs_agent_workspace(conversation_id);
    let target = review_target();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");
    workspace_repo
        .upsert_workspace_review_monitor(stale_passed_monitor(conversation_id, &target))
        .await
        .expect("seed stale passed review monitor");
    seed_terminal_run(agent_run_repo.as_ref(), conversation_id).await;

    let refreshed = recover_stale_publish_repair_for_workspace_and_reload(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        workspace,
    )
    .await
    .expect("recover stale publish repair");

    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(events
        .iter()
        .any(|event| event.step == "stale_repair_recovered"));
}

#[tokio::test]
async fn batch_recovery_counts_only_recovered_workspaces() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let recoverable_id = conversation_id(4);
    let active_id = conversation_id(5);
    let recoverable = needs_agent_workspace(recoverable_id);
    let active = needs_agent_workspace(active_id);
    workspace_repo
        .create_or_update(recoverable)
        .await
        .expect("seed recoverable workspace");
    workspace_repo
        .create_or_update(active)
        .await
        .expect("seed active workspace");
    seed_terminal_run(agent_run_repo.as_ref(), recoverable_id).await;
    agent_run_repo
        .create(AgentRun::new(active_id))
        .await
        .expect("seed active run");

    let recovered = recover_stale_agent_workspace_publish_repairs(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
    )
    .await
    .expect("recover batch");

    assert_eq!(recovered, 1);
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&active_id)
            .await
            .expect("load active workspace")
            .expect("active workspace exists")
            .publication_push_status
            .as_deref(),
        Some("needs_agent")
    );
}

#[tokio::test]
async fn recovery_heals_only_an_active_current_repair_to_fixing() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(6);
    let mut workspace = needs_agent_workspace(conversation_id);
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed active run");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_sent",
            "succeeded",
            "Sent failure to workspace repair agent",
            Some("agent_fixable".to_string()),
        ))
        .await
        .expect("seed repair evidence");

    let refreshed = recover_stale_publish_repair_for_workspace_and_reload(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        workspace,
    )
    .await
    .expect("reconcile active repair");

    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("fixing"));
}

#[tokio::test]
async fn recovery_restores_blocked_state_only_for_the_current_pr_autofix_replacement() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(7);
    let mut workspace = needs_agent_workspace(conversation_id.clone());
    workspace.pr_supervision_status = Some("blocked".to_string());
    let fingerprint = "github_pr_autofix:684:head:replacement";
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed blocked workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix",
            "started",
            "PR autofix replacement started.",
            Some(fingerprint.to_string()),
        ))
        .await
        .expect("seed autofix evidence");
    let mut replacement = AgentRun::new(conversation_id.clone());
    replacement.action_kind = Some(AgentRunActionKind::PrAutofix);
    replacement.action_context_id = Some("684".to_string());
    replacement.action_target_id = Some(fingerprint.to_string());
    agent_run_repo
        .create(replacement)
        .await
        .expect("seed exact active replacement");

    let (refreshed, outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
            None,
        )
        .await
        .expect("recover exact active replacement");

    assert_eq!(
        outcome,
        StalePublishRepairRecoveryOutcome::ActiveReplacement
    );
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("fixing"));
}

#[tokio::test]
async fn recovery_does_not_treat_an_unrelated_active_run_as_a_pr_autofix_replacement() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(8);
    let workspace = needs_agent_workspace(conversation_id.clone());
    let fingerprint = "github_pr_autofix:684:head:retry";
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix",
            "failed",
            "PR autofix failed.",
            Some(fingerprint.to_string()),
        ))
        .await
        .expect("seed exact autofix event");
    seed_failed_pr_autofix_run(
        agent_run_repo.as_ref(),
        conversation_id.clone(),
        fingerprint,
    )
    .await;
    agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed unrelated active run");

    let (refreshed, outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
            None,
        )
        .await
        .expect("recover retry-eligible autofix");

    assert_eq!(outcome, StalePublishRepairRecoveryOutcome::RetryEligible);
    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
}

mod extracted_inline_tests {
    use super::*;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, AgentRun, AgentRunStatus, ChatConversationId,
        IdeationAnalysisBaseRefKind, ProjectId,
    };
    use crate::infrastructure::memory::{
        MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    };

    fn needs_agent_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-1".to_string()),
            "ralphx/test/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.publication_pr_number = Some(42);
        workspace.publication_pr_status = Some("failed".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_supervision_status = Some("fixing".to_string());
        workspace
    }

    async fn create_failed_run(
        agent_run_repo: &MemoryAgentRunRepository,
        conversation_id: ChatConversationId,
    ) {
        let run = agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed run");
        agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("mark run failed");
    }

    #[tokio::test]
    async fn recovers_needs_agent_workspace_when_no_agent_run_is_active() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        let recovered = recover_stale_agent_workspace_publish_repairs(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await
        .expect("recover stale repair");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));

        let events = workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("list events");
        assert!(events.iter().any(|event| {
            event.step == STALE_REPAIR_RECOVERED_STEP
                && event.status == "succeeded"
                && event.classification.as_deref() == Some(STALE_NEEDS_AGENT_CLASSIFICATION)
        }));
    }

    #[tokio::test]
    async fn reloads_recovered_workspace_from_app_state() {
        let state = crate::application::AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
        let workspace = needs_agent_workspace(conversation_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed run");
        state
            .agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("mark run failed");

        let refreshed = recover_stale_publish_repair_for_workspace_in_state(&state, workspace)
            .await
            .expect("recover stale repair");

        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn recovers_stale_supervised_autofix_workspace_as_blocked() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_current = Some(true);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        let recovered = recover_stale_agent_workspace_publish_repairs(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await
        .expect("recover stale repair");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
        assert_eq!(
            refreshed.pr_supervision_summary.as_deref(),
            Some(STALE_REPAIR_BLOCKED_SUMMARY)
        );
        assert_eq!(refreshed.pr_auto_merge_current, Some(true));
    }

    #[tokio::test]
    async fn startup_helper_recovers_stale_repairs() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("77777777-7777-7777-7777-777777777777");

        recover_stale_agent_workspace_publish_repairs_on_startup(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await;

        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        recover_stale_agent_workspace_publish_repairs_on_startup(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await;

        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn keeps_needs_agent_workspace_locked_while_agent_run_is_active() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed active run");

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("needs_agent")
        );
        assert!(
            workspace_repo
                .list_publication_events(&conversation_id)
                .await
                .expect("list events")
                .is_empty(),
            "active repairs must not be downgraded"
        );
    }

    #[tokio::test]
    async fn ignores_workspace_that_is_not_waiting_on_agent_repair() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace.publication_push_status = Some("failed".to_string());

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
    }

    #[tokio::test]
    async fn ignores_workspace_without_terminal_repair_run_evidence() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("55555555-5555-5555-5555-555555555555");
        let workspace = needs_agent_workspace(conversation_id);

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
    }

    #[tokio::test]
    async fn recovers_terminal_run_without_completion_timestamp() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("88888888-8888-8888-8888-888888888888");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let mut run = AgentRun::new(conversation_id);
        run.status = AgentRunStatus::Failed;
        run.completed_at = None;
        agent_run_repo.create(run).await.expect("seed run");

        let recovered =
            recover_stale_publish_repair_for_workspace(workspace_repo, agent_run_repo, workspace)
                .await
                .expect("check repair state");

        assert!(recovered);
    }

    #[tokio::test]
    async fn does_not_recover_a_fresh_claim_from_an_older_terminal_run() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("66666666-6666-6666-6666-666666666666");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;
        workspace.updated_at = chrono::Utc::now() + chrono::Duration::minutes(5);

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
        let current = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            current.publication_push_status.as_deref(),
            Some("needs_agent")
        );
    }

    #[tokio::test]
    async fn stale_recovery_snapshot_cannot_overwrite_newer_workspace_state() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("99999999-9999-9999-9999-999999999999");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;
        workspace_repo
            .update_publication(
                &conversation_id,
                workspace.publication_pr_number,
                workspace.publication_pr_url.as_deref(),
                workspace.publication_pr_status.as_deref(),
                Some("pushed"),
            )
            .await
            .expect("persist newer publication state");
        workspace_repo
            .update_pr_auto_merge_state(
                &conversation_id,
                workspace.pr_auto_merge_current,
                Some("monitoring"),
                Some("Newer state is authoritative"),
            )
            .await
            .expect("persist newer supervision state");

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("stale recovery should be rejected");

        assert!(!recovered);
        let current = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(current.publication_push_status.as_deref(), Some("pushed"));
        assert_eq!(current.pr_supervision_status.as_deref(), Some("monitoring"));
        assert!(workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap()
            .iter()
            .all(|event| event.step != STALE_REPAIR_RECOVERED_STEP));
    }

    fn transient_workspace(
        conversation_id: ChatConversationId,
        status: &str,
    ) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-1".to_string()),
            "ralphx/test/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.publication_push_status = Some(status.to_string());
        workspace
    }

    #[tokio::test]
    async fn recovers_stale_transient_refreshing_workspace() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let conversation_id =
            ChatConversationId::from_string("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        let workspace = transient_workspace(conversation_id.clone(), "refreshing");
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        // stale_older_than_secs=0 means any workspace updated at or before now is stale
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            0,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));

        let events = workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list events");
        assert!(events.iter().any(|e| {
            e.step == STALE_TRANSIENT_RECOVERED_STEP
                && e.status == "succeeded"
                && e.classification.as_deref() == Some(STALE_TRANSIENT_CLASSIFICATION)
        }));
    }

    #[tokio::test]
    async fn skips_recent_transient_workspace_within_staleness_window() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let conversation_id =
            ChatConversationId::from_string("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        let workspace = transient_workspace(conversation_id.clone(), "checking");
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        // stale_older_than_secs=3600 means only workspaces older than 1 hour are stale;
        // a just-created workspace must not be recovered
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            3600,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 0);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("checking")
        );
    }

    #[tokio::test]
    async fn recovers_all_four_stale_transient_statuses() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());

        for (id, status) in [
            ("cccccccc-cccc-cccc-cccc-cccccccccc01", "refreshing"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc02", "checking"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc03", "committing"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc04", "describing"),
        ] {
            let conv_id = ChatConversationId::from_string(id.to_string());
            let workspace = transient_workspace(conv_id, status);
            workspace_repo
                .create_or_update(workspace)
                .await
                .expect("seed workspace");
        }

        // stale_older_than_secs=0 catches all freshly-seeded transient workspaces
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            0,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 4);
    }

    #[tokio::test]
    async fn imports_only_exact_legacy_repair_provenance_then_blocks_its_terminal_run() {
        let state = AppState::new_test();
        let conversation_id = conversation_id(91);
        let workspace = needs_agent_workspace(conversation_id.clone());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id.clone()))
            .await
            .expect("seed exact legacy run");
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_requested",
                "started",
                "legacy publish repair requested",
                Some("agent_fixable:publish".to_string()),
            ))
            .await
            .expect("seed continuation provenance");
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_sent",
                "succeeded",
                "legacy repair dispatched",
                Some(format!("agent_fixable:run:{}", run.id)),
            ))
            .await
            .expect("seed run provenance");

        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&state)
                .await
                .expect("import exact legacy repair"),
            0
        );
        let imported = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load durable attempt")
            .expect("exact provenance should import");
        assert_eq!(imported.source, AgentWorkspaceRepairSource::Legacy);
        assert_eq!(
            imported.continuation,
            AgentWorkspaceRepairContinuation::Publish
        );
        assert_eq!(imported.phase, AgentWorkspaceRepairPhase::Repairing);
        assert_eq!(imported.reserved_agent_run_id, Some(run.id));
        let imported_workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("reload imported compatibility projection")
            .expect("workspace remains present");
        let imported_events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("load import audit events");

        // Startup/recovery may re-enter after a crash. The exact legacy import is one-time:
        // it joins the same durable generation and replays neither projection nor audit events.
        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&state)
                .await
                .expect("repeat exact legacy import"),
            0
        );
        assert_eq!(
            state
                .agent_workspace_repair_repo
                .get_current_repair_attempt(&conversation_id)
                .await
                .expect("reload durable attempt")
                .expect("attempt remains current")
                .id,
            imported.id
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .expect("reload compatibility projection")
                .expect("workspace remains present"),
            imported_workspace
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .list_publication_events(&conversation_id)
                .await
                .expect("reload import audit events"),
            imported_events
        );

        state
            .agent_run_repo
            .fail(&run.id, "repair process stopped")
            .await
            .expect("terminalize run");
        assert!(recover_agent_workspace_repair_after_terminal_run(
            &state,
            &conversation_id,
            &run.id
        )
        .await
        .expect("recover exact terminal run"));
        let blocked = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load blocked attempt")
            .expect("attempt remains visible for retry");
        assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
        assert!(blocked.blocker.is_some());
    }

    #[tokio::test]
    async fn startup_recovery_ignores_exact_legacy_provenance_when_a_durable_attempt_exists() {
        let state = AppState::new_test();
        let conversation_id = conversation_id(92);
        let workspace = needs_agent_workspace(conversation_id.clone());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed legacy projection");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id.clone()))
            .await
            .expect("seed active durable run");
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_requested",
                "started",
                "legacy publish repair requested",
                Some("agent_fixable:publish".to_string()),
            ))
            .await
            .expect("seed exact legacy continuation provenance");
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_sent",
                "succeeded",
                "legacy repair dispatched",
                Some(format!("agent_fixable:run:{}", run.id)),
            ))
            .await
            .expect("seed exact legacy run provenance");

        let mut durable_attempt = AgentWorkspaceRepairAttempt::new(
            conversation_id.clone(),
            AgentWorkspaceRepairSource::BaseUpdate,
            AgentWorkspaceRepairContinuation::UpdateOnly,
            "main",
            false,
            false,
            false,
            None,
            chrono::Utc::now(),
        );
        durable_attempt.phase = AgentWorkspaceRepairPhase::Repairing;
        durable_attempt.reserved_agent_run_id = Some(run.id.clone());
        let durable_attempt = match state
            .agent_workspace_repair_repo
            .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: durable_attempt,
                reason: "durable repair already owns this conversation".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            })
            .await
            .expect("start durable repair")
        {
            StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
            outcome => panic!("expected a new durable repair, got {outcome:?}"),
        };
        let before_workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load compatibility projection")
            .expect("workspace remains present");
        let before_events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("load legacy events");

        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&state)
                .await
                .expect("recover with durable authority"),
            0
        );

        assert_eq!(
            state
                .agent_workspace_repair_repo
                .get_current_repair_attempt(&conversation_id)
                .await
                .expect("reload durable repair")
                .expect("durable repair remains current")
                .id,
            durable_attempt.id
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .expect("reload compatibility projection")
                .expect("workspace remains present"),
            before_workspace
        );
        assert_eq!(
            state
                .agent_conversation_workspace_repo
                .list_publication_events(&conversation_id)
                .await
                .expect("reload legacy events"),
            before_events
        );
        assert!(state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&durable_attempt.id)
            .await
            .expect("load durable effects")
            .is_none());
    }

    #[tokio::test]
    async fn ambiguous_legacy_repair_fails_closed_without_guessing_run_or_continuation() {
        let state = AppState::new_test();
        let conversation_id = conversation_id(92);
        state
            .agent_conversation_workspace_repo
            .create_or_update(needs_agent_workspace(conversation_id.clone()))
            .await
            .expect("seed ambiguous legacy workspace");

        assert_eq!(
            recover_stale_agent_workspace_publish_repairs_for_state(&state)
                .await
                .expect("block ambiguous legacy repair"),
            1
        );
        let attempt = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("load blocked legacy attempt")
            .expect("ambiguous legacy state must remain observable");
        assert_eq!(attempt.source, AgentWorkspaceRepairSource::Legacy);
        assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Blocked);
        assert_eq!(
            attempt.continuation,
            AgentWorkspaceRepairContinuation::Manual
        );
        assert!(attempt.reserved_agent_run_id.is_none());
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load compatibility projection")
            .expect("workspace exists");
        assert_eq!(workspace.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(workspace.pr_supervision_status.as_deref(), Some("blocked"));
    }
}
