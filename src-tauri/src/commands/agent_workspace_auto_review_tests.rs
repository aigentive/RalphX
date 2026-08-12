use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use super::agent_workspace_auto_review::{
    begin_auto_review, handle_auto_review_workspace_change_result, interactive_slot_key,
    maybe_start_auto_review, maybe_start_auto_review_from_app_handle,
    related_workspace_runtime_is_generating, resolve_auto_review_start_action,
    resolve_workspace_conversation_id_for_review_event, spawn_auto_review_after_workspace_change,
    spawn_auto_review_for_workspace, spawn_auto_review_from_completion_payload, AutoReviewDecision,
    AutoReviewSkipReason, AutoReviewStartAction, AutoReviewTrigger, WorkspaceChangedEmitter,
};
use crate::application::agent_workspace_review::{
    apply_review_artifact_to_monitor, load_agent_workspace_review_context,
};
use crate::application::chat_service::events::AGENT_RUN_COMPLETED;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRun, AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome, ArtifactId, AutomationRunId,
    ChatContextType, ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::review::ReviewSettings;
use crate::domain::services::running_agent_registry::RunningAgentKey;
use crate::infrastructure::tool_paths::resolve_git_cli_path;

fn test_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

fn test_app_with_execution_state(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(state)
        .manage(Arc::new(ExecutionState::new()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new(resolve_git_cli_path())
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_repo() -> (tempfile::TempDir, PathBuf, String) {
    let temp = tempfile::Builder::new()
        .prefix("auto-review-command-")
        .tempdir_in(std::env::current_dir().expect("checkout cwd should resolve"))
        .expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir should be created");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let base_sha = git(&repo, &["rev-parse", "HEAD"]);
    (temp, repo, base_sha)
}

async fn seed_project(state: &AppState, repo: &Path) -> Project {
    let project = Project::new(
        "Auto Review Command".to_string(),
        repo.to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project)
        .await
        .expect("project should persist")
}

fn workspace(
    project: &Project,
    worktree_path: &Path,
    base_commit: Option<String>,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::new(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        base_commit,
        "ralphx/test/auto-review-command".to_string(),
        worktree_path.to_string_lossy().to_string(),
    )
}

async fn seed_workspace_conversation(state: &AppState, workspace: &AgentConversationWorkspace) {
    let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
    conversation.id = workspace.conversation_id.clone();
    conversation.agent_mode = Some(workspace.mode);
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("workspace conversation should persist");
}

async fn seed_child_conversation(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> ChatConversationId {
    let mut child = ChatConversation::new_project(workspace.project_id.clone());
    child.parent_conversation_id = Some(workspace.conversation_id.as_str().to_string());
    let child_id = child.id.clone();
    state
        .chat_conversation_repo
        .create(child)
        .await
        .expect("child conversation should persist");
    child_id
}

fn commit_workspace_delta(repo: &Path) {
    std::fs::write(repo.join("changed.rs"), "pub fn changed() {}\n")
        .expect("changed file should be written");
    git(repo, &["add", "changed.rs"]);
    git(repo, &["commit", "-m", "workspace change"]);
}

fn commit_tracked_base_file(repo: &Path) -> String {
    std::fs::write(repo.join("tracked.rs"), "pub fn tracked() {}\n")
        .expect("tracked file should be written");
    git(repo, &["add", "tracked.rs"]);
    git(repo, &["commit", "-m", "tracked base"]);
    git(repo, &["rev-parse", "HEAD"])
}

fn add_all_workspace_delta_sources(repo: &Path) {
    std::fs::write(repo.join("committed.rs"), "pub fn committed() {}\n")
        .expect("committed file should be written");
    git(repo, &["add", "committed.rs"]);
    git(repo, &["commit", "-m", "committed workspace delta"]);

    std::fs::write(repo.join("staged.rs"), "pub fn staged() {}\n")
        .expect("staged file should be written");
    git(repo, &["add", "staged.rs"]);

    std::fs::write(repo.join("tracked.rs"), "pub fn tracked() { let _ = 1; }\n")
        .expect("tracked file should be modified");
    std::fs::write(repo.join("untracked.rs"), "pub fn untracked() {}\n")
        .expect("untracked file should be written");
}

async fn current_target_monitor(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    status: AgentWorkspaceReviewMonitorStatus,
    outcome: AgentWorkspaceReviewOutcome,
) -> AgentWorkspaceReviewMonitor {
    let context = load_agent_workspace_review_context(state, workspace)
        .await
        .expect("review context should load");
    let target = context.target.expect("workspace should have review target");
    let mut monitor = context.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha,
        target.diff_fingerprint,
        Some("run-1".to_string()),
        ArtifactId::new(),
        1,
        Utc::now(),
        None,
    );
    monitor.status = status;
    monitor.review_outcome = outcome;
    monitor
}

#[test]
fn skip_reason_codes_are_stable() {
    let cases = [
        (AutoReviewSkipReason::WorkspaceMissing, "workspace_missing"),
        (
            AutoReviewSkipReason::InactiveWorkspace,
            "inactive_workspace",
        ),
        (
            AutoReviewSkipReason::NotReviewableMode,
            "not_reviewable_mode",
        ),
        (
            AutoReviewSkipReason::ManualOnlyArchived,
            "manual_only_archived",
        ),
        (
            AutoReviewSkipReason::ManualOnlyTerminalPr,
            "manual_only_terminal_pr",
        ),
        (
            AutoReviewSkipReason::NoReviewableChanges,
            "no_reviewable_changes",
        ),
        (AutoReviewSkipReason::GateNotRequired, "gate_not_required"),
        (
            AutoReviewSkipReason::WorkspaceAutomationOff,
            "workspace_automation_off",
        ),
        (AutoReviewSkipReason::AlreadyReviewing, "already_reviewing"),
        (AutoReviewSkipReason::BlockingFindings, "blocking_findings"),
        (AutoReviewSkipReason::ReviewFailed, "review_failed"),
        (
            AutoReviewSkipReason::RelatedRuntimeGenerating,
            "related_runtime_generating",
        ),
        (AutoReviewSkipReason::StartSkipped, "start_skipped"),
    ];

    for (reason, expected) in cases {
        assert_eq!(reason.as_str(), expected);
    }
}

#[test]
fn auto_review_trigger_codes_are_stable() {
    let cases = [
        (AutoReviewTrigger::AgentCompletion, "agent_completion"),
        (AutoReviewTrigger::BaseUpdate, "base_update"),
    ];

    for (trigger, expected) in cases {
        assert_eq!(trigger.as_str(), expected);
    }
}

#[test]
fn begin_auto_review_deduplicates_until_guard_drops() {
    let conversation_id = ChatConversationId::new();
    let guard = begin_auto_review(&conversation_id).expect("first guard should be acquired");

    assert!(begin_auto_review(&conversation_id).is_none());

    drop(guard);

    assert!(begin_auto_review(&conversation_id).is_some());
}

#[test]
fn interactive_slot_key_uses_project_context() {
    assert_eq!(
        interactive_slot_key("conversation-1"),
        "project/conversation-1"
    );
}

#[test]
fn spawn_auto_review_for_workspace_skips_when_already_in_flight() {
    let app = test_app_with_execution_state(AppState::new_test());
    let conversation_id = ChatConversationId::new();
    let guard = begin_auto_review(&conversation_id).expect("first guard should be acquired");

    spawn_auto_review_for_workspace(
        app.handle().clone(),
        AGENT_RUN_COMPLETED,
        conversation_id.clone(),
    );

    assert!(begin_auto_review(&conversation_id).is_none());
    drop(guard);
}

#[tokio::test]
async fn project_completion_event_with_missing_workspace_resolves_without_starting_review() {
    let app = test_app_with_execution_state(AppState::new_test());
    let conversation_id = ChatConversationId::new();

    spawn_auto_review_from_completion_payload(
        app.handle().clone(),
        AGENT_RUN_COMPLETED,
        conversation_id,
    );

    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
}

#[tokio::test]
async fn project_completion_event_resolves_workspace_and_runs_skip_path() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    seed_workspace_conversation(&state, &workspace).await;
    let app = test_app_with_execution_state(state);

    spawn_auto_review_from_completion_payload(
        app.handle().clone(),
        AGENT_RUN_COMPLETED,
        workspace.conversation_id.clone(),
    );

    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Some(guard) = begin_auto_review(&workspace.conversation_id) {
            drop(guard);
            return;
        }
    }
    panic!("spawn guard should release");
}

#[tokio::test]
async fn spawn_auto_review_for_workspace_runs_async_skip_path() {
    let app = test_app_with_execution_state(AppState::new_test());
    let conversation_id = ChatConversationId::new();

    spawn_auto_review_for_workspace(
        app.handle().clone(),
        AGENT_RUN_COMPLETED,
        conversation_id.clone(),
    );

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let guard = begin_auto_review(&conversation_id).expect("spawn guard should release");
    drop(guard);
}

#[tokio::test]
async fn auto_review_start_action_starts_first_review_for_dirty_workspace_without_artifact() {
    let (_temp, repo, _initial_base_sha) = init_repo();
    let base_sha = commit_tracked_base_file(&repo);
    add_all_workspace_delta_sources(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("review context should load");
    let target = context.target.expect("dirty workspace should have target");
    let changed_paths = target
        .review_packet
        .changed_files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    assert!(changed_paths.contains(&"committed.rs"));
    assert!(changed_paths.contains(&"staged.rs"));
    assert!(changed_paths.contains(&"tracked.rs"));
    assert!(changed_paths.contains(&"untracked.rs"));
    assert_eq!(
        context.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Required
    );
    assert!(context.monitor.review_artifact_id.is_none());

    let action = resolve_auto_review_start_action(&state, &execution_state, &workspace)
        .await
        .expect("auto-review action should resolve");

    assert_eq!(action, AutoReviewStartAction::Start);
}

#[tokio::test]
async fn review_automation_start_honors_enabled_workspace_override_when_globals_are_off() {
    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(&project, &repo, Some(base_sha));
    workspace.review_automation_override = Some(true);
    seed_workspace_conversation(&state, &workspace).await;
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should update");

    assert_eq!(
        resolve_auto_review_start_action(&state, &execution_state, &workspace)
            .await
            .expect("auto-review action should resolve"),
        AutoReviewStartAction::Start
    );
}

#[tokio::test]
async fn review_automation_start_reports_explicit_workspace_opt_out() {
    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(&project, &repo, Some(base_sha));
    workspace.review_automation_override = Some(false);
    seed_workspace_conversation(&state, &workspace).await;

    assert_eq!(
        resolve_auto_review_start_action(&state, &execution_state, &workspace)
            .await
            .expect("auto-review action should resolve"),
        AutoReviewStartAction::Skip(AutoReviewSkipReason::WorkspaceAutomationOff)
    );
}

#[tokio::test]
async fn auto_review_start_action_starts_when_existing_review_is_outdated() {
    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;
    let monitor = current_target_monitor(
        &state,
        &workspace,
        AgentWorkspaceReviewMonitorStatus::Ready,
        AgentWorkspaceReviewOutcome::Passed,
    )
    .await;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("current passing monitor should persist");
    std::fs::write(repo.join("new-untracked.rs"), "pub fn newer() {}\n")
        .expect("new untracked file should be written");

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("review context should load");
    assert!(context.is_outdated);
    assert_eq!(
        context.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Required
    );

    let action = resolve_auto_review_start_action(&state, &execution_state, &workspace)
        .await
        .expect("auto-review action should resolve");

    assert_eq!(action, AutoReviewStartAction::Start);
}

#[tokio::test]
async fn auto_review_skips_plan_workspace_without_spawning_review() {
    let (_temp, repo, base_sha) = init_repo();
    add_all_workspace_delta_sources(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(&project, &repo, Some(base_sha));
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
    conversation.id = workspace.conversation_id.clone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Plan);
    conversation.automation_run_id = Some(AutomationRunId::from_string("run-1"));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("automation conversation should persist");

    let decision = maybe_start_auto_review(&state, &execution_state, &workspace)
        .await
        .expect("auto-review should resolve");

    assert_eq!(
        decision,
        AutoReviewDecision::Skipped(AutoReviewSkipReason::NotReviewableMode)
    );
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("workspace monitor lookup should succeed");
    assert!(
        monitor.is_none(),
        "PLAN turn completion must not create a review monitor"
    );
}

#[tokio::test]
async fn auto_review_skips_ordinary_plan_workspace_before_conversation_lookup() {
    let (_temp, repo, base_sha) = init_repo();
    add_all_workspace_delta_sources(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(&project, &repo, Some(base_sha));
    workspace.mode = AgentConversationWorkspaceMode::Plan;

    let action = resolve_auto_review_start_action(&state, &execution_state, &workspace)
        .await
        .expect("PLAN mode should produce a normal skip decision");

    assert_eq!(
        action,
        AutoReviewStartAction::Skip(AutoReviewSkipReason::NotReviewableMode)
    );
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[test]
fn base_update_auto_review_trigger_dedupes_when_workspace_is_already_in_flight() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = Project::new(
        "Auto Review Base Update".to_string(),
        repo.to_string_lossy().to_string(),
    );
    let workspace = workspace(&project, &repo, Some(base_sha));
    let guard = begin_auto_review(&workspace.conversation_id).expect("guard should be acquired");

    assert!(!spawn_auto_review_after_workspace_change(
        state,
        execution_state,
        workspace.clone(),
        AutoReviewTrigger::BaseUpdate,
        None,
    ));
    assert!(begin_auto_review(&workspace.conversation_id).is_none());
    drop(guard);
}

#[test]
fn base_update_auto_review_emits_workspace_changed_when_review_starts() {
    let conversation_id = ChatConversationId::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let emitter: WorkspaceChangedEmitter = Box::new(move |conversation_id| {
        tx.send(conversation_id.as_str().to_string())
            .expect("workspace changed event should send");
    });

    assert!(handle_auto_review_workspace_change_result(
        AutoReviewTrigger::BaseUpdate,
        &conversation_id,
        Ok(AutoReviewDecision::Started),
        Some(&emitter),
    ));

    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1))
            .expect("workspace changed event should emit after review starts"),
        conversation_id.as_str()
    );
}

