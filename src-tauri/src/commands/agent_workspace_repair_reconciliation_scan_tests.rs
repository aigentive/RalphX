#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Listener;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::agent_workspace_pr_supervision_recovery::{
    recover_agent_workspace_durable_repair_reconciliation,
    AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::agent_workspace_publish_recovery::recover_agent_workspace_repair_attempts_for_state;
use crate::application::agent_workspace_publish_repair_state::{
    NEEDS_HUMAN_REPAIR_REASON, ORPHANED_REPAIR_DISPATCH_RESCUE_GRACE_SECS,
};
use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    pr_supervision_schedule_route, schedule_pr_supervision_recovery_for_workspace,
    try_acquire_agent_workspace_publish_guard, PrSupervisionScheduleRoute,
};
use crate::commands::ExecutionState;
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun,
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairAttemptId, AgentWorkspaceRepairContinuation,
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairPhase,
    AgentWorkspaceRepairSource, ChatConversation, ChatConversationId, GitTargetIdentity,
    GitTargetLeaseOwner, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, AgentWorkspaceRepairRepository,
    BindAgentWorkspaceRepairAttemptRun, CompleteAgentWorkspaceRepairEffect,
    CompleteAgentWorkspaceRepairEffectOutcome, CreateAgentWorkspaceRepairEffect,
    CreateAgentWorkspaceRepairEffectOutcome, ImportLegacyAgentWorkspaceRepairAttempt,
    ImportLegacyAgentWorkspaceRepairAttemptOutcome, SettleAgentWorkspaceRepairAttempt,
    SettleAgentWorkspaceRepairAttemptOutcome, SettleAndStartAgentWorkspaceRepairSuccessor,
    SettleAndStartAgentWorkspaceRepairSuccessorOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

use super::agent_workspace_blocked_repair_base_retry_scan::run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle;
use super::agent_workspace_repair_reconciliation_scan::{
    persist_stale_base_detected_at_transition,
    run_agent_workspace_base_freshness_scan_tick_from_app_handle,
    run_agent_workspace_repair_reconciliation_scan_tick_for_state,
    run_agent_workspace_repair_reconciliation_scan_tick_from_app_handle,
};

fn mock_app_with_state(state: AppState) -> tauri::App<MockRuntime> {
    mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

fn mock_app_with_state_and_execution(
    state: AppState,
    execution_state: Arc<ExecutionState>,
) -> tauri::App<MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(execution_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

fn run_git(repo_path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout should be utf8")
        .trim()
        .to_string()
}

fn commit_file(repo_path: &Path, relative_path: &str, contents: &str, message: &str) -> String {
    std::fs::write(repo_path.join(relative_path), contents)
        .expect("fixture file should be written");
    run_git(repo_path, &["add", relative_path]);
    run_git(repo_path, &["commit", "-m", message]);
    run_git(repo_path, &["rev-parse", "HEAD"])
}

/// Seeds an active, unpublished Edit workspace (no PR yet) with its own real git repo + worktree
/// so `resolve_workspace_base` / `inspect_publish_branch_freshness_for_source_after_fetch` can
/// run against real refs. Auto-publish flags default to disabled; tests opt in explicitly.
fn unpublished_git_workspace_fixture() -> (tempfile::TempDir, Project, AgentConversationWorkspace) {
    let root = tempfile::tempdir().expect("temp repo should be created");
    let project_repo = root.path().join("project");
    let worktree_parent = root.path().join("worktrees");
    std::fs::create_dir_all(&project_repo).expect("project repo directory should be created");
    std::fs::create_dir_all(&worktree_parent).expect("worktree parent should be created");

    run_git(&project_repo, &["init"]);
    run_git(&project_repo, &["config", "user.email", "test@example.com"]);
    run_git(&project_repo, &["config", "user.name", "Test User"]);
    run_git(&project_repo, &["checkout", "-b", "main"]);
    let base_commit = commit_file(&project_repo, "README.md", "initial\n", "initial");

    let conversation_id = ChatConversationId::new();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        ProjectId::from_string(format!("project-{}", conversation_id.as_str())),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_commit),
        format!("ralphx/test/{}", conversation_id.as_str()),
        String::new(),
    );

    let mut project = Project::new(
        "Base Freshness Fixture".to_string(),
        project_repo.to_string_lossy().to_string(),
    );
    project.id = workspace.project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let worktree_path =
        resolve_agent_conversation_workspace_path(&project, &workspace.conversation_id)
            .expect("workspace path should resolve");
    run_git(
        &project_repo,
        &[
            "worktree",
            "add",
            "-b",
            &workspace.branch_name,
            worktree_path
                .to_str()
                .expect("worktree path should be utf8"),
            "main",
        ],
    );
    workspace.worktree_path = worktree_path.to_string_lossy().to_string();

    (root, project, workspace)
}

fn minimal_active_workspace(
    conversation_id: ChatConversationId,
    suffix: &str,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string(format!("project-{suffix}")),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        format!("ralphx/test/{suffix}"),
        format!("/tmp/ralphx-scan-test-{suffix}"),
    )
}

/// Seeds a repair attempt whose reservation has no bound run and is already past the
/// spawn-grace window, so reconciliation settles it as an interrupted delivery through a purely
/// repo-owned path (a fabricated but shape-valid target lease; no real git/filesystem access):
/// `settle_agent_workspace_repair_dispatch_outcome` appends exactly one `repair_sent` publication
/// event when it schedules (or blocks) the retry. A Dispatching attempt always carries a canonical
/// target lease in production (set atomically by `reserve_agent_workspace_repair_dispatch`), and
/// `settle_agent_workspace_repair_dispatch_outcome` asserts one exists, so this fixture must too.
async fn seed_dispatching_repair_attempt(
    state: &AppState,
    conversation_id: ChatConversationId,
) -> AgentWorkspaceRepairAttempt {
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id,
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "scan reconciliation fixture".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut attempt) = started else {
        panic!("first repair attempt must start");
    };
    let target_identity = GitTargetIdentity::new(
        std::path::PathBuf::from(format!(
            "/tmp/ralphx-scan-test-fixture-{}",
            attempt.conversation_id.as_str()
        )),
        "refs/heads/ralphx/test/scan-fixture",
    )
    .expect("valid canonical fixture target identity");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner,
        })
        .await
        .expect("acquire fixture target lease")
    else {
        panic!("fixture target lease acquisition must succeed");
    };
    let expected_updated_at = attempt.updated_at;
    attempt.git_common_dir = Some(
        target_identity
            .git_common_dir()
            .to_string_lossy()
            .into_owned(),
    );
    attempt.target_ref = Some(target_identity.full_ref().to_string());
    attempt.target_identity_version = Some(
        crate::application::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION,
    );
    attempt.target_lease_epoch = Some(fencing_epoch);
    attempt.phase = AgentWorkspaceRepairPhase::Dispatching;
    attempt.updated_at = chrono::Utc::now()
        - chrono::Duration::seconds(ORPHANED_REPAIR_DISPATCH_RESCUE_GRACE_SECS + 60);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Dispatching,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed dispatching repair attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("seeding dispatching attempt must apply, got {outcome:?}"),
    }
}

async fn wait_for_repair_publication_event(
    state: &AppState,
    conversation_id: &ChatConversationId,
    step: &str,
) {
    for _ in 0..100 {
        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(conversation_id)
            .await
            .expect("load publication events");
        if events.iter().any(|event| event.step == step) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("expected a '{step}' publication event within timeout");
}

#[tokio::test]
async fn scan_tick_schedules_recovery_for_an_active_workspace_with_an_unsettled_repair_attempt() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    state
        .agent_conversation_workspace_repo
        .create_or_update(minimal_active_workspace(
            conversation_id.clone(),
            "positive",
        ))
        .await
        .expect("seed workspace");
    seed_dispatching_repair_attempt(&state, conversation_id.clone()).await;

    let scheduled = run_agent_workspace_repair_reconciliation_scan_tick_for_state(
        &state,
        &Arc::new(ExecutionState::new()),
    )
    .await
    .expect("scan tick should succeed");
    assert_eq!(scheduled, 1);

    wait_for_repair_publication_event(&state, &conversation_id, "repair_sent").await;
}

/// Proof obligation 1: a due retry fires from a scan tick alone (no `WorkspaceLoad`); an attempt
/// still within its spawn-grace window is left untouched by the same tick.
#[tokio::test]
async fn scan_tick_redelivers_a_due_interrupted_dispatch_but_leaves_a_fresh_one_untouched() {
    let state = AppState::new_test();

    let due_conversation_id = ChatConversationId::new();
    state
        .agent_conversation_workspace_repo
        .create_or_update(minimal_active_workspace(due_conversation_id.clone(), "due"))
        .await
        .expect("seed due workspace");
    seed_dispatching_repair_attempt(&state, due_conversation_id.clone()).await;

    let fresh_conversation_id = ChatConversationId::new();
    state
        .agent_conversation_workspace_repo
        .create_or_update(minimal_active_workspace(
            fresh_conversation_id.clone(),
            "fresh",
        ))
        .await
        .expect("seed fresh workspace");
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                fresh_conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "fresh dispatch, not yet due".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start fresh repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut fresh_attempt) = started else {
        panic!("fresh repair attempt must start");
    };
    let expected_updated_at = fresh_attempt.updated_at;
    fresh_attempt.phase = AgentWorkspaceRepairPhase::Dispatching;
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: fresh_attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Dispatching,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed fresh dispatching attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("seeding fresh dispatching attempt must apply, got {outcome:?}"),
    }

    let scheduled = run_agent_workspace_repair_reconciliation_scan_tick_for_state(
        &state,
        &Arc::new(ExecutionState::new()),
    )
    .await
    .expect("scan tick should succeed");
    assert_eq!(
        scheduled, 2,
        "both candidates are scheduled onto the reconciler"
    );

    wait_for_repair_publication_event(&state, &due_conversation_id, "repair_sent").await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let fresh_current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&fresh_conversation_id)
        .await
        .expect("reload fresh attempt")
        .expect("fresh attempt remains current");
    assert_eq!(
        fresh_current.phase,
        AgentWorkspaceRepairPhase::Dispatching,
        "an attempt still inside its spawn-grace window must not be redelivered"
    );
    let fresh_events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&fresh_conversation_id)
        .await
        .expect("load fresh events");
    assert!(
        fresh_events.is_empty(),
        "not-yet-due candidates must not produce a retry event"
    );
}

/// Proof obligation 2: an attempt already settled `RetryableFailure` (durable `next_dispatch_at`
/// scheduled by the deferred waiter) is re-dispatched by a later scan tick.
#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn scan_tick_redelivers_a_due_scheduled_retry_and_binds_a_replacement_run() {
    let _environment_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("lock test environment");
    let _spawn_permission = TestEnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let mut state = AppState::new_test();
    let worktree_parent = tempfile::tempdir().expect("create scan repair worktree parent");
    let project_dir = tempfile::tempdir().expect("create scan repair project directory");
    let cli_path = project_dir.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"repair started"}]},"session_id":"scan-due-retry-session"}'
printf '%s\n' '{"type":"result","session_id":"scan-due-retry-session","is_error":false,"result":"repair started","cost_usd":0.0}'
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

    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-scan-due-retry".to_string());
    let mut project = Project::new(
        "scan due retry project".to_string(),
        project_dir.path().display().to_string(),
    );
    project.id = project_id.clone();
    project.worktree_parent_directory = Some(worktree_parent.path().display().to_string());
    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("derive exact scan workspace path");
    std::fs::create_dir_all(workspace_path.join(".git")).expect("seed test workspace marker");
    state
        .project_repo
        .create(project)
        .await
        .expect("seed scan retry project");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed scan retry conversation");
    let mut workspace = minimal_active_workspace(conversation_id.clone(), "scan-due-retry");
    workspace.project_id = project_id;
    workspace.worktree_path = workspace_path.display().to_string();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed scan retry workspace");

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
            reason: "scan due scheduled retry".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start scan repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut attempt) = started else {
        panic!("first scan repair attempt must start");
    };
    let target_identity =
        GitTargetIdentity::new(workspace_path, "refs/heads/ralphx/test/scan-due-retry")
            .expect("valid canonical scan target identity");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner,
        })
        .await
        .expect("acquire scan due retry target lease")
    else {
        panic!("scan due retry lease acquisition must succeed");
    };
    let expected_updated_at = attempt.updated_at;
    attempt.git_common_dir = Some(
        target_identity
            .git_common_dir()
            .to_string_lossy()
            .into_owned(),
    );
    attempt.target_ref = Some(target_identity.full_ref().to_string());
    attempt.target_identity_version = Some(
        crate::application::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION,
    );
    attempt.target_lease_epoch = Some(fencing_epoch);
    attempt.dispatch_count = 1;
    // Models a durable `next_dispatch_at` already scheduled by an earlier settled
    // `RetryableFailure` (the deferred waiter), now due.
    attempt.next_dispatch_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Requested,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("make scan due retry due")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("seeding due scan retry must apply, got {outcome:?}"),
    }

    let scheduled = run_agent_workspace_repair_reconciliation_scan_tick_for_state(
        &state,
        &Arc::new(ExecutionState::new()),
    )
    .await
    .expect("scan tick should succeed");
    assert_eq!(scheduled, 1);

    for _ in 0..200 {
        let current = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("reload scan retry attempt")
            .expect("scan retry attempt remains current");
        if current.phase == AgentWorkspaceRepairPhase::Repairing {
            assert!(current.reserved_agent_run_id.is_some());
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "expected the scan tick to redeliver the due scheduled retry and bind a replacement run"
    );
}