#[test]
fn base_update_auto_review_does_not_emit_workspace_changed_when_review_skips() {
    let conversation_id = ChatConversationId::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let emitter: WorkspaceChangedEmitter = Box::new(move |conversation_id| {
        tx.send(conversation_id.as_str().to_string())
            .expect("workspace changed event should send");
    });

    assert!(!handle_auto_review_workspace_change_result(
        AutoReviewTrigger::BaseUpdate,
        &conversation_id,
        Ok(AutoReviewDecision::Skipped(
            AutoReviewSkipReason::GateNotRequired,
        )),
        Some(&emitter),
    ));

    assert!(rx.try_recv().is_err());
}

#[test]
fn auto_review_workspace_change_result_handles_started_without_emitter() {
    let conversation_id = ChatConversationId::new();

    assert!(handle_auto_review_workspace_change_result(
        AutoReviewTrigger::AgentCompletion,
        &conversation_id,
        Ok(AutoReviewDecision::Started),
        None,
    ));
}

#[test]
fn auto_review_workspace_change_result_handles_errors_without_emitting() {
    let conversation_id = ChatConversationId::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let emitter: WorkspaceChangedEmitter = Box::new(move |conversation_id| {
        tx.send(conversation_id.as_str().to_string())
            .expect("workspace changed event should send");
    });

    assert!(!handle_auto_review_workspace_change_result(
        AutoReviewTrigger::AgentCompletion,
        &conversation_id,
        Err("review start failed".to_string()),
        Some(&emitter),
    ));

    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn auto_review_skips_manual_only_and_non_reviewable_workspaces_before_loading_context() {
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = Project::new(
        "Auto Review Preconditions".to_string(),
        "/tmp/ralphx-auto-review-preconditions".to_string(),
    );
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::new(),
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base".to_string()),
        "ralphx/test/preconditions".to_string(),
        "/tmp/ralphx-auto-review-preconditions".to_string(),
    );

    let mut archived = workspace.clone();
    archived.status = AgentConversationWorkspaceStatus::Archived;
    assert_eq!(
        maybe_start_auto_review(&state, &execution_state, &archived)
            .await
            .expect("archived decision should load"),
        AutoReviewDecision::Skipped(AutoReviewSkipReason::ManualOnlyArchived)
    );

    let mut missing = workspace.clone();
    missing.status = AgentConversationWorkspaceStatus::Missing;
    assert_eq!(
        maybe_start_auto_review(&state, &execution_state, &missing)
            .await
            .expect("missing decision should load"),
        AutoReviewDecision::Skipped(AutoReviewSkipReason::InactiveWorkspace)
    );

    let mut review_pr = workspace.clone();
    review_pr.mode = AgentConversationWorkspaceMode::ReviewPr;
    assert_eq!(
        maybe_start_auto_review(&state, &execution_state, &review_pr)
            .await
            .expect("review-pr decision should load"),
        AutoReviewDecision::Skipped(AutoReviewSkipReason::NotReviewableMode)
    );

    let mut merged_pr = workspace;
    merged_pr.publication_pr_status = Some("merged".to_string());
    assert_eq!(
        maybe_start_auto_review(&state, &execution_state, &merged_pr)
            .await
            .expect("terminal PR decision should load"),
        AutoReviewDecision::Skipped(AutoReviewSkipReason::ManualOnlyTerminalPr)
    );
}