/// Proof obligation 3b (load-bearing): a scan-driven reconcile racing the
/// `claim_recovery`-bypassing startup whole-table loop over the same attempt performs exactly one
/// state transition; the CAS fence in `transition_repair_attempt` — not `claim_recovery` — is what
/// makes this safe, since the whole-table loop never calls `claim_recovery`.
#[tokio::test]
async fn concurrent_scan_and_startup_sweep_settle_the_same_attempt_exactly_once() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    state
        .agent_conversation_workspace_repo
        .create_or_update(minimal_active_workspace(conversation_id.clone(), "race"))
        .await
        .expect("seed race workspace");
    seed_dispatching_repair_attempt(&state, conversation_id.clone()).await;

    let (durable, startup) = tokio::join!(
        recover_agent_workspace_durable_repair_reconciliation(&state, &conversation_id),
        recover_agent_workspace_repair_attempts_for_state(&state)
    );
    durable.expect("scan-driven durable reconciliation");
    startup.expect("startup whole-table reconciliation sweep");

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load race events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "repair_sent")
            .count(),
        1,
        "the id/generation/updated_at CAS fence must let exactly one racer settle the attempt"
    );
}

/// Proof obligation 10: a repair-repo listing error aborts the tick with no recovery; a
/// subsequently healthy repo lets the next tick retry successfully.
#[tokio::test]
async fn scan_tick_fails_closed_when_repair_attempt_listing_errors() {
    let mut state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    state
        .agent_conversation_workspace_repo
        .create_or_update(minimal_active_workspace(
            conversation_id.clone(),
            "fail-closed",
        ))
        .await
        .expect("seed fail-closed workspace");
    let healthy_repair_repo = Arc::clone(&state.agent_workspace_repair_repo);
    seed_dispatching_repair_attempt(&state, conversation_id.clone()).await;

    state.agent_workspace_repair_repo = Arc::new(ListingErrorRepairRepository::new(Arc::clone(
        &healthy_repair_repo,
    )));

    let error = run_agent_workspace_repair_reconciliation_scan_tick_for_state(
        &state,
        &Arc::new(ExecutionState::new()),
    )
    .await
    .expect_err("a repair-repo listing failure must abort the tick");
    assert!(error.contains("repair attempt listing failed"));

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load fail-closed events");
    assert!(
        events.is_empty(),
        "a fail-closed tick must perform no recovery, never read as 'nothing pending'"
    );

    state.agent_workspace_repair_repo = healthy_repair_repo;
    let scheduled = run_agent_workspace_repair_reconciliation_scan_tick_for_state(
        &state,
        &Arc::new(ExecutionState::new()),
    )
    .await
    .expect("the next tick must retry successfully once the repo recovers");
    assert_eq!(scheduled, 1);
}

/// Proof obligation 4: with `github_service = None`, both a workspace-load-style recovery and a
/// scan-tick-style recovery run the durable reconciler for a stuck repair — `github_service` is
/// checked only inside `schedule_pr_supervision_recovery_for_workspace`, which after Phase B
/// routes the non-GitHub case through the durable-only helper unconditionally.
#[tokio::test]
async fn non_github_workspace_get_and_scan_tick_both_run_durable_reconciler() {
    let state = AppState::new_test();
    assert!(
        state.github_service.is_none(),
        "test AppState must be non-GitHub for this proof"
    );

    for (suffix, trigger) in [
        (
            "non-github-get",
            AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        ),
        (
            "non-github-scan",
            AgentWorkspacePrSupervisionRecoveryTrigger::PeriodicScan,
        ),
    ] {
        let conversation_id = ChatConversationId::new();
        let workspace = minimal_active_workspace(conversation_id.clone(), suffix);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        seed_dispatching_repair_attempt(&state, conversation_id.clone()).await;

        schedule_pr_supervision_recovery_for_workspace(
            &state,
            &Arc::new(ExecutionState::new()),
            &workspace,
            trigger,
            true,
        );

        wait_for_repair_publication_event(&state, &conversation_id, "repair_sent").await;
    }
}

/// Proof obligation 2: routing a PR-supervision-ineligible workspace away from the expensive
/// runtime must not drop durable repair. With GitHub configured and a terminal publication PR,
/// scheduling still reconciles the stuck repair attempt through the durable-only reconciler — a
/// plain "skip and return" veto would leave the attempt stranded forever.
#[tokio::test]
async fn pr_supervision_ineligible_workspace_still_runs_the_durable_reconciler() {
    let mut state = AppState::new_test();
    state.github_service = Some(
        Arc::new(crate::tests::mock_github_service::MockGithubService::new())
            as Arc<dyn crate::domain::services::GithubServiceTrait>,
    );

    let conversation_id = ChatConversationId::new();
    let mut workspace = minimal_active_workspace(conversation_id.clone(), "ineligible-durable");
    workspace.auto_publish_enabled = true;
    workspace.pr_autofix_enabled = true;
    workspace.publication_pr_number = Some(4242);
    workspace.publication_pr_status = Some("merged".to_string());
    assert_eq!(
        pr_supervision_schedule_route(true, &workspace),
        PrSupervisionScheduleRoute::DurableOnly("workspace_terminal"),
        "fixture must exercise the durable-only routing arm"
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_dispatching_repair_attempt(&state, conversation_id.clone()).await;

    schedule_pr_supervision_recovery_for_workspace(
        &state,
        &Arc::new(ExecutionState::new()),
        &workspace,
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        true,
    );

    wait_for_repair_publication_event(&state, &conversation_id, "repair_sent").await;
}

#[tokio::test]
async fn app_handle_scan_tick_skips_when_startup_git_auth_is_pending() {
    let state = AppState::new_test();
    state.startup_git_auth_recovery_state.mark_pending();
    let app = mock_app_with_state(state);

    let count = run_agent_workspace_repair_reconciliation_scan_tick_from_app_handle(app.handle())
        .await
        .expect("pending startup recovery should skip the scan tick");

    assert_eq!(count, 0);
}

#[tokio::test]
async fn app_handle_scan_tick_errors_without_app_state() {
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let error = run_agent_workspace_repair_reconciliation_scan_tick_from_app_handle(app.handle())
        .await
        .expect_err("missing AppState should fail");

    assert_eq!(error, "AppState is not available");
}

#[tokio::test]
async fn app_handle_scan_tick_schedules_recovery_through_the_managed_app_state() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    state
        .agent_conversation_workspace_repo
        .create_or_update(minimal_active_workspace(
            conversation_id.clone(),
            "app-handle-positive",
        ))
        .await
        .expect("seed app-handle workspace");
    seed_dispatching_repair_attempt(&state, conversation_id.clone()).await;
    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));

    let scheduled =
        run_agent_workspace_repair_reconciliation_scan_tick_from_app_handle(app.handle())
            .await
            .expect("app-handle scan tick should succeed");
    assert_eq!(scheduled, 1);
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

/// Scripts only `list_recoverable_repair_attempts`; every other call delegates to a real memory
/// repo so this fixture stays a narrow fault-injection wrapper rather than a second fake.
/// `Duplicated` reproduces the per-attempt listing shape the memory repo's one-current-attempt
/// invariant cannot otherwise produce: several unsettled rows for the same conversation.
enum ScriptedRecoverableListing {
    Error,
    Duplicated,
}

struct ListingErrorRepairRepository {
    inner: Arc<dyn AgentWorkspaceRepairRepository>,
    listing: ScriptedRecoverableListing,
}

impl ListingErrorRepairRepository {
    fn new(inner: Arc<dyn AgentWorkspaceRepairRepository>) -> Self {
        Self {
            inner,
            listing: ScriptedRecoverableListing::Error,
        }
    }

    fn duplicating(inner: Arc<dyn AgentWorkspaceRepairRepository>) -> Self {
        Self {
            inner,
            listing: ScriptedRecoverableListing::Duplicated,
        }
    }
}

#[async_trait]
impl AgentWorkspaceRepairRepository for ListingErrorRepairRepository {
    async fn get_current_repair_attempt(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        self.inner.get_current_repair_attempt(conversation_id).await
    }

    async fn get_latest_repair_attempt_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        self.inner
            .get_latest_repair_attempt_for_conversation(conversation_id)
            .await
    }

    async fn get_repair_attempt(
        &self,
        attempt_id: &AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        self.inner.get_repair_attempt(attempt_id).await
    }

    async fn get_repair_attempt_for_run(
        &self,
        conversation_id: &ChatConversationId,
        run_id: &crate::domain::entities::AgentRunId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        self.inner
            .get_repair_attempt_for_run(conversation_id, run_id)
            .await
    }

    async fn list_recoverable_repair_attempts(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceRepairAttempt>> {
        match self.listing {
            ScriptedRecoverableListing::Error => Err(AppError::Infrastructure(
                "repair attempt listing failed".to_string(),
            )),
            ScriptedRecoverableListing::Duplicated => {
                let attempts = self.inner.list_recoverable_repair_attempts().await?;
                Ok(attempts
                    .into_iter()
                    .flat_map(|attempt| [attempt.clone(), attempt])
                    .collect())
            }
        }
    }

    async fn list_repair_attempts_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentWorkspaceRepairAttempt>> {
        self.inner
            .list_repair_attempts_for_conversation(conversation_id)
            .await
    }

    async fn start_or_join_repair_attempt(
        &self,
        request: StartOrJoinAgentWorkspaceRepairAttempt,
    ) -> AppResult<StartOrJoinAgentWorkspaceRepairAttemptOutcome> {
        self.inner.start_or_join_repair_attempt(request).await
    }

    async fn bind_repair_attempt_run(
        &self,
        request: BindAgentWorkspaceRepairAttemptRun,
    ) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome> {
        self.inner.bind_repair_attempt_run(request).await
    }

    async fn transition_repair_attempt(
        &self,
        request: AgentWorkspaceRepairAttemptTransition,
    ) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome> {
        self.inner.transition_repair_attempt(request).await
    }

    async fn settle_repair_attempt(
        &self,
        request: SettleAgentWorkspaceRepairAttempt,
    ) -> AppResult<SettleAgentWorkspaceRepairAttemptOutcome> {
        self.inner.settle_repair_attempt(request).await
    }

    async fn settle_and_start_repair_successor(
        &self,
        request: SettleAndStartAgentWorkspaceRepairSuccessor,
    ) -> AppResult<SettleAndStartAgentWorkspaceRepairSuccessorOutcome> {
        self.inner.settle_and_start_repair_successor(request).await
    }

    async fn create_repair_effect(
        &self,
        request: CreateAgentWorkspaceRepairEffect,
    ) -> AppResult<CreateAgentWorkspaceRepairEffectOutcome> {
        self.inner.create_repair_effect(request).await
    }

    async fn get_repair_effect_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<crate::domain::entities::AgentWorkspaceRepairEffect>> {
        self.inner
            .get_repair_effect_by_idempotency_key(idempotency_key)
            .await
    }

    async fn get_open_repair_effect(
        &self,
        attempt_id: &AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<crate::domain::entities::AgentWorkspaceRepairEffect>> {
        self.inner.get_open_repair_effect(attempt_id).await
    }

    async fn complete_repair_effect(
        &self,
        request: CompleteAgentWorkspaceRepairEffect,
    ) -> AppResult<CompleteAgentWorkspaceRepairEffectOutcome> {
        self.inner.complete_repair_effect(request).await
    }

    async fn import_legacy_repair_attempt(
        &self,
        request: ImportLegacyAgentWorkspaceRepairAttempt,
    ) -> AppResult<ImportLegacyAgentWorkspaceRepairAttemptOutcome> {
        self.inner.import_legacy_repair_attempt(request).await
    }
}

// ============================================================================
// Phase E-scan: base-ahead detection + gated auto-update (F6)
// ============================================================================

#[tokio::test]
async fn base_freshness_scan_sets_stale_base_detected_at_on_ahead_transition_and_emits_change_once()
{
    let (_repo, project, mut workspace) = unpublished_git_workspace_fixture();
    workspace.auto_publish_enabled = false;
    workspace.auto_publish_initial_pr_enabled = false;
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");
    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    app.handle()
        .listen("agent:workspace_changed", move |event| {
            let _ = tx.send(event.payload().to_string());
        });

    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("current-base tick should succeed");
    assert_eq!(updated, 0);
    assert!(
        rx.try_recv().is_err(),
        "no transition should mean no emitted event"
    );

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("ahead-base tick should succeed");
    assert_eq!(
        updated, 0,
        "auto-publish-disabled workspace should only be detected, not updated"
    );

    let reloaded = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert!(reloaded.stale_base_detected_at.is_some());
    let payload = rx
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("transition should emit agent:workspace_changed");
    assert_eq!(
        payload,
        serde_json::json!({ "conversation_id": conversation_id.as_str() }).to_string()
    );

    // Steady-state tick: still ahead, no new transition, no re-emit.
    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("steady-state tick should succeed");
    assert_eq!(updated, 0);
    assert!(
        rx.try_recv().is_err(),
        "a steady-state tick must not re-emit agent:workspace_changed"
    );
}

#[tokio::test]
async fn base_freshness_scan_clears_stale_base_detected_at_when_base_is_current() {
    let (_repo, project, mut workspace) = unpublished_git_workspace_fixture();
    workspace.stale_base_detected_at = Some(Utc::now() - chrono::Duration::hours(1));
    let conversation_id = workspace.conversation_id.clone();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");
    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));

    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("current-base tick should succeed");
    assert_eq!(updated, 0);

    let reloaded = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert_eq!(reloaded.stale_base_detected_at, None);
}

#[tokio::test]
async fn base_freshness_scan_leaves_stale_base_detected_at_untouched_when_base_is_blocked() {
    let (_repo, project, mut workspace) = unpublished_git_workspace_fixture();
    let previously_detected = Utc::now() - chrono::Duration::hours(1);
    workspace.stale_base_detected_at = Some(previously_detected);
    workspace.base_ref = "deleted-base".to_string();
    workspace.base_commit = None;
    let conversation_id = workspace.conversation_id.clone();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");
    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));

    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("blocked-base tick should still succeed");
    assert_eq!(updated, 0);

    let reloaded = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert_eq!(reloaded.stale_base_detected_at, Some(previously_detected));
}