#[tokio::test]
async fn auto_review_skips_when_no_reviewable_changes_exist() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;

    let decision = maybe_start_auto_review(&state, &execution_state, &workspace)
        .await
        .expect("decision should load");

    assert_eq!(
        decision,
        AutoReviewDecision::Skipped(AutoReviewSkipReason::NoReviewableChanges)
    );
}

#[tokio::test]
async fn auto_review_skips_required_gate_when_workspace_review_not_required() {
    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should update");

    let decision = maybe_start_auto_review(&state, &execution_state, &workspace)
        .await
        .expect("decision should load");

    assert_eq!(
        decision,
        AutoReviewDecision::Skipped(AutoReviewSkipReason::GateNotRequired)
    );
}

#[tokio::test]
async fn app_handle_auto_review_adapter_reports_missing_state_and_missing_workspace() {
    let no_state_app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");
    let missing_state =
        maybe_start_auto_review_from_app_handle(no_state_app.handle(), ChatConversationId::new())
            .await
            .expect_err("missing AppState should error");
    assert_eq!(missing_state, "AppState is not available");

    let no_execution_app = test_app(AppState::new_test());
    let missing_execution = maybe_start_auto_review_from_app_handle(
        no_execution_app.handle(),
        ChatConversationId::new(),
    )
    .await
    .expect_err("missing ExecutionState should error");
    assert_eq!(missing_execution, "ExecutionState is not available");

    let app = test_app_with_execution_state(AppState::new_test());
    let missing_workspace =
        maybe_start_auto_review_from_app_handle(app.handle(), ChatConversationId::new())
            .await
            .expect("missing workspace should be a skip decision");
    assert_eq!(
        missing_workspace,
        AutoReviewDecision::Skipped(AutoReviewSkipReason::WorkspaceMissing)
    );
}