#[tokio::test]
async fn persist_stale_base_detected_at_transition_survives_concurrent_publication() {
    let conversation_id = ChatConversationId::new();
    let workspace = minimal_active_workspace(conversation_id.clone(), "concurrent-publication");
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should seed");

    // Simulates a PR landing (via `update_publication`) after the scan's listing snapshot was
    // taken but before the base-freshness transition is persisted.
    workspace_repo
        .update_publication(
            &conversation_id,
            Some(1042),
            Some("https://github.com/example/repo/pull/1042"),
            Some("open"),
            Some("pushed"),
        )
        .await
        .expect("concurrent publication update should succeed");

    let refreshed = persist_stale_base_detected_at_transition::<tauri::test::MockRuntime>(
        &state, None, workspace, true,
    )
    .await
    .expect("transition should persist")
    .expect("workspace should still exist");

    assert!(refreshed.stale_base_detected_at.is_some());
    assert_eq!(refreshed.publication_pr_number, Some(1042));
    assert_eq!(refreshed.publication_pr_status.as_deref(), Some("open"));
    assert_eq!(refreshed.publication_push_status.as_deref(), Some("pushed"));

    let reloaded = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert_eq!(reloaded.publication_pr_number, Some(1042));
    assert!(reloaded.stale_base_detected_at.is_some());
}

#[tokio::test]
async fn persist_stale_base_detected_at_transition_survives_concurrent_toggle() {
    let conversation_id = ChatConversationId::new();
    let workspace = minimal_active_workspace(conversation_id.clone(), "concurrent-toggle");
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should seed");

    // Simulates the user flipping automation preferences after the scan's listing snapshot was
    // taken but before the base-freshness transition is persisted.
    workspace_repo
        .update_pr_supervision_preferences(&conversation_id, true, true, "squash")
        .await
        .expect("concurrent preference toggle should succeed");

    let refreshed = persist_stale_base_detected_at_transition::<tauri::test::MockRuntime>(
        &state, None, workspace, true,
    )
    .await
    .expect("transition should persist")
    .expect("workspace should still exist");

    assert!(refreshed.stale_base_detected_at.is_some());
    assert!(refreshed.pr_autofix_enabled);
    assert!(refreshed.pr_auto_merge_desired);

    let reloaded = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert!(reloaded.pr_autofix_enabled);
    assert!(reloaded.pr_auto_merge_desired);
    assert!(reloaded.stale_base_detected_at.is_some());
}

#[tokio::test]
async fn base_freshness_scan_skips_auto_update_when_agent_run_is_active() {
    let (_repo, project, workspace) = unpublished_git_workspace_fixture();
    // `auto_publish_enabled` defaults to `true` on a freshly constructed workspace: opted in.
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");
    state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("active run should seed");

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("tick should succeed even when the idle gate blocks the update");
    assert_eq!(
        updated, 0,
        "an active agent run must block unattended auto-update"
    );

    let reloaded = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert!(
        reloaded.stale_base_detected_at.is_some(),
        "detection must still run under the idle gate"
    );
}

#[tokio::test]
async fn base_freshness_scan_skips_auto_update_when_repair_attempt_is_unsettled() {
    let (_repo, project, workspace) = unpublished_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");
    seed_dispatching_repair_attempt(&state, conversation_id.clone()).await;

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("tick should succeed even when the repair gate blocks the update");
    assert_eq!(
        updated, 0,
        "an unsettled durable repair attempt must block unattended auto-update"
    );
}

#[tokio::test]
async fn base_freshness_scan_skips_auto_update_when_not_opted_in() {
    let (_repo, project, mut workspace) = unpublished_git_workspace_fixture();
    workspace.auto_publish_enabled = false;
    workspace.auto_publish_initial_pr_enabled = false;
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("tick should succeed");
    assert_eq!(updated, 0, "a non-opted-in workspace gets detection only");

    let reloaded = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert!(reloaded.stale_base_detected_at.is_some());
}

#[tokio::test]
async fn base_freshness_scan_auto_updates_idle_opted_in_workspace_when_base_is_ahead() {
    let (_repo, project, workspace) = unpublished_git_workspace_fixture();
    // `auto_publish_enabled` defaults to `true`: opted in, idle, no repair attempt.
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = AppState::new_test();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("tick should succeed");
    assert_eq!(
        updated, 1,
        "an idle, opted-in, repair-settled workspace with an ahead base should auto-update"
    );
}

#[tokio::test]
async fn base_freshness_scan_skips_auto_update_when_publish_guard_is_held_then_proceeds_once_free()
{
    let (_repo, project, workspace) = unpublished_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));

    {
        let _guard = try_acquire_agent_workspace_publish_guard(&conversation_id)
            .expect("publish guard should be acquirable");
        let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed even when the publish guard is held");
        assert_eq!(
            updated, 0,
            "publish-guard contention must be a benign skip, not a failure"
        );
        let reloaded = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace should load")
            .expect("workspace should exist");
        assert!(
            reloaded.stale_base_detected_at.is_some(),
            "detection must not be blocked by publish-guard contention"
        );
    }

    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("tick should succeed once the publish guard is free");
    assert_eq!(
        updated, 1,
        "auto-update should proceed once the publish guard is free"
    );
}

#[tokio::test]
async fn base_freshness_scan_skips_a_missing_worktree_candidate_without_blocking_a_healthy_one() {
    let (_repo, project, mut healthy_workspace) = unpublished_git_workspace_fixture();
    healthy_workspace.auto_publish_enabled = false;
    healthy_workspace.auto_publish_initial_pr_enabled = false;
    let healthy_conversation_id = healthy_workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();

    let ineligible_conversation_id = ChatConversationId::new();
    let mut ineligible_workspace = AgentConversationWorkspace::new(
        ineligible_conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        healthy_workspace.base_commit.clone(),
        format!("ralphx/test/{}", ineligible_conversation_id.as_str()),
        format!(
            "/tmp/ralphx-scan-missing-worktree-{}",
            ineligible_conversation_id.as_str()
        ),
    );
    ineligible_workspace.auto_publish_enabled = false;
    ineligible_workspace.auto_publish_initial_pr_enabled = false;

    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(healthy_workspace)
        .await
        .expect("healthy workspace should seed");
    workspace_repo
        .create_or_update(ineligible_workspace)
        .await
        .expect("ineligible workspace should seed");
    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));

    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("a structurally ineligible candidate must not fail the tick");
    assert_eq!(updated, 0);

    let ineligible_reloaded = workspace_repo
        .get_by_conversation_id(&ineligible_conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert_eq!(ineligible_reloaded.stale_base_detected_at, None);

    // Advance the base so the healthy candidate is detected in the very same tick as the
    // ineligible one, proving the ineligible candidate does not block detection for others.
    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");
    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("tick should not fail");
    assert_eq!(
        updated, 0,
        "auto-publish-disabled workspace should only be detected, not updated"
    );

    let healthy_reloaded = workspace_repo
        .get_by_conversation_id(&healthy_conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert!(
        healthy_reloaded.stale_base_detected_at.is_some(),
        "the ineligible candidate must not block detection for a healthy candidate in the same tick"
    );

    let ineligible_reloaded_again = workspace_repo
        .get_by_conversation_id(&ineligible_conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert_eq!(ineligible_reloaded_again.stale_base_detected_at, None);
}

#[tokio::test]
async fn app_handle_base_freshness_scan_tick_skips_when_startup_git_auth_is_pending() {
    let state = AppState::new_test();
    state.startup_git_auth_recovery_state.mark_pending();
    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));

    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("pending startup recovery should skip the scan");

    assert_eq!(updated, 0);
}

#[tokio::test]
async fn app_handle_base_freshness_scan_tick_errors_without_execution_state() {
    let app = mock_app_with_state(AppState::new_test());

    let error = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect_err("missing ExecutionState should fail");

    assert_eq!(error, "ExecutionState is not available");
}

// ============================================================================
// Blocked-repair base-retry half: unattended parity for the selection-time
// auto-refresh-from-base trigger (2026-08-13 incident)
// ============================================================================

/// The incident workspace shape: the unpublished Edit git fixture, published and routed to
/// `needs_agent`. `list_active_unpublished_edit_workspaces` therefore does not list it, which is
/// half of why the existing base-freshness lane never reached the stuck conversation.
fn published_git_workspace_fixture() -> (tempfile::TempDir, Project, AgentConversationWorkspace) {
    let (root, project, mut workspace) = unpublished_git_workspace_fixture();
    workspace.publication_pr_number = Some(1042);
    workspace.publication_pr_url = Some("https://github.com/example/repo/pull/1042".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    (root, project, workspace)
}

/// Generation 7's exact pending reasons: the `pr_autofix_needs_human` hold that falsified both
/// candidate directions in plan v2, plus the agent-minutes spend-limit marker it also carried.
fn incident_pending_reasons() -> Vec<String> {
    vec![
        NEEDS_HUMAN_REPAIR_REASON.to_string(),
        "agent_minutes_spend_limit_reached".to_string(),
    ]
}

/// Seeds project + workspace + a Blocked repair attempt with no queued dispatch — the exact shape
/// `agent_workspace_repair_operation_recovery_action` projects as `RetryRepair`. Attempt defaults
/// are the incident's (recorded target base commit = the workspace's pre-drift base tip, incident
/// pending reasons); `tune_attempt` overrides them for the negative gates.
async fn seed_blocked_repair_state(
    project: Project,
    workspace: AgentConversationWorkspace,
    tune_attempt: impl FnOnce(&mut AgentWorkspaceRepairAttempt),
) -> AppState {
    let state = AppState::new_test();
    let conversation_id = workspace.conversation_id.clone();
    let recorded_target_base_commit = workspace.base_commit.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");

    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id,
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                Utc::now(),
            ),
            reason: "blocked repair base retry fixture".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut attempt) = started else {
        panic!("first repair attempt must start");
    };
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.next_dispatch_at = None;
    attempt.target_base_commit = recorded_target_base_commit;
    attempt.pending_reasons = incident_pending_reasons();
    attempt.dispatch_count = 0;
    attempt.updated_at = Utc::now();
    tune_attempt(&mut attempt);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: attempt.clone(),
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: attempt.phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed blocked repair attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("seeding a blocked attempt must apply, got {outcome:?}"),
    }

    state
}

async fn current_repair_attempt(
    repair_repo: &Arc<dyn AgentWorkspaceRepairRepository>,
    conversation_id: &ChatConversationId,
) -> AgentWorkspaceRepairAttempt {
    repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("current repair attempt should load")
        .expect("a repair attempt should exist")
}

/// Asserts the durable state is untouched: the seeded Blocked generation is still the current
/// attempt, so no successor was started. A supersede would settle it and make the current attempt
/// an unsettled `BaseUpdate` generation instead.
async fn assert_no_repair_dispatch(
    repair_repo: &Arc<dyn AgentWorkspaceRepairRepository>,
    conversation_id: &ChatConversationId,
) {
    let attempt = current_repair_attempt(repair_repo, conversation_id).await;
    assert_eq!(
        attempt.phase,
        AgentWorkspaceRepairPhase::Blocked,
        "the blocked generation must survive untouched"
    );
    assert_eq!(
        attempt.source,
        AgentWorkspaceRepairSource::Publish,
        "no base-update successor generation may be started"
    );
    assert!(
        attempt.settled_at.is_none(),
        "the blocked generation must not be superseded"
    );
}

/// Proof obligations 1, 5, and 6. Reproduces generation 7 exactly: Blocked with no queued
/// dispatch, holding `pr_autofix_needs_human` plus the spend-limit marker, its recorded target base
/// commit left behind by a base branch that has since moved. Neither shipped scan half dispatches
/// a repair; the new lane supersedes it with exactly one `BaseUpdate` successor, once per tip.
#[tokio::test]
async fn blocked_repair_base_retry_dispatches_once_when_base_moved() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let blocked_generation = current_repair_attempt(&repair_repo, &conversation_id)
        .await
        .generation;

    // The two shipped halves run first and must leave the blocked generation exactly as it is.
    // The reconciliation half still *schedules* PR-supervision recovery for this workspace, which
    // is not a repair dispatch, so the assertion is on durable repair state rather than its count.
    run_agent_workspace_repair_reconciliation_scan_tick_from_app_handle(app.handle())
        .await
        .expect("reconciliation half should succeed");
    let base_freshness_updated =
        run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
            .await
            .expect("base-freshness half should succeed");
    assert_eq!(
        base_freshness_updated, 0,
        "a published workspace is not even a base-freshness candidate"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;

    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("blocked-repair base-retry tick should succeed");
    assert_eq!(
        dispatched, 1,
        "a blocked, retry-admissible attempt whose base has moved must be re-driven unattended"
    );

    let successor = current_repair_attempt(&repair_repo, &conversation_id).await;
    assert_eq!(
        successor.generation,
        blocked_generation + 1,
        "the blocked generation must be superseded by exactly one successor"
    );
    assert_eq!(
        successor.source,
        AgentWorkspaceRepairSource::BaseUpdate,
        "the successor must come from the update-from-base route, exactly like the UI lane"
    );
    let superseded = repair_repo
        .list_repair_attempts_for_conversation(&conversation_id)
        .await
        .expect("repair attempts should load")
        .into_iter()
        .find(|attempt| attempt.generation == blocked_generation)
        .expect("the original generation should still be readable");
    assert_eq!(
        superseded.outcome,
        Some(crate::domain::entities::AgentWorkspaceRepairOutcome::Superseded),
        "the stuck generation must be settled as superseded, not left unsettled"
    );

    // Obligation 5. The successor records the freshly observed tip as its target, so the very same
    // base motion cannot read as new evidence twice — this is the one-retry-per-tip invariant, and
    // it holds regardless of whether the successor's own dispatch then succeeds or re-blocks.
    let dispatched_again =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("second tick should succeed");
    assert_eq!(
        dispatched_again, 0,
        "one base tip may authorize at most one unattended retry"
    );
}