#[tokio::test]
async fn auto_review_skips_terminal_review_gate_states() {
    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;

    for (monitor_status, outcome, expected) in [
        (
            AgentWorkspaceReviewMonitorStatus::Reviewing,
            AgentWorkspaceReviewOutcome::None,
            AutoReviewSkipReason::AlreadyReviewing,
        ),
        (
            AgentWorkspaceReviewMonitorStatus::Ready,
            AgentWorkspaceReviewOutcome::Passed,
            AutoReviewSkipReason::GateNotRequired,
        ),
        (
            AgentWorkspaceReviewMonitorStatus::Ready,
            AgentWorkspaceReviewOutcome::Blocking,
            AutoReviewSkipReason::BlockingFindings,
        ),
        (
            AgentWorkspaceReviewMonitorStatus::Ready,
            AgentWorkspaceReviewOutcome::RunFailed,
            AutoReviewSkipReason::ReviewFailed,
        ),
    ] {
        let monitor = current_target_monitor(&state, &workspace, monitor_status, outcome).await;
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("monitor should persist");

        let decision = maybe_start_auto_review(&state, &execution_state, &workspace)
            .await
            .expect("decision should load");

        assert_eq!(decision, AutoReviewDecision::Skipped(expected));
    }
}

#[tokio::test]
async fn auto_review_skips_required_gate_when_related_runtime_is_generating() {
    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;
    state
        .agent_run_repo
        .create(AgentRun::new(workspace.conversation_id.clone()))
        .await
        .expect("active workspace run should persist");

    let decision = maybe_start_auto_review(&state, &execution_state, &workspace)
        .await
        .expect("decision should load");

    assert_eq!(
        decision,
        AutoReviewDecision::Skipped(AutoReviewSkipReason::RelatedRuntimeGenerating)
    );
}