/// Proof obligation 6, pinned on its own: the `pr_autofix_needs_human` hold does NOT block this
/// lane. That is deliberate parity with the shipped selection-time lane (Overview Decision 1), and
/// it is the behavior that actually resolved the incident. A future strictness change must break
/// this test rather than silently diverge the two lanes again.
#[tokio::test]
async fn blocked_repair_base_retry_retries_through_the_needs_human_hold_by_design() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |attempt| {
        attempt.pending_reasons = vec![NEEDS_HUMAN_REPAIR_REASON.to_string()];
    })
    .await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 1,
        "the needs-human hold must not fence the unattended lane; the UI lane retries through it"
    );

    let settled_original = repair_repo
        .list_repair_attempts_for_conversation(&conversation_id)
        .await
        .expect("repair attempts should load")
        .into_iter()
        .find(|attempt| attempt.settled_at.is_some())
        .expect("the superseded generation should still be readable");
    assert!(
        settled_original
            .pending_reasons
            .iter()
            .any(|reason| reason == NEEDS_HUMAN_REPAIR_REASON),
        "the retried generation is the one that carried the human hold"
    );
}

/// Proof obligation 2: the base is ahead of the workspace, but the attempt already targets the
/// fetched tip. That is the durable equivalent of the frontend's per-session dedupe key.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_the_attempt_already_targets_the_fetched_tip() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();

    // Advance the base before seeding so the attempt can record the post-drift tip.
    let advanced_tip = commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let state = seed_blocked_repair_state(project, workspace, |attempt| {
        attempt.target_base_commit = Some(advanced_tip);
    })
    .await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 0,
        "an already-targeted tip is not new evidence and must not authorize another supersede"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: the base has not moved at all.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_base_is_not_ahead() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(dispatched, 0, "a current base authorizes no retry");
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: without a recorded target base commit, base motion cannot be proven for
/// this attempt, so the lane fails closed and leaves the workspace to the selection lane.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_the_recorded_target_commit_is_missing() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |attempt| {
        attempt.target_base_commit = None;
    })
    .await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 0,
        "an unprovable base move must fail closed, not retry optimistically"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: the frontend gate requires `!hasUncommittedChanges`.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_the_worktree_is_dirty() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");
    std::fs::write(worktree_path.join("uncommitted.txt"), "wip\n")
        .expect("uncommitted change should be written");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 0,
        "an unattended timer must never re-drive a dirty worktree"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: the frontend gate requires `unpublishedCommitCount === 0`.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_the_branch_has_unpublished_commits() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    run_git(
        &worktree_path,
        &["config", "user.email", "test@example.com"],
    );
    run_git(&worktree_path, &["config", "user.name", "Test User"]);
    commit_file(&worktree_path, "work.txt", "work\n", "workspace work");
    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 0,
        "unpublished work on the branch must not be re-driven unattended"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: a blocked base is not stale, mirroring the existing base-freshness half.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_the_base_is_blocked() {
    let (_repo, project, mut workspace) = published_git_workspace_fixture();
    workspace.base_ref = "deleted-base".to_string();
    let conversation_id = workspace.conversation_id.clone();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("a blocked base must not fail the tick");
    assert_eq!(dispatched, 0);
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: Overview Decision 2's consent gate. A non-opted-in workspace keeps today's
/// selection-triggered behavior.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_not_opted_in() {
    let (_repo, project, mut workspace) = published_git_workspace_fixture();
    workspace.auto_publish_enabled = false;
    workspace.auto_publish_initial_pr_enabled = false;
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 0,
        "unattended agent spend requires an automation opt-in"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: the idle gate.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_an_agent_run_is_active() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);
    state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("active run should seed");

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(dispatched, 0, "an active run blocks unattended re-drive");
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: a queued dispatch means the attempt is not retry-admissible
/// (`agent_workspace_repair_operation_recovery_action` returns `None`, not `RetryRepair`).
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_the_attempt_has_a_queued_dispatch() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |attempt| {
        attempt.next_dispatch_at = Some(Utc::now() + chrono::Duration::minutes(5));
    })
    .await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 0,
        "an already-queued retry must not be superseded by this lane"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 3: an open durable repair effect fences retry admission.
#[tokio::test]
async fn blocked_repair_base_retry_skips_when_an_open_repair_effect_fences_admission() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    let attempt = current_repair_attempt(&repair_repo, &conversation_id).await;
    let created = repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_attempt_updated_at: attempt.updated_at,
            effect: AgentWorkspaceRepairEffect::new(
                attempt.id.clone(),
                AgentWorkspaceRepairEffectKind::CreatePr,
                "blocked-retry-open-effect",
                Utc::now(),
            ),
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("open repair effect should be creatable");
    assert!(
        matches!(created, CreateAgentWorkspaceRepairEffectOutcome::Created(_)),
        "fixture must actually leave an open effect, got {created:?}"
    );

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 0,
        "an open create-PR effect must keep the attempt fenced"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// The Edit-mode gate (blueprint E12). `resolve_agent_workspace_publish_target` materializes a
/// worktree for an Ideation workspace with a linked plan branch, so an unattended timer must
/// reject non-Edit workspaces before it ever resolves a publish target.
#[tokio::test]
async fn blocked_repair_base_retry_skips_a_non_edit_workspace() {
    let (_repo, project, mut workspace) = published_git_workspace_fixture();
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(dispatched, 0, "only Edit workspaces are candidates");
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}

/// Proof obligation 8: an *unpublished* Edit workspace with an unsettled blocked attempt is a
/// candidate for both halves. The base-freshness half must still skip it on its settled-repair
/// gate, so exactly one lane acts and there is no double dispatch.
#[tokio::test]
async fn blocked_repair_base_retry_acts_where_the_base_freshness_half_skips_on_its_repair_gate() {
    let (_repo, project, workspace) = unpublished_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let base_freshness_updated =
        run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
            .await
            .expect("base-freshness half should succeed");
    assert_eq!(
        base_freshness_updated, 0,
        "the unsettled-repair gate must keep the base-freshness half out of this workspace"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;

    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("blocked-repair base-retry tick should succeed");
    assert_eq!(dispatched, 1, "exactly one lane may act on this workspace");
}

/// Proof obligation 4: a structurally broken candidate is skipped without failing the tick or
/// starving a healthy candidate in the same pass.
#[tokio::test]
async fn blocked_repair_base_retry_skips_a_broken_candidate_without_blocking_a_healthy_one() {
    let (_repo, project, healthy_workspace) = published_git_workspace_fixture();
    let healthy_conversation_id = healthy_workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();

    let broken_conversation_id = ChatConversationId::new();
    let broken_workspace = AgentConversationWorkspace::new(
        broken_conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        healthy_workspace.base_commit.clone(),
        format!("ralphx/test/{}", broken_conversation_id.as_str()),
        format!(
            "/tmp/ralphx-blocked-retry-missing-worktree-{}",
            broken_conversation_id.as_str()
        ),
    );

    let state = seed_blocked_repair_state(project, healthy_workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);
    state
        .agent_conversation_workspace_repo
        .create_or_update(broken_workspace)
        .await
        .expect("broken workspace should seed");
    seed_blocked_attempt_for(&state, broken_conversation_id.clone()).await;

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("a broken candidate must not fail the tick");
    assert_eq!(
        dispatched, 1,
        "the healthy candidate must still be re-driven in the same pass"
    );
    assert_no_repair_dispatch(&repair_repo, &broken_conversation_id).await;

    let healthy = current_repair_attempt(&repair_repo, &healthy_conversation_id).await;
    assert_eq!(
        healthy.source,
        AgentWorkspaceRepairSource::BaseUpdate,
        "the healthy candidate must dispatch a base-update successor"
    );
}

/// Seeds a second Blocked attempt for an already-seeded workspace, reusing the same shape as
/// `seed_blocked_repair_state`'s attempt.
async fn seed_blocked_attempt_for(state: &AppState, conversation_id: ChatConversationId) {
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id,
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                Utc::now(),
            ),
            reason: "secondary blocked repair fixture".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start secondary repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(mut attempt) = started else {
        panic!("secondary repair attempt must start");
    };
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.next_dispatch_at = None;
    attempt.target_base_commit = Some("stale-base-sha".to_string());
    attempt.pending_reasons = incident_pending_reasons();
    attempt.updated_at = Utc::now();
    state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed secondary blocked attempt");
}

#[tokio::test]
async fn app_handle_blocked_repair_base_retry_tick_skips_when_startup_git_auth_is_pending() {
    let state = AppState::new_test();
    state.startup_git_auth_recovery_state.mark_pending();
    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));

    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("pending startup recovery should skip the scan");

    assert_eq!(dispatched, 0);
}

#[tokio::test]
async fn app_handle_blocked_repair_base_retry_tick_errors_without_execution_state() {
    let app = mock_app_with_state(AppState::new_test());

    let error =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect_err("missing ExecutionState should fail");

    assert_eq!(error, "ExecutionState is not available");
}

/// Proof obligation 9 / the dedupe constraint: `list_recoverable_repair_attempts` returns one row
/// per unsettled attempt, so a conversation can appear more than once (blueprint E13). The lane
/// must collapse them before doing any git work, or it fetches the same repo twice per tick and
/// can supersede the same generation twice. The memory repo enforces one current attempt per
/// conversation, so the duplicate listing is injected at the seam the lane actually reads.
#[tokio::test]
async fn blocked_repair_base_retry_dedupes_multiple_attempt_rows_for_one_conversation() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let mut state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let inner_repair_repo = Arc::clone(&state.agent_workspace_repair_repo);
    state.agent_workspace_repair_repo = Arc::new(ListingErrorRepairRepository::duplicating(
        Arc::clone(&inner_repair_repo),
    ));

    let recoverable = state
        .agent_workspace_repair_repo
        .list_recoverable_repair_attempts()
        .await
        .expect("recoverable attempts should list");
    assert_eq!(
        recoverable
            .iter()
            .filter(|attempt| attempt.conversation_id == conversation_id)
            .count(),
        2,
        "the fixture must actually produce two candidate rows for one conversation"
    );
    let blocked_generation = current_repair_attempt(&inner_repair_repo, &conversation_id)
        .await
        .generation;

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");
    assert_eq!(
        dispatched, 1,
        "two attempt rows for one conversation must produce at most one candidate pass"
    );

    let successor = current_repair_attempt(&inner_repair_repo, &conversation_id).await;
    assert_eq!(
        successor.generation,
        blocked_generation + 1,
        "a duplicated candidate row must not supersede the blocked generation twice"
    );
}

// ────────────────────────────────────────────────────────────────────
// Review-owned worktree exclusion — unattended base updates
// ────────────────────────────────────────────────────────────────────

/// Persists a review monitor that reports the reviewer as in flight.
async fn seed_reviewing_monitor(state: &AppState, workspace: &AgentConversationWorkspace) {
    let mut monitor = crate::domain::entities::AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.status = crate::domain::entities::AgentWorkspaceReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("review monitor should seed");
}

/// Proof obligation 6: the reviewer runs in a child conversation, so this scan's existing
/// active-run gate cannot see it. Without the review gate, an unattended base merge would land
/// under an in-flight review and invalidate the delta being reviewed.
#[tokio::test]
async fn base_freshness_scan_skips_auto_update_while_review_work_owns_the_worktree() {
    let (_repo, project, workspace) = unpublished_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should seed");
    seed_reviewing_monitor(&state, &workspace).await;

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("tick should succeed even when the review gate blocks the update");

    assert_eq!(
        updated, 0,
        "an in-flight review must block the unattended base auto-update"
    );
    let reloaded = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert!(
        reloaded.stale_base_detected_at.is_some(),
        "detection must still run under the review gate"
    );
}

/// A review fixer holds the same worktree as the reviewer, so it excludes the scan just as well.
#[tokio::test]
async fn base_freshness_scan_skips_auto_update_while_a_review_fixer_is_active() {
    let (_repo, project, workspace) = unpublished_git_workspace_fixture();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let state = AppState::new_test();
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should seed");
    let mut monitor = crate::domain::entities::AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.status = crate::domain::entities::AgentWorkspaceReviewMonitorStatus::Blocked;
    monitor.review_fixer_status =
        Some(crate::domain::entities::WORKSPACE_REVIEW_FIXER_STATUS_RUNNING.to_string());
    workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("review monitor should seed");

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let updated = run_agent_workspace_base_freshness_scan_tick_from_app_handle(app.handle())
        .await
        .expect("tick should succeed");

    assert_eq!(
        updated, 0,
        "an active review fixer must block the unattended base auto-update"
    );
}

/// Proof obligation 6, second scan: the blocked-repair retry merges from base into the same
/// worktree and must stand down for review-owned work too.
#[tokio::test]
async fn blocked_repair_base_retry_skips_while_review_work_owns_the_worktree() {
    let (_repo, project, workspace) = published_git_workspace_fixture();
    let conversation_id = workspace.conversation_id.clone();
    let repo_path = Path::new(&project.working_directory).to_path_buf();
    let workspace_for_monitor = workspace.clone();
    let state = seed_blocked_repair_state(project, workspace, |_| {}).await;
    let repair_repo = Arc::clone(&state.agent_workspace_repair_repo);
    seed_reviewing_monitor(&state, &workspace_for_monitor).await;

    commit_file(&repo_path, "drift.txt", "drift\n", "advance base");

    let app = mock_app_with_state_and_execution(state, Arc::new(ExecutionState::new()));
    let dispatched =
        run_agent_workspace_blocked_repair_base_retry_scan_tick_from_app_handle(app.handle())
            .await
            .expect("tick should succeed");

    assert_eq!(
        dispatched, 0,
        "an in-flight review must block the unattended blocked-repair base retry"
    );
    assert_no_repair_dispatch(&repair_repo, &conversation_id).await;
}