#[tokio::test]
async fn related_workspace_runtime_treats_running_child_as_generating_unless_ipr_is_idle() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;
    let child_id = seed_child_conversation(&state, &workspace).await;
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.review_conversation_id = Some(ChatConversationId::new());

    assert!(!related_workspace_runtime_is_generating(
        &state,
        &execution_state,
        &workspace,
        &monitor
    )
    .await
    .expect("runtime state should load"));

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(ChatContextType::Project.to_string(), child_id.as_str()),
            0,
            child_id.as_str().to_string(),
            String::new(),
            None,
            None,
        )
        .await;

    assert!(related_workspace_runtime_is_generating(
        &state,
        &execution_state,
        &workspace,
        &monitor
    )
    .await
    .expect("active child should block"));

    let child_id_text = child_id.as_str();
    execution_state.mark_interactive_idle(&interactive_slot_key(&child_id_text));

    assert!(!related_workspace_runtime_is_generating(
        &state,
        &execution_state,
        &workspace,
        &monitor
    )
    .await
    .expect("idle child should not block"));
}

#[tokio::test]
async fn related_workspace_runtime_detects_active_run_without_registry_entry() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;
    let monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    let run = AgentRun::new(workspace.conversation_id.clone());
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("active run should persist");

    assert!(related_workspace_runtime_is_generating(
        &state,
        &execution_state,
        &workspace,
        &monitor
    )
    .await
    .expect("active run should be visible"));
}

#[tokio::test]
async fn related_workspace_runtime_checks_registry_run_status_when_run_id_is_present() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;
    let monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                workspace.conversation_id.as_str(),
            ),
            0,
            workspace.conversation_id.as_str(),
            "missing-run".to_string(),
            None,
            None,
        )
        .await;
    assert!(related_workspace_runtime_is_generating(
        &state,
        &execution_state,
        &workspace,
        &monitor
    )
    .await
    .expect("missing registered run should be treated as generating"));

    state
        .running_agent_registry
        .unregister(
            &RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                workspace.conversation_id.as_str(),
            ),
            "missing-run",
        )
        .await;
    let run = state
        .agent_run_repo
        .create(AgentRun::new(workspace.conversation_id.clone()))
        .await
        .expect("run should persist");
    state
        .agent_run_repo
        .complete(&run.id)
        .await
        .expect("run should complete");
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                workspace.conversation_id.as_str(),
            ),
            0,
            workspace.conversation_id.as_str(),
            run.id.as_str(),
            None,
            None,
        )
        .await;

    assert!(!related_workspace_runtime_is_generating(
        &state,
        &execution_state,
        &workspace,
        &monitor
    )
    .await
    .expect("completed registered run should not be generating"));
}

#[tokio::test]
async fn resolve_review_event_workspace_returns_direct_workspace_conversation() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let app = test_app(state);

    let resolved = resolve_workspace_conversation_id_for_review_event(
        app.handle(),
        &workspace.conversation_id,
    )
    .await
    .expect("workspace resolution should succeed");

    assert_eq!(resolved, Some(workspace.conversation_id));
}

#[tokio::test]
async fn resolve_review_event_workspace_maps_child_conversation_to_parent_workspace() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let child_id = seed_child_conversation(&state, &workspace).await;
    let app = test_app(state);

    let resolved = resolve_workspace_conversation_id_for_review_event(app.handle(), &child_id)
        .await
        .expect("child resolution should succeed");

    assert_eq!(resolved, Some(workspace.conversation_id));
}

#[tokio::test]
async fn resolve_review_event_workspace_ignores_unrelated_and_missing_conversations() {
    let (_temp, repo, _base_sha) = init_repo();
    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let unrelated = ChatConversation::new_project(project.id);
    let unrelated_id = unrelated.id.clone();
    state
        .chat_conversation_repo
        .create(unrelated)
        .await
        .expect("unrelated conversation should persist");
    let app = test_app(state);

    let unresolved =
        resolve_workspace_conversation_id_for_review_event(app.handle(), &unrelated_id)
            .await
            .expect("unrelated resolution should succeed");
    let missing = resolve_workspace_conversation_id_for_review_event(
        app.handle(),
        &ChatConversationId::new(),
    )
    .await
    .expect("missing resolution should succeed");

    assert!(unresolved.is_none());
    assert!(missing.is_none());
}

#[tokio::test]
async fn resolve_review_event_workspace_reports_missing_app_state() {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let error = resolve_workspace_conversation_id_for_review_event(
        app.handle(),
        &ChatConversationId::new(),
    )
    .await
    .expect_err("missing AppState should error");

    assert_eq!(error, "AppState is not available");
}

// ── Repair-aware auto-review deferral ────────────────────────────────────

/// Reviewing a head that a publish repair is actively rewriting wastes the run and inflates the
/// convergence counter, so dispatch defers instead. The existing repair-boundary starter
/// re-dispatches once the repair settles, which is why deferral is safe.
#[tokio::test]
async fn auto_review_defers_while_a_durable_repair_attempt_is_active() {
    use crate::domain::entities::{
        AgentWorkspaceRepairAttempt, AgentWorkspaceRepairContinuation, AgentWorkspaceRepairSource,
    };
    use crate::domain::repositories::StartOrJoinAgentWorkspaceRepairAttempt;

    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;
    // The repair aggregate keys off a persisted workspace row.
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    // Both directions in one test: reviewable before the repair exists, deferred after.
    assert_eq!(
        resolve_auto_review_start_action(&state, &execution_state, &workspace)
            .await
            .expect("auto-review action should resolve"),
        AutoReviewStartAction::Start,
        "this workspace is otherwise reviewable, so the skip below is attributable to the repair"
    );

    state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                workspace.conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                Utc::now(),
            ),
            reason: "repair in flight".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("repair attempt should start");

    assert_eq!(
        resolve_auto_review_start_action(&state, &execution_state, &workspace)
            .await
            .expect("auto-review action should resolve"),
        AutoReviewStartAction::Skip(AutoReviewSkipReason::RepairAttemptActive)
    );
}

#[tokio::test]
async fn auto_review_defers_while_the_review_fixer_is_active() {
    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("review context should load");
    let mut monitor = context.monitor;
    monitor.review_fixer_status = Some("running".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("fixer-active monitor should persist");

    assert_eq!(
        resolve_auto_review_start_action(&state, &execution_state, &workspace)
            .await
            .expect("auto-review action should resolve"),
        AutoReviewStartAction::Skip(AutoReviewSkipReason::ReviewFixerActive)
    );
}

/// `cycle_capped` means automatic fixing is switched off, not that a fixer is running.
#[tokio::test]
async fn auto_review_proceeds_when_the_fixer_is_only_cycle_capped() {
    let (_temp, repo, base_sha) = init_repo();
    commit_workspace_delta(&repo);
    let state = AppState::new_test();
    let execution_state = ExecutionState::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(&project, &repo, Some(base_sha));
    seed_workspace_conversation(&state, &workspace).await;

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("review context should load");
    let mut monitor = context.monitor;
    monitor.review_fixer_status = Some("cycle_capped".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("cycle-capped monitor should persist");

    assert_eq!(
        resolve_auto_review_start_action(&state, &execution_state, &workspace)
            .await
            .expect("auto-review action should resolve"),
        AutoReviewStartAction::Start
    );
}
